use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::zdf::{self, Knobs};
use crate::primitives::solve::NewtonStats;
use crate::units::{Db, Hz};

/// The diode (TB-303) ladder lowpass in Stinchcombe's analysis: four
/// coupled capacitors, every rung a `tanh` of the voltage across it,
/// zero-delay.
///
/// # Models
/// The TB-303's diode ladder with ideal junctions, as Tim Stinchcombe
/// analyses it (*Analysis of the Moog transistor ladder and derivative
/// filters*, 2008): rung `k` passes `tanh(xₖ − xₖ₋₁)` (voltages in units
/// of `2·Vt`), the driven pair `tanh(x_in)`, the top pair `−tanh(x₃)`;
/// each capacitor integrates the difference of the rungs above and below
/// it, the bottom one (half the others) twice as fast. Its small-signal
/// response is Stinchcombe's `2 / (s⁴ + 8s³ + 20s² + 16s + 2)`: unlike
/// the transistor ladder the stages load each other, it self-oscillates
/// at a feedback gain of 17, and the slope is gentler. Output `−x₃ / drive`;
/// `x_in = drive·input − 17·resonance·output`. The circuit it reduces is
/// [`DiodeLadder`](crate::circuit::topologies::diode_ladder::DiodeLadder).
///
/// # Derivation
/// The rung currents from the junction law (left and right arms carry
/// `I/2 ± δ`, so `δ = (I/2)·tanh(Δv / 2Vt)`; the linear structure is
/// Zavalishin's diode ladder, *The Art of VA Filter Design* §5.10),
/// integrated by the trapezoidal rule with zero-delay feedback, prewarped so the
/// self-oscillation pitch (`√2` times the rungs' corner) is exact at any
/// rate; Newton on the four capacitor voltages each sample. f64 state.
///
/// # Verified
/// `crates/stave-test/tests/filters_match_ngspice.rs::stinchcombe_matches_the_diode_ladder`:
/// against ngspice's run of the diode ladder deck, resonance 10 of 17,
/// 40 mV at 300 Hz: at the ideal pitch within 8e-3 normalized RMS
/// (measured 4.1e-3, the driven pair's β), tuned to the circuit's own
/// pitch and onset within 2e-4 (measured 4.9e-5: the structure is the
/// circuit, to the solver's error); and
/// `crates/stave-dsp/src/primitives/filters/ladder/tests.rs::stinchcombe_cutoff_is_exact_across_the_range`:
/// the small-signal response within 0.01 dB and 0.1° of Stinchcombe's
/// transfer function at half, one and one and a half times the pitch,
/// 50 Hz–16 kHz, resonance 0–0.8 (measured 4e-5 dB); its siblings hold
/// self-oscillation (0.5 % at 1.01; 971 Hz at 1.3 for 1 kHz), the drive's
/// harmonics and rate invariance.
///
/// # Cost
/// Datasheet `"Ladder (Stinchcombe)"` at 48 kHz.
///
/// # Aliasing
/// The rungs' `tanh` aliases: run it at 4× —
/// `Oversampled2x::front(Oversampled2x::nested(..))`, 84 samples of
/// latency. A stepped tone (500 Hz–12 kHz, half scale, +12 dB drive,
/// cutoff 8 kHz): audible inharmonic energy 64.4 dB down at 44.1 kHz,
/// 84.9 dB at 48 kHz (budgets 62 and 82,
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`); at 2× only 18.7 and 37.9 dB.
///
/// # Valid ranges
/// Cutoff 1 Hz–0.49·fs (held there), resonance 0 to about 1.5, drive
/// −24 to +36 dB. Solves are capped and counted ([`stats`](Self::stats)).
/// The cutoff may move every sample, at audio rate: swept 50 Hz–15 kHz
/// at 1.7 kHz it stays bounded and converged
/// (`crates/stave-dsp/src/primitives/filters/ladder/tests.rs::ladders_stay_bounded_under_audio_rate_modulation`).
#[derive(Clone, Debug)]
pub struct Stinchcombe {
    knobs: Knobs,
    /// The upper rungs' trapezoidal gain, `ω₀·T/2`; the bottom's is twice it.
    g: f64,
    s: [f64; 4],
    y: [f64; 4],
    stats: NewtonStats,
}

/// The feedback gain at which the linear diode ladder self-oscillates.
const ONSET: f64 = 17.0;

impl Stinchcombe {
    /// A ladder that self-oscillates at `cutoff` once `resonance`
    /// reaches 1, drive 0 dB.
    pub fn new(cutoff: Hz, resonance: f32) -> Self {
        let knobs = Knobs::new(cutoff, resonance);
        Self {
            g: knobs.warped() / core::f64::consts::SQRT_2,
            knobs,
            s: [0.0; 4],
            y: [0.0; 4],
            stats: NewtonStats::default(),
        }
    }

    /// Retune, state kept; cheap enough per sample (one `tan`).
    pub fn set_cutoff(&mut self, cutoff: Hz) {
        self.knobs.cutoff = cutoff;
        self.g = self.knobs.warped() / core::f64::consts::SQRT_2;
    }

    /// 0 is a plain four-pole lowpass, 1 the onset of self-oscillation
    /// (a feedback gain of 17).
    pub fn set_resonance(&mut self, resonance: f32) {
        self.knobs.resonance = resonance;
    }

    /// Gain into the rungs' `tanh` (0 dB: full scale is `2·Vt`); the
    /// output is divided by it.
    pub fn set_drive(&mut self, drive: Db) {
        self.knobs.set_drive(drive);
    }

    /// Newton's work and cap hits since `prepare`.
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// The five rung currents (driven pair, three rungs, top pair) for
    /// capacitor voltages `y`.
    #[inline]
    fn rungs(x: f64, k: f64, y: &[f64; 4]) -> [f64; 5] {
        [
            zdf::tanh(x + k * y[3]),
            zdf::tanh(y[1] - y[0]),
            zdf::tanh(y[2] - y[1]),
            zdf::tanh(y[3] - y[2]),
            -zdf::tanh(y[3]),
        ]
    }

    /// One sample in f64.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let (g, d) = (self.g, self.knobs.drive);
        let k = ONSET * self.knobs.resonance();
        let x = d * input;
        let gain = [2.0 * g, g, g, g];
        let s = self.s;
        let mut y = self.y;
        let (mut iterations, mut converged) = (0, false);
        let mut delta;
        loop {
            delta = Self::rungs(x, k, &y);
            let mut r = [0.0; 4];
            for i in 0..4 {
                r[i] = y[i] - s[i] - gain[i] * (delta[i + 1] - delta[i]);
            }
            if zdf::small(&r) {
                converged = true;
                break;
            }
            if iterations == zdf::MAX_ITERATIONS {
                break;
            }
            iterations += 1;
            let slope = delta.map(|t| 1.0 - t * t);
            let mut j = [[0.0; 4]; 4];
            for i in 0..4 {
                // δᵢ₊₁ falls as yᵢ rises (a rung above), δᵢ rises (below).
                j[i][i] = 1.0 + gain[i] * (slope[i + 1] + if i > 0 { slope[i] } else { 0.0 });
                if i < 3 {
                    j[i][i + 1] = -gain[i] * slope[i + 1];
                }
                if i > 0 {
                    j[i][i - 1] = -gain[i] * slope[i];
                }
            }
            // The driven pair sees the output through the feedback.
            j[0][3] += gain[0] * k * slope[0];
            zdf::solve(&mut j, &mut r);
            zdf::update(&mut y, &r);
        }
        zdf::record(&mut self.stats, iterations, converged);
        for i in 0..4 {
            self.s[i] = y[i] + gain[i] * (delta[i + 1] - delta[i]);
        }
        self.y = y;
        -y[3] / d
    }
}

impl Stage for Stinchcombe {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.knobs.sample_rate = sample_rate;
        self.g = self.knobs.warped() / core::f64::consts::SQRT_2;
        self.reset();
        self.stats = NewtonStats::default();
    }
    fn reset(&mut self) {
        self.s = [0.0; 4];
        self.y = [0.0; 4];
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }
    /// The linearised ladder's poles,
    /// `s⁴ + 8s³ + 20s² + 16s + 2 + 34·resonance = 0`.
    fn tail(&self) -> Tail {
        let k = ONSET * self.knobs.resonance();
        zdf::linear_tail(&[1.0, 8.0, 20.0, 16.0, 2.0 + 2.0 * k], self.g)
    }
}
