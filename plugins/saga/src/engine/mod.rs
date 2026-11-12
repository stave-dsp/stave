//! The saga ENGINE — a two-machine ambient generator modelled on a
//! dual-DSP reverse/reverb pedal (docs/saga.md). The engine is its
//! COMPONENTS; `process` below is the whole per-block dance:
//! * `motion.rs` — `Motion`: the clock — base, DIVE/RISE glide, MOD LFO.
//! * `clock.rs` — `ClockDomain`: host rate ⇄ the 32 kHz core clock.
//! * `programs.rs` — `Programs`: the five programs (two machines in series,
//!   two feedback paths), built from the cells:
//!   `reverse.rs` (ReverseLine), `tank.rs` (Tank), `transpose.rs`
//!   (Transposer), `path.rs` (AnalogPath).
//! * `wet.rs` — `Wet`: TONE, WIDTH, the soft ceiling.
//! * `telemetry.rs` — `Telemetry`: clock + tail out.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{DebugBus, Environment, ProcessCtx, Tail};

use crate::params::SagaParams;

mod clock;
mod motion;
mod path;
mod programs;
mod reverse;
mod tank;
mod telemetry;
mod transpose;
mod wet;

use self::clock::ClockDomain;
use self::programs::{ms_to_ticks, Knobs, Programs, MAX_TIME_MS};
use self::motion::Motion;
use self::telemetry::Telemetry;
use self::wet::Wet;

/// TIME's floor in ms (params.rs) — program IV reads the knob's
/// position between the two.
const MIN_TIME_MS: f32 = 50.0;
/// Beats per SYNC division (index 0 = FREE, unused).
const SYNC_BEATS: [f32; 7] = [0.0, 2.0, 1.0, 0.75, 0.5, 1.0 / 3.0, 0.25];
/// A program change fades the wet out, wipes the core, fades back.
const SWITCH_FADE_MS: f32 = 25.0;

#[inline]
fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The render state of every gliding knob — smoothing is the
/// processor's (one `ContinuousControl` per declared glide).
struct Controls {
    feedback: ContinuousControl,
    regen: ContinuousControl,
    mod_depth: ContinuousControl,
    mix: ContinuousControl,
    x: ContinuousControl,
    time: ContinuousControl,
    tone: ContinuousControl,
    width: ContinuousControl,
    mod_rate: ContinuousControl,
    output: ContinuousControl,
}

impl Controls {
    fn new(p: &SagaParams) -> Self {
        Self {
            feedback: ContinuousControl::new(&p.feedback),
            regen: ContinuousControl::new(&p.regen),
            mod_depth: ContinuousControl::new(&p.mod_depth),
            mix: ContinuousControl::new(&p.mix),
            x: ContinuousControl::new(&p.x),
            time: ContinuousControl::new(&p.time),
            tone: ContinuousControl::new(&p.tone),
            width: ContinuousControl::new(&p.width),
            mod_rate: ContinuousControl::new(&p.mod_rate),
            output: ContinuousControl::new(&p.output),
        }
    }

    fn prepare(&mut self, sr: f64, p: &SagaParams) {
        self.feedback.prepare(sr, &p.feedback);
        self.regen.prepare(sr, &p.regen);
        self.mod_depth.prepare(sr, &p.mod_depth);
        self.mix.prepare(sr, &p.mix);
        self.x.prepare(sr, &p.x);
        self.time.prepare(sr, &p.time);
        self.tone.prepare(sr, &p.tone);
        self.width.prepare(sr, &p.width);
        self.mod_rate.prepare(sr, &p.mod_rate);
        self.output.prepare(sr, &p.output);
    }
}

pub struct SagaEngine {
    params: Arc<SagaParams>,
    controls: Controls,
    sr: f32,
    /// The program the core is wired as (lags `mode` across a switch).
    mode: usize,
    /// Wet fade 0..1 around program switches.
    fade: f32,
    motion: Motion,
    clock: ClockDomain,
    programs: Programs,
    wet: Wet,
    telemetry: Telemetry,
    last_clock: f32,
}

impl stave_core::Processor for SagaEngine {
    type Params = SagaParams;

    fn new(params: Arc<SagaParams>) -> Self {
        Self {
            controls: Controls::new(&params),
            params,
            sr: 48_000.0,
            mode: 0,
            fade: 1.0,
            motion: Motion::new(),
            clock: ClockDomain::new(),
            programs: Programs::new(),
            wet: Wet::new(),
            telemetry: Telemetry::new(),
            last_clock: 1.0,
        }
    }

    fn activate(&mut self, env: Environment) {
        self.sr = env.sample_rate;
        self.controls.prepare(f64::from(env.sample_rate), &self.params);
        self.mode = self.params.mode.index();
        self.fade = 1.0;
        self.last_clock = 1.0;
        self.motion.prepare(env.sample_rate);
        self.clock.prepare(env.sample_rate);
        self.programs.prepare();
        self.wet.prepare(env.sample_rate);
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.telemetry.attach(bus);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Bind: the program, its voicing, the converter filters at
        //    the clock the last block ended on, and where TIME comes
        //    from (the knob, or the host tempo).
        let wanted = p.mode.index();
        self.programs.voice(self.mode, p.x.value());
        self.clock.set_clock(self.last_clock);
        let synced_ms = match (p.sync.index(), ctx.transport.as_ref().and_then(|t| t.tempo_bpm)) {
            (0, _) | (_, None) => None,
            (div, Some(bpm)) => Some(
                (60_000.0 / bpm as f32 * SYNC_BEATS[div]).clamp(MIN_TIME_MS, MAX_TIME_MS),
            ),
        };
        let shape = p.mod_shape.index();
        let (shift, ramp, held) = (p.shift.value(), p.ramp.value(), p.dive.value());

        // 2. The audio block.
        let c = &mut self.controls;
        let mut time_ramp = c.time.begin_block(ctx.params, &p.time);
        let mut fb_ramp = c.feedback.begin_block(ctx.params, &p.feedback);
        let mut regen_ramp = c.regen.begin_block(ctx.params, &p.regen);
        let mut x_ramp = c.x.begin_block(ctx.params, &p.x);
        let mut mix_ramp = c.mix.begin_block(ctx.params, &p.mix);
        let mut tone_ramp = c.tone.begin_block(ctx.params, &p.tone);
        let mut width_ramp = c.width.begin_block(ctx.params, &p.width);
        let mut depth_ramp = c.mod_depth.begin_block(ctx.params, &p.mod_depth);
        let mut rate_ramp = c.mod_rate.begin_block(ctx.params, &p.mod_rate);
        let mut out_ramp = c.output.begin_block(ctx.params, &p.output);
        let fade_step = 1000.0 / (SWITCH_FADE_MS * self.sr);
        let (motion, clock, programs, wet) = (&mut self.motion, &mut self.clock, &mut self.programs, &mut self.wet);
        let (mode, fade) = (&mut self.mode, &mut self.fade);
        let mut clk = self.last_clock;
        let mut wet_peak = 0.0f32;

        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        lanes.run(|fr| {
            let knob_ms = time_ramp.next();
            let time_ms = synced_ms.unwrap_or(knob_ms);
            let k = Knobs {
                feedback: fb_ramp.next(),
                regen: regen_ramp.next(),
                x: x_ramp.next(),
                len: ms_to_ticks(time_ms),
            };
            let base = if *mode == 3 {
                Motion::stretch((time_ms / MIN_TIME_MS).ln() / (MAX_TIME_MS / MIN_TIME_MS).ln())
            } else {
                1.0
            };
            clk = motion.tick(base, shift, ramp, held, depth_ramp.next(), rate_ramp.next(), shape);

            let dry = if active >= 2 { [fr.input(0), fr.input(1)] } else { [fr.input(0); 2] };
            let w = clock.process(dry, clk, |xi| programs.tick(xi, k));
            let w = wet.process(w, tone_ramp.next(), width_ramp.next());

            // Program switch: fade out, rewire on silence, fade in.
            if wanted != *mode {
                *fade -= fade_step;
                if *fade <= 0.0 {
                    *fade = 0.0;
                    *mode = wanted;
                    programs.reset();
                    programs.voice(wanted, k.x);
                }
            } else if *fade < 1.0 {
                *fade = (*fade + fade_step).min(1.0);
            }

            let m = mix_ramp.next() * core::f32::consts::FRAC_PI_2;
            let (dry_g, wet_g) = (m.cos(), m.sin() * *fade);
            let g = db_to_amp(out_ramp.next());
            wet_peak = wet_peak.max(w[0].abs()).max(w[1].abs());
            if active >= 2 {
                fr.write(0, (dry[0] * dry_g + w[0] * wet_g) * g);
                fr.write(1, (dry[1] * dry_g + w[1] * wet_g) * g);
            } else if active == 1 {
                fr.write(0, (dry[0] * dry_g + 0.5 * (w[0] + w[1]) * wet_g) * g);
            }
        });
        self.last_clock = clk;

        // 3. What happened, reported out.
        self.telemetry.publish(clk, wet_peak);
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    /// The loops are soft-clipped, so near-unity feedback sustains
    /// indefinitely; below that the longest tank (25 s RT60) bounds it.
    fn tail(&self) -> Tail {
        let p = &self.params;
        if p.feedback.value() > 0.9 || p.regen.value() > 0.9 {
            Tail::Infinite
        } else {
            Tail::Samples((30.0 * self.sr) as u32)
        }
    }
}
