//! The smear ENGINE — the spine: the per-block process loop and the
//! control-rate updates that keep the field drifting. The garden
//! itself (the resonator lattice) is `garden.rs`.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, ProcessCtx, Tap};
use stave_dsp::db_to_amp;

mod garden;
use garden::*;

use crate::params::SmearParams;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Declared observation points: the editor's IN/OUT meters
/// read these wires. (They were dead before this — the engine never
/// published; found by the declared-taps sweep.)
pub struct SmearTaps {
    pub io_in: Tap,
    pub io_out: Tap,
}

impl SmearTaps {
    const fn new() -> Self {
        Self {
            io_in: Tap::new("io_in"),
            io_out: Tap::new("io_out"),
        }
    }
}

stave_core::continuous_controls! {
    struct Controls<SmearParams> => Ramps, Values {
        seed, smear, bloom, decay, flux, shimmer, drive, motion, depth, tone, mix, output,
    }
}

pub struct SmearEngine {
    sr: f32,
    debug: Option<Arc<DebugBus>>,
    taps: SmearTaps,
    chans: stave_dsp::PerChannel<ChanState, MAX_CHANS>,
    ctrl_phase: usize,

    controls: Controls,
    params: Arc<SmearParams>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Cubic soft-clip: transparent below ~0.7, asymptotic toward ±1.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let t = (x * 0.7).clamp(-1.5, 1.5);
    (1.5 * t - 0.5 * t * t * t).clamp(-1.0, 1.0)
}

impl stave_core::Processor for SmearEngine {
    type Params = SmearParams;

    fn new(params: Arc<SmearParams>) -> Self {
        Self {
            sr: 48_000.0,
            debug: None,
            taps: SmearTaps::new(),
            chans: stave_dsp::PerChannel::new(|lane| {
                // Lanes 0/1 keep their historical seeds (stereo is
                // bit-pinned through the 5.1 lift); rears/C/LFE get
                // their own gardens.
                ChanState::new(
                    [
                        0x1234_5678,
                        0x9E37_79B9,
                        0x6A09_E667,
                        0xBB67_AE85,
                        0x3C6E_F372,
                        0xA54F_F53A,
                    ][lane],
                )
            }),
            ctrl_phase: 0,
            controls: Controls::new(&params),
            params,
        }
    }

    fn activate(&mut self, env: stave_core::Environment) {
        let sample_rate = env.sample_rate;
        self.sr = sample_rate;
        for c in self.chans.iter_mut() {
            c.reset();
        }
        self.ctrl_phase = 0;
        self.controls.prepare(f64::from(sample_rate), &self.params);
        for c in self.chans.iter_mut() {
            c.update_coeffs(sample_rate, self.params.decay.value(), 0.0, 0.0);
        }
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.debug = Some(bus);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        let mut ramps = self.controls.begin_block(ctx.params, &p);
        let in_chans = ctx.input.channel_count();
        let out_chans = ctx.output.channel_count();
        let chans = in_chans.min(out_chans).min(MAX_CHANS);
        let frames = ctx.frames;
        let sr = self.sr;
        // Makeup gain: only a handful of resonators ring at once and each is
        // normalised to ~unity peak, so the raw sum is quiet. Bring the wet
        // up to roughly dry level; soft-clip catches dense excitation.
        let norm = 14.0 / (N_RES as f32).sqrt();

        let mut peak_in = 0.0f32;
        let mut peak_out = 0.0f32;
        for f in 0..frames {
            let v = ramps.next();
            let out_lin = db_to_amp(v.output);
            // Control-rate update: coefficients + tuning drift.
            if self.ctrl_phase == 0 {
                let depth = v.depth;
                for (ci, c) in self.chans.iter_mut().enumerate() {
                    c.update_coeffs(sr, v.decay, v.motion, v.flux);
                    // DEPTH widens the image by detuning the gardens
                    // apart — a per-lane spread map (L/R the classic
                    // pair, rears wider, C/LFE anchored). The stereo
                    // entries reproduce the historical arithmetic
                    // bit-exactly (1 − x ≡ 1 + (−1)·x).
                    if depth > 0.0 {
                        const SPREAD: [f32; MAX_CHANS] = [-1.0, 1.0, 0.0, 0.0, -1.6, 1.6];
                        let widen = 1.0 + SPREAD[ci] * (depth * 0.004);
                        if SPREAD[ci] != 0.0 {
                            for d in c.detune.iter_mut() {
                                *d *= widen;
                            }
                        }
                    }
                }
            }
            self.ctrl_phase = (self.ctrl_phase + 1) % CTRL_INTERVAL;

            // Coupling/feedback gains, scaled so the network of
            // mutually-exciting resonators stays stable.
            let smear = v.smear * 0.35;
            let bloom = v.bloom * 0.5;
            let shimmer = v.shimmer * 0.4;
            let drive = v.drive;
            let drive_k = 1.0 + drive * 6.0;
            let exc_gain = v.seed * 0.6;

            for c in 0..chans {
                let dry = ctx.input.channel(c)[f];
                peak_in = peak_in.max(dry.abs());
                let g = &mut self.chans[c];

                let mut wet = 0.0f32;
                for i in 0..N_RES {
                    // Sympathetic excitation: dry input + energy blooming in
                    // from neighbouring semitones (smear), an octave below
                    // (shimmer), and the resonator's own regeneration
                    // (bloom). Coupling reads the PREVIOUS sample's outputs.
                    let lo = if i > 0 { g.out_prev[i - 1] } else { 0.0 };
                    let hi = if i + 1 < N_RES {
                        g.out_prev[i + 1]
                    } else {
                        0.0
                    };
                    let oct = if i >= 12 { g.out_prev[i - 12] } else { 0.0 };
                    let mut exc =
                        dry * exc_gain + smear * (lo + hi) + shimmer * oct + bloom * g.out_prev[i];

                    // DRIVE — soft saturation in the excitation path adds
                    // warmth and tames the regenerative feedback.
                    if drive > 0.0 {
                        exc = (exc * drive_k).tanh() / drive_k.sqrt();
                    }

                    let y = g.a1[i] * g.y1[i] - g.a2[i] * g.y2[i] + g.b0[i] * exc;
                    let y = y + DENORM - DENORM; // denormal guard
                    g.y2[i] = g.y1[i];
                    g.y1[i] = y;
                    g.out_cur[i] = y;
                    wet += y;
                }
                core::mem::swap(&mut g.out_prev, &mut g.out_cur);

                let wet = soft_clip(wet * norm);

                // TONE: one-pole HF damping.
                let lp = 1.0 - v.tone * 0.9;
                g.tone_state = g.tone_state * (1.0 - lp) + wet * lp;
                let wet = g.tone_state;

                let out = dry + (wet * out_lin - dry) * v.mix;
                peak_out = peak_out.max(out.abs());
                ctx.output.channel(c)[f] = out;
            }
        }
        if let Some(bus) = &self.debug {
            let db = |p: f32| 20.0 * (p.max(1e-6) as f64).log10();
            self.taps.io_in.set(bus, db(peak_in));
            self.taps.io_out.set(bus, db(peak_out));
        }

        for c in chans..out_chans {
            for f in 0..frames {
                ctx.output.channel(c)[f] = 0.0;
            }
        }
    }
}
