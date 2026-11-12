//! CIRCUIT — a physics stage AT THE WRITE HEAD. Everything that
//! passes the head — the program on its first lap AND the fed-back
//! tail on every lap after — goes through the stage and the voice's
//! loop filter, so the character COMPOUNDS down the echo train the
//! way generation loss does on a real machine.
//!
//! LOUD destruction, DIALED decay — HEAT works the way a real
//! machine runs hot: it does not amplify the loop, it LOWERS THE
//! CEILING toward the program (`s·cell(x/s)` — unity small-signal
//! gain, compressive above, ceiling ∝ s), and the loudness the
//! crushed repeats lose comes back OUTSIDE the loop, from a
//! heat-riding output gain the feedback never sees. So: destruction
//! depth = how far the program sits above the ceiling; decay = the
//! dialed feedback, exactly, once the tail falls under the knee.
//!
//! (Two rejected makeups, both measured: constant `cell(x·d)/d`
//! crushed driven repeats to threshold/drive ≈ −20 dB per lap —
//! quiet AND four-repeat short; an envelope-ratio AGC restored
//! level per lap, which IS per-lap gain 1 — TUBE self-oscillated at
//! every feedback setting and the pinned screamer times its clamp
//! ran off exponentially. Level restoration must live outside the
//! recursion.)
//!
//! Two rules cage the loop at any HEAT:
//! * every voice is small-signal unity and compressive above —
//!   out never exceeds in;
//! * the loop filters only CUT — any in-loop gain above unity at any
//!   frequency would outrun the dialed feedback.
//!
//! HEAT = 0 bypasses everything, which keeps the classic law
//! bit-identical (a law test).

use stave_dsp::FilterSpec;
use stave_dsp::RcClipperConfig;
use stave_dsp::Xorshift32;
use stave_dsp::{shapes, Biquad, NonlinearRc, PerChannel, Slew, Stage};

use crate::params::EddyParams;

/// The ceiling scale's ends: HEAT sweeps `s` log-linearly from
/// CLEAN (ceiling well above any program — barely a kiss) down to
/// CRUSH (the program sits ~14 dB over the knee — destroyed).
const S_CLEAN: f32 = 2.4;
const S_CRUSH: f32 = 0.15;

/// The scale at a given heat, and the output gain that pays the
/// crushed repeats' loudness back OUTSIDE the loop.
#[inline]
fn scale(heat: f32) -> f32 {
    S_CLEAN * (S_CRUSH / S_CLEAN).powf(heat)
}

/// TAPE flutter depth at full HEAT, in milliseconds of head wander.
const FLUTTER_MS: f32 = 0.35;

/// One lane's voice chain: the stateful stages side by side (their
/// state is a few floats; parking beats swap machinery at this
/// scale) and the voice's loop filters. TUBE is memoryless — an
/// asymmetric conduction shape — so it has no stage.
struct LaneStages {
    /// SCREAMER: diode-pair clipper whose threshold rides on the
    /// cap's charge history — earlier repeats bias how later ones
    /// clip.
    screamer: NonlinearRc,
    /// TAPE: path-dependent magnetization…
    tape_mag: stave_dsp::Hysteresis,
    /// …into a slew limit (the head can only move so fast), which is
    /// what dulls and squashes each generation like a real dub loop.
    tape_slew: Slew,
    /// The voice's generation-loss filters (cut-only — see the cage
    /// rule above): what each pass through the machine takes away.
    lo_cut: Biquad,
    hi_cut: Biquad,
}

impl LaneStages {
    fn new() -> Self {
        Self {
            screamer: NonlinearRc::diode_clipper(&RcClipperConfig {
                tau_ms: 4.0,
                threshold: 0.5,
                hardness: 6.0,
            }),
            tape_mag: stave_dsp::Hysteresis::new(2.5, 0.6),
            tape_slew: Slew::new(400.0, 400.0),
            lo_cut: Biquad::new(),
            hi_cut: Biquad::new(),
        }
    }
}

pub(super) struct Circuit {
    stages: PerChannel<LaneStages, 2>,
    /// The applied voice index; a change resets stage state (stale
    /// charge from another voice must not thump into the loop).
    kind: usize,
    /// TAPE flutter: one capstan, one wander — a smoothed random
    /// walk shared by both lanes (the machine has one motor).
    flutter: f32,
    flutter_rng: Xorshift32,
    sr: f32,
}

impl Circuit {
    pub(super) fn new() -> Self {
        Self {
            stages: PerChannel::new(|_| LaneStages::new()),
            kind: 0,
            flutter: 0.0,
            flutter_rng: Xorshift32::new(0x2F6E_2B1D),
            sr: 48_000.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.flutter = 0.0;
        for lane in self.stages.iter_mut() {
            lane.screamer.prepare(sr);
            lane.tape_mag.prepare(sr);
            lane.tape_slew.prepare(sr);
            lane.lo_cut.reset();
            lane.hi_cut.reset();
        }
    }

    /// Param changes become structure, block-rate: voice switches
    /// reset the stages; HEAT re-times the tape slew and morphs the
    /// voice's loop filters from open (heat → 0) toward the machine
    /// at its darkest.
    pub(super) fn apply_changes(&mut self, p: &EddyParams) {
        let kind = p.circuit.index();
        if kind != self.kind {
            self.kind = kind;
            for lane in self.stages.iter_mut() {
                lane.screamer.reset();
                lane.tape_mag.reset();
                lane.tape_slew.reset();
                lane.lo_cut.reset();
                lane.hi_cut.reset();
            }
        }
        let heat = p.heat.value();
        let slew_rate = 400.0 - 360.0 * heat;
        for lane in self.stages.iter_mut() {
            lane.tape_slew.set_rates(slew_rate, slew_rate, self.sr);
            match kind {
                // TUBE: warm rounding — the top folds away as the
                // stage runs hotter.
                0 => lane.hi_cut.set(
                    FilterSpec::HighShelf {
                        hz: 2_800.0,
                        db: -8.0 * heat,
                    },
                    self.sr,
                ),
                // SCREAMER: the honk — cuts close in from BOTH sides
                // with heat until only the mids fight through.
                1 => {
                    lane.lo_cut.set(
                        FilterSpec::Highpass {
                            hz: 120.0 + 400.0 * heat,
                            q: 0.707,
                        },
                        self.sr,
                    );
                    lane.hi_cut.set(
                        FilterSpec::Lowpass {
                            hz: 7_000.0 - 4_200.0 * heat,
                            q: 0.707,
                        },
                        self.sr,
                    );
                }
                // TAPE: generation loss proper — each lap loses top
                // end and the deep rumble the transport can't hold.
                _ => {
                    lane.lo_cut
                        .set(FilterSpec::Highpass { hz: 55.0, q: 0.707 }, self.sr);
                    lane.hi_cut.set(
                        FilterSpec::Lowpass {
                            hz: 8_500.0 - 5_500.0 * heat,
                            q: 0.707,
                        },
                        self.sr,
                    );
                }
            }
        }
    }

    /// The write-head port — shapes one lane's write (program + fed-
    /// back tail): the ceiling drops toward the program as HEAT
    /// rises (`s·voice(x/s)` — unity small-signal, compressive
    /// above), then the voice's generation-loss filters. `heat <= 0`
    /// is a true bypass: the sample returns untouched.
    #[inline]
    pub(super) fn head(&mut self, lane: usize, x: f32, heat: f32) -> f32 {
        if heat <= 0.0 {
            return x;
        }
        let s = scale(heat);
        let u = x / s;
        let stages = &mut self.stages[lane];
        let shaped = s * match self.kind {
            // TUBE: asymmetric conduction — positive half clips a
            // knee earlier than negative (grid current), so repeats
            // grow even harmonics as they age.
            0 => shapes::asym(0.55, 1.15, 1.6)(u as f64) as f32,
            // SCREAMER: charge-history clipper.
            1 => stages.screamer.tick(u),
            // TAPE: magnetization loop into the slew limit; the
            // hysteresis stage carries its own small-signal makeup.
            _ => stages.tape_slew.tick(stages.tape_mag.tick(u)),
        };
        match self.kind {
            0 => stages.hi_cut.process(shaped),
            _ => stages.hi_cut.process(stages.lo_cut.process(shaped)),
        }
    }

    /// The loudness the crushed repeats lost, paid back OUTSIDE the
    /// loop (the feedback never sees this gain — the cage survives).
    /// 1.0 exactly at heat 0 (`wet × 1.0` keeps the bit-classic law).
    #[inline]
    pub(super) fn out_gain(&self, heat: f32) -> f32 {
        if heat <= 0.0 {
            return 1.0;
        }
        (1.0 / scale(heat)).clamp(1.0, 3.5)
    }

    /// TAPE's capstan wander, in SAMPLES of read-head displacement —
    /// ticked once per frame, shared by both lanes. Zero for the
    /// other voices and at heat 0 (the bit-classic bypass holds:
    /// `base + 0.0` is exact).
    #[inline]
    pub(super) fn flutter_tick(&mut self, heat: f32) -> f32 {
        if self.kind != 2 || heat <= 0.0 {
            return 0.0;
        }
        // xorshift white noise → one-pole at ~6 Hz = the wander.
        let white = self.flutter_rng.next_bipolar();
        let k = 1.0 - (-2.0 * core::f32::consts::PI * 6.0 / self.sr).exp();
        self.flutter += (white - self.flutter) * k;
        self.flutter * heat * FLUTTER_MS * 0.001 * self.sr
    }
}
