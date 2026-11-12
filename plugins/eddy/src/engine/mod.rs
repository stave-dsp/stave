//! The eddy ENGINE — a feedback delay whose read head breathes, with
//! four feedback LAWS over one M/S delay substrate (the editor's
//! tabs). The engine is its COMPONENTS — read the fields, know the
//! design; `process` below is the whole per-block dance through them:
//! * `echo.rs` — `Echo`: the shared M/S delay lanes + the classic law.
//! * `motion.rs` — `Motion`: the LFO + the recipe that shapes it.
//! * `circuit.rs` — CIRCUIT: a physics cell at the write head.
//! * `motor.rs` — MOTOR: the wet tail loads the transport; time sags.
//! * `shift.rs` — SHIFT: every repeat transposed by the dialed interval.
//! * `orbit.rs` — ORBIT: each repeat rotates through the image.
//! * `diffusion.rs` — the second tabbed stage, in-loop on the read
//!   side: PLATE / PRISM / DUST / MESH — crank DIFFUSE and the
//!   delay grows into one integrated reverberant system.
//! * `telemetry.rs` — `Telemetry`: wave cycle, cursor, tail energy.

extern crate alloc;
use alloc::sync::Arc;
use stave_core::params::ContinuousControl;

use stave_core::{DebugBus, Environment, ProcessCtx, Tail};
use stave_dsp::db_to_amp;

use crate::params::EddyParams;

mod circuit;
mod diffusion;
mod echo;
mod motion;
mod motor;
mod orbit;
mod shift;
mod telemetry;

use circuit::Circuit;
use diffusion::Diffusion;
use echo::Echo;
use motion::{Motion, DEPTH_MS};
use motor::Motor;
use shift::Shift;
use telemetry::Telemetry;

/// Longest dialable delay; the line's capacity adds headroom for the
/// glide, the LFO throw, and MOTOR's full sag (SHIFT's transposer
/// owns its own window lines).
const MAX_TIME_MS: f32 = 2_000.0;

/// Two independent levels, one spelling everywhere. At `dry_g = 1`,
/// `wet_g = 0` this returns `dry` EXACTLY (the bit-dry law); at
/// `dry_g = 0` the dry contributes nothing (the no-dry law).
#[inline]
fn blend(dry: f32, wet: f32, dry_g: f32, wet_g: f32) -> f32 {
    dry * dry_g + wet * wet_g
}

/// The per-frame inputs every algorithm law shares: the stereo frame,
/// the read head (base + wobble + spread), the regen gain, and the
/// diffusion sends. Algorithm-specific knobs stay positional — this
/// carries only what is common to all four laws.
#[derive(Copy, Clone)]
pub(crate) struct FrameCtx {
    pub l: f32,
    pub r: f32,
    /// Read-head center in samples (already time-knob glided).
    pub base: f32,
    /// The motion recipe's throw this frame, in samples.
    pub wobble: f32,
    /// Where the wobble lives: 0 = mid, 1 = side.
    pub spread: f32,
    /// Regeneration gain (capped below unity by declaration).
    pub fb: f32,
    /// Field send for the EAR (what you hear).
    pub diffuse: f32,
    /// Field send for the LOOP (what regenerates).
    pub bleed: f32,
}

pub struct EddyEngine {
    params: Arc<EddyParams>,
    time_control: ContinuousControl,
    feedback_control: ContinuousControl,
    dry_control: ContinuousControl,
    wet_control: ContinuousControl,
    output_control: ContinuousControl,
    rate_control: ContinuousControl,
    depth_control: ContinuousControl,
    spread_control: ContinuousControl,
    heat_control: ContinuousControl,
    sag_control: ContinuousControl,
    shift_control: ContinuousControl,
    grain_control: ContinuousControl,
    splice_control: ContinuousControl,
    orbit_control: ContinuousControl,
    diffuse_control: ContinuousControl,
    bleed_control: ContinuousControl,
    sr: f32,
    /// The delay lanes (mid/side) and their regeneration.
    echo: Echo,
    /// The read head's breath: LFO × recipe.
    motion: Motion,
    /// CIRCUIT's cells, MOTOR's transport, SHIFT's transposer —
    /// each algorithm keeps its own state; only the active one ticks.
    circuit: Circuit,
    motor: Motor,
    shift: Shift,
    /// The in-loop diffusion stage (the second tab bar).
    diffusion: Diffusion,
    /// Everything reported out (wave cycle, phase cursor, tail).
    telemetry: Telemetry,
}

impl stave_core::Processor for EddyEngine {
    type Params = EddyParams;

    fn new(params: Arc<EddyParams>) -> Self {
        Self {
            time_control: ContinuousControl::new(&params.time),
            feedback_control: ContinuousControl::new(&params.feedback),
            dry_control: ContinuousControl::new(&params.dry),
            wet_control: ContinuousControl::new(&params.wet),
            output_control: ContinuousControl::new(&params.output),
            rate_control: ContinuousControl::new(&params.rate),
            depth_control: ContinuousControl::new(&params.depth),
            spread_control: ContinuousControl::new(&params.spread),
            heat_control: ContinuousControl::new(&params.heat),
            sag_control: ContinuousControl::new(&params.sag),
            shift_control: ContinuousControl::new(&params.shift),
            grain_control: ContinuousControl::new(&params.grain),
            splice_control: ContinuousControl::new(&params.splice),
            orbit_control: ContinuousControl::new(&params.orbit),
            diffuse_control: ContinuousControl::new(&params.diffuse),
            bleed_control: ContinuousControl::new(&params.bleed),
            params,
            sr: 48_000.0,
            echo: Echo::new(),
            motion: Motion::new(),
            circuit: Circuit::new(),
            motor: Motor::new(),
            shift: Shift::new(),
            diffusion: Diffusion::new(),
            telemetry: Telemetry::new(),
        }
    }

    fn activate(&mut self, env: Environment) {
        let rate = f64::from(env.sample_rate);
        self.time_control.prepare(rate, &self.params.time);
        self.feedback_control.prepare(rate, &self.params.feedback);
        self.dry_control.prepare(rate, &self.params.dry);
        self.wet_control.prepare(rate, &self.params.wet);
        self.output_control.prepare(rate, &self.params.output);
        self.rate_control.prepare(rate, &self.params.rate);
        self.depth_control.prepare(rate, &self.params.depth);
        self.spread_control.prepare(rate, &self.params.spread);
        self.heat_control.prepare(rate, &self.params.heat);
        self.sag_control.prepare(rate, &self.params.sag);
        self.shift_control.prepare(rate, &self.params.shift);
        self.grain_control.prepare(rate, &self.params.grain);
        self.splice_control.prepare(rate, &self.params.splice);
        self.orbit_control.prepare(rate, &self.params.orbit);
        self.diffuse_control.prepare(rate, &self.params.diffuse);
        self.bleed_control.prepare(rate, &self.params.bleed);
        self.sr = env.sample_rate;
        let reach_ms = MAX_TIME_MS * (1.0 + motor::SAG_MAX) + DEPTH_MS;
        let cap = (reach_ms * 0.001 * env.sample_rate) as usize + 8;
        self.echo.prepare(cap);
        self.motion
            .prepare(env.sample_rate, self.params.rate.value());
        self.circuit.prepare(env.sample_rate);
        self.motor.prepare(env.sample_rate);
        self.shift.prepare(env.sample_rate);
        self.diffusion.prepare(env.sample_rate);
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.telemetry.attach(bus);
    }

    /// The per-block orchestration — three named phases:
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Param changes become structure: the motion recipe
        //    re-resolves only when its knobs moved (republishing the
        //    wave view's picture), circuit voices reset on switch,
        //    and the block-rate machine constants settle.
        if self.motion.apply_changes(&p, self.sr) {
            let (motion, sr) = (&self.motion, self.sr);
            self.telemetry
                .publish_cycle(|ys| motion.render_cycle(ys, sr));
        }
        self.circuit.apply_changes(&p);
        self.diffusion.apply_changes(&p);
        self.motor.begin_block(p.inertia.value());
        let algo = p.algo.index();

        // 2. The audio block: every knob rides its DECLARED glide;
        //    the lanes scaffolding owns the channel plumbing; the
        //    active algorithm owns the frame law.
        let mut time_ramp = self.time_control.begin_block(ctx.params, &p.time);
        let mut fb_ramp = self.feedback_control.begin_block(ctx.params, &p.feedback);
        let mut dry_ramp = self.dry_control.begin_block(ctx.params, &p.dry);
        let mut wet_ramp = self.wet_control.begin_block(ctx.params, &p.wet);
        let mut out_ramp = self.output_control.begin_block(ctx.params, &p.output);
        let mut rate_ramp = self.rate_control.begin_block(ctx.params, &p.rate);
        let mut depth_ramp = self.depth_control.begin_block(ctx.params, &p.depth);
        let mut spread_ramp = self.spread_control.begin_block(ctx.params, &p.spread);
        let mut heat_ramp = self.heat_control.begin_block(ctx.params, &p.heat);
        let mut sag_ramp = self.sag_control.begin_block(ctx.params, &p.sag);
        let mut shift_ramp = self.shift_control.begin_block(ctx.params, &p.shift);
        let mut grain_ramp = self.grain_control.begin_block(ctx.params, &p.grain);
        let mut splice_ramp = self.splice_control.begin_block(ctx.params, &p.splice);
        let mut orbit_ramp = self.orbit_control.begin_block(ctx.params, &p.orbit);
        let mut diffuse_ramp = self.diffuse_control.begin_block(ctx.params, &p.diffuse);
        let mut bleed_ramp = self.bleed_control.begin_block(ctx.params, &p.bleed);
        let sr = self.sr;
        let (echo, motion) = (&mut self.echo, &mut self.motion);
        let (circuit, motor, shift) = (&mut self.circuit, &mut self.motor, &mut self.shift);
        let diffusion = &mut self.diffusion;
        let mut wet_peak = 0.0f32;

        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        lanes.run(|fr| {
            // Ramps honor the declared ranges — no read-site
            // clamps to drift out of sync with the table.
            let base = time_ramp.next() * 0.001 * sr;
            let fb = fb_ramp.next();
            let dry_g = dry_ramp.next();
            let wet_g = wet_ramp.next();
            let g = db_to_amp(out_ramp.next());
            let rate = rate_ramp.next();
            let depth = depth_ramp.next();
            let spread = spread_ramp.next();
            let wobble = motion.tick(rate, depth, sr);
            let diffuse = diffuse_ramp.next();
            let bleed = bleed_ramp.next();
            if active >= 2 {
                let (l, r) = (fr.input(0), fr.input(1));
                let cx = FrameCtx {
                    l,
                    r,
                    base,
                    wobble,
                    spread,
                    fb,
                    diffuse,
                    bleed,
                };
                let (wet_l, wet_r) = match algo {
                    0 => {
                        let heat = heat_ramp.next();
                        let wander = circuit.flutter_tick(heat);
                        let (wl, wr) = echo.stereo_frame_shaped(
                            l,
                            r,
                            base + wander,
                            wobble,
                            spread,
                            fb,
                            |lane, x| circuit.head(lane, x, heat),
                            |m, s| diffusion.couple(m, s, diffuse, bleed),
                        );
                        // The crushed repeats' loudness, paid back
                        // outside the loop (see circuit.rs).
                        let loud = circuit.out_gain(heat);
                        (wl * loud, wr * loud)
                    }
                    1 => motor.frame(echo, diffusion, cx, sag_ramp.next()),
                    2 => {
                        let semitones = shift_ramp.next();
                        let grain = grain_ramp.next();
                        let splice = splice_ramp.next();
                        shift.frame(echo, diffusion, cx, semitones, grain, splice)
                    }
                    _ => orbit::frame(echo, diffusion, cx, orbit_ramp.next()),
                };
                wet_peak = wet_peak.max(wet_l.abs()).max(wet_r.abs());
                fr.write(0, blend(l, wet_l, dry_g, wet_g) * g);
                fr.write(1, blend(r, wet_r, dry_g, wet_g) * g);
            } else if active == 1 {
                // Defensive mono fallback (hosts that ignore the
                // declared stereo config): the classic law — the
                // algorithms are image machines and need two ears.
                let dry = fr.input(0);
                let wet = echo.mono_frame(dry, base, wobble, fb);
                wet_peak = wet_peak.max(wet.abs());
                fr.write(0, blend(dry, wet, dry_g, wet_g) * g);
            }
        });

        // 3. What happened, reported out (bus-gated; free unattached).
        self.telemetry.publish_phase(self.motion.phase01());
        self.telemetry.publish_tail(wet_peak);
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    /// The tail is the echo train: time × however many repeats the
    /// current feedback keeps above −60 dB. Honest, from the knobs.
    fn tail(&self) -> Tail {
        let time_s = (self.params.time.value() * 0.001 * self.sr) as u32;
        let fb = self.params.feedback.value();
        if fb <= 0.001 {
            return Tail::Samples(time_s);
        }
        // −60 dB: fb^n < 0.001 → n = ln(0.001)/ln(fb).
        let repeats = (0.001f32.ln() / fb.ln()).ceil().min(400.0) as u32;
        Tail::Samples(time_s.saturating_mul(repeats.max(1)))
    }
}
