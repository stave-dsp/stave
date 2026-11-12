//! Forge's voicing vocabulary — the CHARACTER recipes and the named
//! filter placements (VOICE / POST), separated from the engine in
//! lib.rs: this file is where the sound lives, lib.rs is where the
//! signal flows. Everything here is pure data → shape/spec; no state.

use stave_dsp::{shapes, FilterSpec};

// ---------------------------------------------------------------------------

/// One parametric shape covers all six characters:
/// `f(x) = wa·cheb₇(w)(x) + wk·knee(k)(pg·x)`. Chebyshev weights are
/// linear in the output, so EDGE morphs are exact pointwise blends.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct CharShape {
    w: [f64; 7],
    wa: f64,
    wk: f64,
    k: f64,
    pg: f64,
    /// Saturation speed of the Chebyshev family's tanh bound. All
    /// characters share that bound; without different approach speeds
    /// they converge to the same clipper at high drive (measured: every
    /// h3/h1 within 0.05 of each other at drive 5). cs < 1 keeps the
    /// recipe region wide (polite characters), cs > 1 hits the wall
    /// early (aggressive ones). Slope-compensated.
    cs: f64,
}

/// Harmonic-ladder magnitudes → signed Chebyshev weights. For a sine
/// input, Tₖ(sin θ) = ±sin kθ / ±cos kθ with alternating sign on the odd
/// orders (−, +, − for k = 3, 5, 7). Applying the signs here makes the
/// declared ladder the SMALL-SIGNAL HARMONIC RATIOS, with no fundamental
/// cancellation between orders (raw weights measured backwards: heavy-w₃
/// recipes came out the mildest).
fn signed(w: [f64; 7]) -> [f64; 7] {
    [w[0], w[1], -w[2], w[3], w[4], w[5], -w[6]]
}

pub(crate) fn character_shape(ch: usize, e: f64) -> CharShape {
    match ch {
        // SILK — odd-only, steep falloff: H3 ≫ H5 ≫ H7, no evens. The
        // polite bus sheen; EDGE feeds the higher orders in.
        0 => CharShape {
            w: signed([
                1.0,
                0.0,
                0.05 + 0.11 * e,
                0.0,
                0.010 + 0.045 * e,
                0.0,
                0.018 * e,
            ]),
            wa: 1.0,
            wk: 0.0,
            k: 1.0,
            pg: 1.0,
            cs: 0.6,
        },
        // TRIODE — even-dominant with fast decay: strong H2, modest H3.
        1 => CharShape {
            w: signed([
                1.0,
                0.34 + 0.15 * e,
                0.05 + 0.04 * e,
                0.012 + 0.028 * e,
                0.006 * e,
                0.002 * e,
                0.0,
            ]),
            wa: 1.0,
            wk: 0.0,
            k: 1.0,
            pg: 1.0,
            cs: 0.85,
        },
        // CLASS AB — odd, SLOW falloff (the crossover signature: energy
        // that refuses to die out along the ladder).
        2 => CharShape {
            w: signed([
                1.0,
                0.0,
                0.20 + 0.08 * e,
                0.0,
                0.09 + 0.07 * e,
                0.0,
                0.04 + 0.05 * e,
            ]),
            wa: 1.0,
            wk: 0.0,
            k: 1.0,
            pg: 1.0,
            cs: 1.1,
        },
        // TAPE — odd fast-falloff blended with a compressive knee.
        3 => CharShape {
            w: signed([1.0, 0.0, 0.09 + 0.05 * e, 0.0, 0.02 + 0.02 * e, 0.0, 0.0]),
            wa: 0.6,
            wk: 0.4,
            k: 1.1 + 0.7 * e,
            pg: 1.0,
            cs: 0.75,
        },
        // DIODE — the hard clip family; EDGE is knee hardness.
        4 => CharShape {
            w: [0.0; 7],
            wa: 0.0,
            wk: 1.0,
            k: 2.0 + 14.0 * e * e,
            pg: 1.0,
            cs: 1.0,
        },
        // FUZZ — hot knee + injected H2 bite; EDGE adds pre-gain.
        _ => CharShape {
            w: signed([0.0, 0.28 * e, 0.0, 0.0, 0.0, 0.0, 0.0]),
            wa: 1.0,
            wk: 1.0,
            k: 4.0 + 18.0 * e,
            pg: 1.0 + 1.5 * e,
            cs: 1.3,
        },
    }
}

impl CharShape {
    /// Slope-normalised shape: f′(0) = 1 for every character, so recipes
    /// with heavy high-order weights (whose Tₖ carry a −k·u linear part
    /// that eats the fundamental) drive the soft bound as hard as the
    /// low-order ones. Without this, CLASS AB measured as the MILDEST
    /// character — the opposite of its recipe.
    pub(crate) fn eval(self) -> impl Fn(f64) -> f64 {
        // d/dx Σ wₖ Tₖ(tanh cs·x) at 0: odd Tₖ′(0) alternates ±k, evens 0.
        let cheb_slope =
            self.cs * (self.w[0] - 3.0 * self.w[2] + 5.0 * self.w[4] - 7.0 * self.w[6]);
        let slope = (self.wa * cheb_slope + self.wk * self.pg).max(0.25);
        let cheb = shapes::chebyshev(self.w);
        let knee = shapes::knee(self.k);
        move |x| (self.wa * cheb(self.cs * x) + self.wk * knee(self.pg * x)) / slope
    }

    /// The push-pull composition of this character: antisymmetric pair
    /// around standing bias `b0` with half-imbalance `m` ∈ [0, 1).
    /// Evens cancel BY CONSTRUCTION at m = 0; BIAS becomes the classic
    /// push-pull mismatch (even leakage), EDGE cools the standing bias
    /// toward crossover.
    pub(crate) fn eval_push_pull(self, b0: f64, m: f64) -> impl Fn(f64) -> f64 {
        let f = self.eval();
        move |x| 0.5 * ((1.0 + m) * f(b0 + x) - (1.0 - m) * f(b0 - x))
    }
}

/// Second-clipper hardness per character (CASCADE / PUSH-PULL).
pub(crate) fn stage_b_knee(ch: usize, e: f64) -> f64 {
    let hard = [0.4, 0.6, 0.8, 0.7, 1.6, 2.2][ch.min(5)];
    1.2 + hard * (1.3 + 2.5 * e)
}

/// Grid-conduction sense on the primary curve: excess above the
/// conduction threshold in shaper-input units (drives BLOCKING).
/// ONE-SIDED — grid current flows on positive excursions only; a
/// two-sided |x| sense turns the bias choke into positive feedback
/// (the choke enlarges negative excursions, which re-feed the sense —
/// measured −51 dB runaway).
pub(crate) fn conduction_sense(x: f64) -> f64 {
    (x - 2.5).max(0.0)
}

/// The INTERSTAGE voicing filter for a VOICE profile, with the TONE trim
/// folded in. It sits BETWEEN the stages, so the second nonlinearity
/// eats the profile — where the G(f, level) bend comes from.
pub(crate) fn voice_spec(v: usize, tone: f32) -> FilterSpec {
    let oct = |hz: f32, spread: f32| hz * 2f32.powf(tone * spread);
    match v {
        // FLAT — the classic interstage tilt (TONE is the whole filter).
        0 => FilterSpec::HighShelf {
            hz: 900.0,
            db: tone * 12.0,
        },
        // SCOOP — mid dip between the stages; TONE eases/deepens it.
        1 => FilterSpec::Peak {
            hz: 750.0,
            db: -7.0 + tone * 3.0,
            q: 1.1,
        },
        // BUMP — pushed mids into the second clipper.
        2 => FilterSpec::Peak {
            hz: 950.0,
            db: 5.0 + tone * 3.0,
            q: 1.2,
        },
        // BRIGHT — presence shelf into the second stage.
        3 => FilterSpec::HighShelf {
            hz: 2_200.0,
            db: 6.0 + tone * 4.0,
        },
        // DARK — lowpass into the second stage.
        4 => FilterSpec::Lowpass {
            hz: oct(5_000.0, 1.0),
            q: 0.8,
        },
        // HONK — cocked-wah bell.
        _ => FilterSpec::Peak {
            hz: 1_400.0,
            db: 8.0 + tone * 3.0,
            q: 2.2,
        },
    }
}

/// The OUTPUT filter — the explicit POST FILTER selector. These are
/// plain EQ moves, named as such (this is a saturator, not an amp sim).
pub(crate) fn post_spec(c: usize) -> FilterSpec {
    match c {
        // FLAT — out of the way.
        0 => FilterSpec::Peak {
            hz: 1_000.0,
            db: 0.0,
            q: 1.0,
        },
        // AIR — +2.5 dB high shelf at 2.5 kHz.
        1 => FilterSpec::HighShelf {
            hz: 2_500.0,
            db: 2.5,
        },
        // LOW CUT — 90 Hz highpass (clears saturation mud).
        2 => FilterSpec::Highpass { hz: 90.0, q: 0.8 },
        // WARM — +3 dB low shelf at 180 Hz.
        3 => FilterSpec::LowShelf { hz: 180.0, db: 3.0 },
        // HI CUT — 5.5 kHz lowpass with a resonant edge.
        4 => FilterSpec::Lowpass {
            hz: 5_500.0,
            q: 1.1,
        },
        // DARK — deeper 4 kHz roll-off.
        _ => FilterSpec::Lowpass {
            hz: 4_000.0,
            q: 0.8,
        },
    }
}
