//! The aeolian ENGINE — a sympathetic string harp. The engine is its
//! COMPONENTS; `process` below is the whole per-block dance:
//! * `detect.rs` — `Detector`: twelve pitch-class bands → each
//!   string's excitation, gated by WIND (velocity) and dominance.
//! * `strings.rs` — `Strings`: twelve doubled Karplus–Strong voices,
//!   SUSTAIN = decay, TONE = damping, VIBRATO = per-voice drift.
//! * `ensemble.rs` — `Ensemble`: CHORUS width.
//! * `telemetry.rs` — `Telemetry`: each string's ring level and the
//!   harp's clock → the face's vibrating strings.
//!
//! Stereo: the twelve voices are spread around the circle of fifths,
//! so a chord fans out across the image.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{DebugBus, Environment, ProcessCtx, Tail};
use stave_dsp::Svf;

use crate::params::AeolianParams;

mod detect;
mod ensemble;
mod strings;
mod telemetry;

use self::detect::{Detector, CLASSES};
use self::ensemble::Ensemble;
use self::strings::{Strings, Voicing};
use self::telemetry::Telemetry;

/// The REGISTER selector's first position (C2) puts the strings in octave 2.
const LOW_OCTAVE: i32 = 2;
/// Coefficients follow the gliding knobs every this many frames.
const CONTROL_FRAMES: usize = 32;
/// The harp's level into the mix.
const WET_GAIN: f32 = 0.45;
/// The wet's top end: TONE sweeps this low-pass.
const TONE_LO_HZ: f32 = 500.0;
const TONE_HI_HZ: f32 = 14_000.0;

#[inline]
fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Each voice's (left, right) gains: spread on the circle of fifths.
fn pans() -> [[f32; 2]; CLASSES] {
    core::array::from_fn(|c| {
        let pos = ((c * 7) % CLASSES) as f32 / (CLASSES - 1) as f32; // 0..1 around the fifths
        let p = (0.5 + 0.7 * (pos - 0.5)) * core::f32::consts::FRAC_PI_2;
        [p.cos(), p.sin()]
    })
}

struct Controls {
    mix: ContinuousControl,
    tone: ContinuousControl,
    drift: ContinuousControl,
    wind: ContinuousControl,
    sustain: ContinuousControl,
    output: ContinuousControl,
}

impl Controls {
    fn new(p: &AeolianParams) -> Self {
        Self {
            mix: ContinuousControl::new(&p.mix),
            tone: ContinuousControl::new(&p.tone),
            drift: ContinuousControl::new(&p.drift),
            wind: ContinuousControl::new(&p.wind),
            sustain: ContinuousControl::new(&p.sustain),
            output: ContinuousControl::new(&p.output),
        }
    }

    fn prepare(&mut self, sr: f64, p: &AeolianParams) {
        self.mix.prepare(sr, &p.mix);
        self.tone.prepare(sr, &p.tone);
        self.drift.prepare(sr, &p.drift);
        self.wind.prepare(sr, &p.wind);
        self.sustain.prepare(sr, &p.sustain);
        self.output.prepare(sr, &p.output);
    }
}

pub struct AeolianEngine {
    params: Arc<AeolianParams>,
    controls: Controls,
    sr: f32,
    detector: Detector,
    strings: Strings,
    ensemble: Ensemble,
    tone: Svf,
    pans: [[f32; 2]; CLASSES],
    telemetry: Telemetry,
    /// Seconds of audio played (wrapped) — the strings' animation clock.
    clock: f64,
}

impl stave_core::Processor for AeolianEngine {
    type Params = AeolianParams;

    fn new(params: Arc<AeolianParams>) -> Self {
        Self {
            controls: Controls::new(&params),
            params,
            sr: 48_000.0,
            detector: Detector::new(),
            strings: Strings::new(),
            ensemble: Ensemble::new(),
            tone: Svf::new(),
            pans: pans(),
            telemetry: Telemetry::new(),
            clock: 0.0,
        }
    }

    fn activate(&mut self, env: Environment) {
        self.sr = env.sample_rate;
        self.controls.prepare(f64::from(env.sample_rate), &self.params);
        self.detector.prepare(env.sample_rate);
        self.strings.prepare(env.sample_rate);
        self.ensemble.prepare(env.sample_rate);
        self.ensemble.reset_to(self.params.air.value());
        self.tone.prepare(env.sample_rate);
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.telemetry.attach(bus);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Bind: the register and the switches.
        let octave = LOW_OCTAVE + p.register.index() as i32;
        let (chorus, vibrato) = (p.air.value(), p.sway.value());
        let nyq = 0.45 * self.sr;

        // 2. The audio block.
        let c = &mut self.controls;
        let mut mix_ramp = c.mix.begin_block(ctx.params, &p.mix);
        let mut tone_ramp = c.tone.begin_block(ctx.params, &p.tone);
        let mut mod_ramp = c.drift.begin_block(ctx.params, &p.drift);
        let mut sens_ramp = c.wind.begin_block(ctx.params, &p.wind);
        let mut fb_ramp = c.sustain.begin_block(ctx.params, &p.sustain);
        let mut out_ramp = c.output.begin_block(ctx.params, &p.output);
        let (detector, strings, ensemble, tone, pans) =
            (&mut self.detector, &mut self.strings, &mut self.ensemble, &mut self.tone, &self.pans);
        let mut frame = 0usize;

        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        lanes.run(|fr| {
            let (t, m, s, fb) = (tone_ramp.next(), mod_ramp.next(), sens_ramp.next(), fb_ramp.next());
            if frame.is_multiple_of(CONTROL_FRAMES) {
                strings.voice(Voicing {
                    octave,
                    sustain: fb,
                    tone: t,
                    wind: s,
                    vibrato: if vibrato { m } else { 0.0 },
                });
                tone.set((TONE_LO_HZ * (TONE_HI_HZ / TONE_LO_HZ).powf(t)).min(nyq), 0.1);
            }
            frame += 1;

            let dry = if active >= 2 { [fr.input(0), fr.input(1)] } else { [fr.input(0); 2] };
            let mut exc = [0.0f32; CLASSES];
            detector.tick(0.5 * (dry[0] + dry[1]), s, &mut exc);
            let mut ring = [0.0f32; CLASSES];
            strings.tick(&exc, &mut ring);
            let mut harp = [0.0f32; 2];
            for (v, g) in ring.iter().zip(pans.iter()) {
                harp[0] += v * g[0];
                harp[1] += v * g[1];
            }
            let w = ensemble.tick([harp[0] * WET_GAIN, harp[1] * WET_GAIN], chorus, m);
            let w = [tone.process_lp(w[0], 0), tone.process_lp(w[1], 1)];

            let a = mix_ramp.next() * core::f32::consts::FRAC_PI_2;
            // MIX 0 is bit-exact: cos(0) = 1 exactly, sin(0) = 0 exactly.
            let (dg, wg) = (a.cos(), a.sin());
            let g = db_to_amp(out_ramp.next());
            if active >= 2 {
                fr.write(0, (dry[0] * dg + w[0] * wg) * g);
                fr.write(1, (dry[1] * dg + w[1] * wg) * g);
            } else if active == 1 {
                fr.write(0, (dry[0] * dg + 0.5 * (w[0] + w[1]) * wg) * g);
            }
        });

        // 3. What happened, reported out.
        self.clock = (self.clock + ctx.frames as f64 / f64::from(self.sr)) % 3600.0;
        self.telemetry.publish(self.strings.levels(), self.clock);
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    /// The loops ring for up to their T60; at the top they sustain.
    fn tail(&self) -> Tail {
        let p = &self.params;
        if p.sustain.value() > 0.97 {
            Tail::Infinite
        } else {
            Tail::Samples((40.0 * self.sr) as u32)
        }
    }
}

