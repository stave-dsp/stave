use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::zdf::{self, Knobs};
use crate::primitives::solve::NewtonStats;
use crate::units::{Db, Hz};

/// The transistor (Moog) ladder lowpass in Huovilainen's model: four
/// `tanh` stages with resonance fed back around them, zero-delay.
///
/// # Models
/// The Moog transistor ladder with ideal transistors (matched, infinite
/// β, no Early effect or base resistance), as Antti Huovilainen derives
/// it (*Non-linear digital implementation of the Moog ladder filter*,
/// DAFx-04): with every voltage in units of `2·Vt`,
/// `x′ₖ = ωc·(tanh xₖ₋₁ − tanh xₖ)`, `x₀ = drive·input − 4·resonance·x₄`,
/// output `x₄ / drive`. The circuit it reduces is
/// [`TransistorLadder`](crate::circuit::topologies::transistor_ladder::TransistorLadder).
///
/// # Derivation
/// Huovilainen's ODE — the four-stage case of D'Angelo and Välimäki's
/// generalized ladder (*Generalized Moog Ladder Filter*, IEEE/ACM TASLP
/// 22(12), 2014) — integrated by the trapezoidal rule in Zavalishin's
/// topology-preserving form (*The Art of VA Filter Design*), prewarped so
/// the small-signal cutoff — the self-oscillation pitch — is exact at
/// any rate; each sample's implicit step is solved by Newton on the four
/// stage outputs (not Huovilainen's explicit half-sample scheme), so
/// nothing in the loop is delayed. f64 state.
///
/// # Verified
/// `crates/stave-test/tests/filters_match_ngspice.rs::huovilainen_matches_the_transistor_ladder`:
/// against the transistor ladder netlist (ten 2N3904s) through
/// ngspice's run of the deck, resonance 3 of 4, 40 mV (1.5 Vt) at
/// 300 Hz: at the ideal cutoff within 4e-2 normalized RMS (measured
/// 3.0e-2 — what ideal transistors cost: the circuit sings 3 % lower),
/// tuned to the circuit's own pitch and onset within 1e-2 (measured
/// 6.1e-3); and
/// `crates/stave-dsp/src/primitives/filters/ladder/tests.rs::huovilainen_cutoff_is_exact_across_the_range`:
/// the small-signal response within 0.01 dB and 0.1° of the prewarped
/// `1/((1 + s)⁴ + 4·resonance)` at half, one and one and a half times
/// the cutoff, 50 Hz–16 kHz, resonance 0–0.8 (measured 5e-5 dB). Its
/// siblings: self-oscillation (below 1e-6 left at 0.99, singing at 1.01
/// within 0.5 % of the cutoff, 956 Hz at 1.3 for 1 kHz), the third
/// harmonic rising 2 dB per dB with even ones absent, the same response,
/// harmonics and pitch at 44.1–192 kHz.
///
/// # Cost
/// Datasheet `"Ladder (Huovilainen)"` at 48 kHz, about two Newton
/// iterations a sample (`stats()` counts them).
///
/// # Aliasing
/// The stages' `tanh` aliases: run it at 4× —
/// `Oversampled2x::front(Oversampled2x::nested(..))`, 84 samples of
/// latency at the host rate. A stepped tone (500 Hz–12 kHz, half scale,
/// +12 dB drive, cutoff 8 kHz) leaves its audible inharmonic energy
/// 68.9 dB down at 44.1 kHz and 87.5 dB at 48 kHz (budgets 66 and 85,
/// `crates/stave-dsp/tests/aliasing_budgets.rs::nonlinear_stages_hold_their_alias_budgets`); at 2× only 21.3 and 39.3 dB: the driven pair folds a tone
/// above the cutoff below it.
///
/// # Valid ranges
/// Cutoff 1 Hz–0.49·fs (held there), resonance 0 to about 1.5 (1 is
/// the onset; past it the pitch flattens as the stages saturate), drive
/// −24 to +36 dB. Solves are capped and counted ([`stats`](Self::stats)).
/// The cutoff may move every sample, at audio rate: swept 50 Hz–15 kHz
/// at 1.7 kHz it stays bounded and converged
/// (`crates/stave-dsp/src/primitives/filters/ladder/tests.rs::ladders_stay_bounded_under_audio_rate_modulation`).
#[derive(Clone, Debug)]
pub struct Huovilainen {
    knobs: Knobs,
    g: f64,
    s: [f64; 4],
    y: [f64; 4],
    stats: NewtonStats,
}

impl Huovilainen {
    /// A ladder with its cutoff (the self-oscillation pitch) at `cutoff`
    /// and `resonance` (1 = the onset of self-oscillation), drive 0 dB.
    pub fn new(cutoff: Hz, resonance: f32) -> Self {
        let knobs = Knobs::new(cutoff, resonance);
        Self {
            g: knobs.warped(),
            knobs,
            s: [0.0; 4],
            y: [0.0; 4],
            stats: NewtonStats::default(),
        }
    }

    /// Retune, state kept; cheap enough per sample (one `tan`).
    pub fn set_cutoff(&mut self, cutoff: Hz) {
        self.knobs.cutoff = cutoff;
        self.g = self.knobs.warped();
    }

    /// 0 is a plain four-pole lowpass, 1 the onset of self-oscillation
    /// (a feedback gain of 4).
    pub fn set_resonance(&mut self, resonance: f32) {
        self.knobs.resonance = resonance;
    }

    /// Gain into the stages' `tanh` (0 dB: full scale is `2·Vt`, where
    /// `tanh` bends); the output is divided by it.
    pub fn set_drive(&mut self, drive: Db) {
        self.knobs.set_drive(drive);
    }

    /// Newton's work and cap hits since `prepare`.
    pub fn stats(&self) -> NewtonStats {
        self.stats
    }

    /// One sample in f64.
    pub fn tick_f64(&mut self, input: f64) -> f64 {
        let (g, d) = (self.g, self.knobs.drive);
        let k = 4.0 * self.knobs.resonance();
        let x = d * input;
        let s = self.s;
        let mut y = self.y;
        let mut t = [0.0; 5];
        let (mut iterations, mut converged) = (0, false);
        loop {
            t[0] = zdf::tanh(x - k * y[3]);
            for i in 0..4 {
                t[i + 1] = zdf::tanh(y[i]);
            }
            let mut r = [0.0; 4];
            for i in 0..4 {
                r[i] = y[i] - s[i] - g * (t[i] - t[i + 1]);
            }
            if zdf::small(&r) {
                converged = true;
                break;
            }
            if iterations == zdf::MAX_ITERATIONS {
                break;
            }
            iterations += 1;
            let slope = t.map(|t| 1.0 - t * t);
            let mut j = [[0.0; 4]; 4];
            for i in 0..4 {
                j[i][i] = 1.0 + g * slope[i + 1];
                if i > 0 {
                    j[i][i - 1] = -g * slope[i];
                }
            }
            j[0][3] += g * k * slope[0];
            zdf::solve(&mut j, &mut r);
            zdf::update(&mut y, &r);
        }
        zdf::record(&mut self.stats, iterations, converged);
        for i in 0..4 {
            // The trapezoidal state: y + g·f, i.e. 2y − s.
            self.s[i] = y[i] + g * (t[i] - t[i + 1]);
        }
        self.y = y;
        y[3] / d
    }
}

impl Stage for Huovilainen {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.knobs.sample_rate = sample_rate;
        self.g = self.knobs.warped();
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
    /// The linearised ladder's poles, `(s + 1)⁴ + 4·resonance = 0`;
    /// infinite from the onset on.
    fn tail(&self) -> Tail {
        let k = 4.0 * self.knobs.resonance();
        zdf::linear_tail(&[1.0, 4.0, 6.0, 4.0, 1.0 + k], self.g)
    }
}
