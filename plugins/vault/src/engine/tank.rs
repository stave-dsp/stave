//! THE TANK — an 8-line feedback delay network, the Loop form at
//! reverb scale. Eight mutually-detuned waveguide lines share one
//! energy-preserving Hadamard junction; every per-trip loss is
//! DECLARED (the T60 gain is computed from the dialed decay and
//! the line's own length — the t60 law measures the promise).
//!
//! Lessons carried in from the dynamo wires, paid for by
//! measurement there:
//! * every damped feedback loop gets a DC BLOCKER (a lowpass-damped
//!   loop is resonant at 0 Hz);
//! * loop saturation must be PASSIVE — |sat(x)| ≤ |x| with unity
//!   small-signal gain, so drive can compress and darken the tail
//!   but can never regenerate it;
//! * read heads GLIDE (slew-capped): a size jump bends pitch like
//!   tape instead of clicking, and the chorus rides the same
//!   mechanism (block-rate targets, per-sample linear steps).

use stave_dsp::{t60_gain, DelayLine};

/// SIXTEEN lines — the published floor for high-quality FDNs, and
/// the cure for the tail's slow envelope beating (finite mode
/// density: modal density of an FDN ~ its total delay; doubling
/// the lines doubles it — the plate's breathing was this number).
pub(super) const LINES: usize = 16;

/// Line lengths at size 1.0, in ms — irrationally spaced so no two
/// lines fall into an integer ratio (mode-density by construction).
const BASE_MS: [f32; LINES] = [
    19.7, 22.9, 25.3, 28.1, 31.9, 35.3, 39.7, 43.1, 46.3, 50.9, 53.9, 58.7, 62.3, 66.9, 71.3, 76.9,
];

/// Output tap signs: two BALANCED, mutually orthogonal ±1 vectors
/// (Hadamard rows 3 and 5) — each ear hears all eight lines, the
/// ears agree on none of the signs they share, and neither sums
/// the common mode.
const TAP_L: [f32; LINES] = [
    1.0, 1.0, -1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, -1.0, 1.0, 1.0,
];
const TAP_R: [f32; LINES] = [
    1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0,
];

/// Output-tap POSITIONS as fractions of each line — irrational,
/// all different: the wet taps decouple from the loop period, so
/// the tail's envelope stops pulsing at the lines' round trips
/// (the scoreboard's flutter column, 0.55-0.74 measured on
/// HALL/PLATE vs real rooms' <0.3).
const TAP_POS: [f32; LINES] = [
    0.53, 0.67, 0.71, 0.43, 0.41, 0.79, 0.83, 0.57, 0.61, 0.33, 0.37, 0.73, 0.77, 0.51, 0.47, 0.63,
];

/// Read-head slew cap, samples per sample (~±0.3 semitones): the
/// tape-bend ceiling for size jumps; chorus and wow live far below.
const MAX_SLEW: f32 = 0.02;

/// Wet tap scale (16 incoherent lines summed — energy-matched to
/// the 8-line voicing: 0.25/sqrt(2)).
const TAP_GAIN: f32 = 0.177;

/// IN-LOOP diffusION — the metallic cure: an allpass inside each
/// line's feedback path makes every round trip's phase response
/// frequency-dependent, so the tank's modes go inharmonic and the
/// comb signature (the classic FDN ring) cannot build. Lossless,
/// so passivity is untouched; density now GROWS per trip.
const AP_MS: [f32; LINES] = [
    2.3, 2.7, 3.1, 3.7, 4.3, 4.9, 5.3, 6.1, 6.7, 7.3, 7.9, 8.7, 9.4, 10.1, 10.9, 11.7,
];
/// The SECOND in-loop stage (nested diffusion): one allpass per
/// trip left the PLATE's flutter at 0.75 (tank-only exposes the
/// lines' energy packets pulsing at their periods; rooms mask it
/// with ER). Two distinct stages per trip smear the packet twice.
const AP2_MS: [f32; LINES] = [
    1.7, 2.1, 2.9, 3.3, 3.7, 4.1, 4.7, 5.1, 5.9, 6.3, 7.1, 7.7, 8.3, 8.9, 9.7, 10.3,
];
/// Energy-weighted share of the allpasses' DC group delay that
/// the broadband T60 actually sees (calibrated by the t60 law).
const AP_DC_FACTOR: f32 = 0.55;

/// AP_G at the DIFFUSE default 0.7 (the pre-diffuse-knob voicing,
/// preserved exactly so the scoreboard baselines held).
const AP_G_SCALE: f32 = 0.714_285_7;
/// The bass shelf's crossover (low band decays at decay x bass).
const BASS_XOVER_HZ: f32 = 350.0;
/// The high shelf's crossover (top band decays at decay x the
/// material's declared high multiplier).
const HI_XOVER_HZ: f32 = 2_500.0;
/// Per-line LOW-decay spread: real rooms' low modes decay wildly
/// differently — fast components drop out through the tail and
/// the low-mid timbre EVOLVES as it dies (the field's "static,
/// one-dimensional" low mids were uniform decay).
const LOW_SPREAD: [f32; LINES] = [
    1.25, 0.78, 1.45, 0.68, 1.1, 0.85, 1.32, 0.94, 0.72, 1.38, 0.9, 1.18, 0.8, 1.28, 0.98, 1.05,
];

struct Line {
    delay: DelayLine,
    /// Current read position (glided) and its per-sample step.
    len: f32,
    step: f32,
    /// Per-trip T60 gain for the CURRENT length.
    g: f32,
    /// Damping one-pole + the loop DC blocker.
    lp: f32,
    dc_x1: f32,
    dc_y1: f32,
    /// The in-loop diffusers (two nested stages).
    ap: DelayLine,
    ap_len: f32,
    ap2: DelayLine,
    ap2_len: f32,
    /// The bass SHELF states (first-order): in a FEEDBACK path a
    /// subtractive band split must be phase-complementary or the
    /// recombination near the crossover EXCEEDS the band gains —
    /// the two-pole "steeper seam" split let loop gain pass 1 when
    /// bass or decay spread the band gains apart (field: turning
    /// either DOWN self-oscillated into the ceiling — constant
    /// distortion). A cascade of first-order shelves is minimum-
    /// phase: its magnitude interpolates strictly BETWEEN the band
    /// gains at every dial. The seam is 6 dB/oct again — safety
    /// first; a steeper safe seam is an LR design for later.
    low: f32,
    ratio_low: f32,
    /// Per-sample glides for the loop gains (worklist 7): a
    /// per-block step on a recirculating signal is a zipper.
    g_step: f32,
    ratio_low_step: f32,
    ratio_sub_step: f32,
    ratio_hi_step: f32,
    /// The HIGH shelf (worklist 3): materials declare a high-band
    /// T60 multiplier; the shelf makes the promise per trip.
    hi: f32,
    ratio_hi: f32,
    /// The SUB shelf below the modal coverage (the tank hands the
    /// deep lows to the MODES — body for the attack only).
    sub: f32,
    ratio_sub: f32,
    /// Per-line chorus phase + its own rate (never shared).
    chorus_ph: f32,
    chorus_rate: f32,
}

impl Line {
    fn new() -> Self {
        Self {
            delay: DelayLine::new(),
            len: 100.0,
            step: 0.0,
            g: 0.5,
            lp: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            ap: DelayLine::new(),
            ap_len: 100.0,
            ap2: DelayLine::new(),
            ap2_len: 100.0,
            low: 0.0,
            ratio_low: 1.0,
            g_step: 0.0,
            ratio_low_step: 0.0,
            ratio_sub_step: 0.0,
            ratio_hi_step: 0.0,
            hi: 0.0,
            ratio_hi: 1.0,
            sub: 0.0,
            ratio_sub: 1.0,
            chorus_ph: 0.0,
            chorus_rate: 0.9,
        }
    }
}

pub(super) struct Tank {
    lines: [Line; LINES],
    /// DC blocker pole (~20 Hz at the prepared rate).
    dc_r: f32,
    /// Bass crossover one-pole coefficient.
    k_bass: f32,
    k_hi: f32,
    /// Sub handoff crossover (scales with the modal coverage:
    /// bigger rooms slide their modes down, the tank keeps more).
    k_sub: f32,
    /// The in-loop diffusion coefficient (from the DIFFUSE dial).
    ap_g: f32,
    sr: f32,
}

impl Tank {
    pub fn new() -> Self {
        Self {
            lines: core::array::from_fn(|i| {
                let mut l = Line::new();
                // Incommensurate slow rates around ~0.9 Hz.
                l.chorus_ph = (i as f32) * 2.399_963;
                l.chorus_rate = 0.9 * (0.55 + 0.9 * ((i as f32 * 0.618_034) % 1.0));
                l
            }),
            dc_r: 0.9974,
            k_bass: 0.05,
            k_hi: 0.25,
            k_sub: 0.01,
            ap_g: 0.5,
            sr: 48_000.0,
        }
    }

    /// Prepare-time only: capacity for size 3.0 plus modulation
    /// headroom (the audio path never allocates).
    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.dc_r = 1.0 - core::f32::consts::TAU * 20.0 / sr;
        self.k_bass = 1.0 - (-core::f32::consts::TAU * BASS_XOVER_HZ / sr).exp();
        self.k_hi = 1.0 - (-core::f32::consts::TAU * HI_XOVER_HZ / sr).exp();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let cap = (BASE_MS[i] * 0.001 * sr * 3.0) as usize + 96;
            line.delay.prepare(cap);
            line.len = BASE_MS[i] * 0.001 * sr;
            // INTEGER lengths: a linear read at a FIXED fractional
            // position inside a feedback loop is hidden compounding
            // HF loss (review catch) — these lengths never glide,
            // so they round.
            line.ap_len = (AP_MS[i] * 0.001 * sr).round();
            line.ap.prepare(line.ap_len as usize + 8);
            line.ap2_len = (AP2_MS[i] * 0.001 * sr).round();
            line.ap2.prepare(line.ap2_len as usize + 8);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in self.lines.iter_mut() {
            line.delay.reset();
            line.ap.reset();
            line.ap2.reset();
            line.lp = 0.0;
            line.dc_x1 = 0.0;
            line.dc_y1 = 0.0;
            line.step = 0.0;
            line.low = 0.0;
            line.sub = 0.0;
            line.hi = 0.0;
        }
        // Transport stop is a KNOWN PLACE (the dynamo lesson): the
        // chorus phases restart, or renders drift across stop/start.
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.chorus_ph = (i as f32) * 2.399_963;
        }
    }

    /// Block-rate voicing: aim every read head (size × chorus ×
    /// wow, slew-capped) and set the per-trip T60 gains.
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        t60_s: f32,
        bass: f32,
        hi_mult: f32,
        diffuse: f32,
        chorus_samples: f32,
        frames: usize,
    ) {
        let n = frames.max(1) as f32;
        // Allpass DENSITY peaks near g ~0.5-0.6 and gets SPARSER
        // toward g→1 (the direct term dominates — measured: plate
        // NED slowed at the dial's top). The cap keeps the top of
        // the dial in the dense region.
        self.ap_g = (AP_G_SCALE * diffuse).clamp(0.0, 0.58);
        // The modal bank covers up to ~(85/size) Hz — the tank
        // hands off below that. (An external review caught the
        // slice-9 patch for this line silently missing its anchor:
        // k_sub sat at its 0.01 init — ~77 Hz at 48k and
        // sample-rate-dependent. Every patch asserts now.)
        let sub_hz = (85.0 / size.max(0.1)).clamp(24.0, 170.0);
        self.k_sub = 1.0 - (-core::f32::consts::TAU * sub_hz / self.sr).exp();
        let block_s = n / self.sr;
        for (i, line) in self.lines.iter_mut().enumerate() {
            // Per-line INDEPENDENT rates (field: "sounds like a
            // single chorus LFO in the tank" — it was: one master
            // phase, sixteen offsets, ONE rate). Sixteen slow
            // wobbles at incommensurate rates read as water, not
            // vibrato.
            line.chorus_ph = (line.chorus_ph + core::f32::consts::TAU * line.chorus_rate * block_s)
                % core::f32::consts::TAU;
            let chorus = chorus_samples * line.chorus_ph.sin();
            let base = BASE_MS[i] * 0.001 * self.sr * size;
            let target = base + chorus;
            let slew = MAX_SLEW * n;
            line.step = (target - line.len).clamp(-slew, slew) / n;
            // The T60 promise: per-trip loss such that the FULL
            // loop — line plus the in-loop diffuser's DC group
            // delay — decays 60 dB in the dialed seconds.
            // Our allpass form is (−g+z⁻ᴺ)/(1−gz⁻ᴺ): DC group delay
            // N(1+g)/(1−g) = 3N at g=0.5 — the (1−g)/(1+g) first
            // draft under-compensated 9x and a small room's decay
            // read +60% long (the scoreboard's first catch).
            // An allpass's group delay is N(1+g)/(1-g) at DC but
            // N(1-g)/(1+g) at HF — with two stages the spread
            // doubled and full-DC compensation over-sized the loss
            // (broadband T60 read 32% fast, measured). The factor
            // is the energy-weighted middle, calibrated by the t60
            // law.
            let ap_dc = AP_DC_FACTOR * (1.0 + self.ap_g) / (1.0 - self.ap_g).max(0.28);
            let loop_len = base + (line.ap_len + line.ap2_len) * ap_dc;
            let g_new = t60_gain(loop_len, t60_s * self.sr);
            line.g_step = (g_new - line.g) / n;
            // The low band rings decay x bass x a PER-LINE spread
            // (real low modes are neighbors that decay wildly
            // differently — the low-mid timbre evolves as fast
            // components drop out). Applied as SHELF RATIOS so the
            // loop's magnitude stays between the band gains.
            let g_low = t60_gain(loop_len, t60_s * bass * LOW_SPREAD[i] * self.sr);
            // Below the modal coverage the tank decays FAST (x0.3):
            // attack body only — the standing waves own the sustain.
            let g_sub = t60_gain(loop_len, t60_s * bass * 0.3 * self.sr);
            line.ratio_low_step = (g_low / g_new.max(1e-6) - line.ratio_low) / n;
            line.ratio_sub_step = (g_sub / g_low.max(1e-6) - line.ratio_sub) / n;
            // The material's HIGH promise (xover 2.5 kHz — above
            // the bass shelf, so in the top band the cascade's
            // gain is g·ratio_hi exactly). DAMP absorption still
            // composes on top: the dial stays the air, the
            // material stays the wall.
            let g_hi = t60_gain(loop_len, t60_s * hi_mult * self.sr);
            line.ratio_hi_step = (g_hi / g_new.max(1e-6) - line.ratio_hi) / n;
        }
    }

    /// One sample through the whole tank. `inj_l`/`inj_r` feed the
    /// even/odd lines; `damp_k` is the absorption one-pole
    /// coefficient.
    #[inline]
    pub fn tick(&mut self, inj_l: f32, inj_r: f32, damp_k: f32) -> (f32, f32) {
        let mut fed = [0.0f32; LINES];
        let mut wet_l = 0.0f32;
        let mut wet_r = 0.0f32;
        for (i, line) in self.lines.iter_mut().enumerate() {
            let y = line.delay.read_cubic(line.len);
            // The wet tap reads MID-LINE (irrational fraction) —
            // output timing decoupled from the loop period.
            let yo = line.delay.read(line.len * TAP_POS[i]);
            line.len += line.step;
            wet_l += TAP_L[i] * yo;
            wet_r += TAP_R[i] * yo;
            // Absorption, then the bridge terminates displacement
            // (DC out of the loop), then the T60 loss.
            line.lp += damp_k * (y - line.lp);
            let dc = line.lp - line.dc_x1 + self.dc_r * line.dc_y1;
            line.dc_x1 = line.lp;
            line.dc_y1 = dc;
            // The bass + sub SHELVES in cascade (first-order,
            // minimum-phase: loop magnitude provably between the
            // band gains at every dial — the passivity-at-every-
            // dial law).
            line.low += self.k_bass * (dc - line.low);
            let x1 = dc + (line.ratio_low - 1.0) * line.low;
            line.sub += self.k_sub * (x1 - line.sub);
            let x2 = x1 + (line.ratio_sub - 1.0) * line.sub;
            // The high shelf works on the LP's complement — same
            // first-order minimum-phase family, so the loop's
            // magnitude stays between the band gains at any dial.
            line.hi += self.k_hi * (x2 - line.hi);
            let x3 = x2 + (line.ratio_hi - 1.0) * (x2 - line.hi);
            fed[i] = line.g * x3;
            line.g += line.g_step;
            line.ratio_low += line.ratio_low_step;
            line.ratio_sub += line.ratio_sub_step;
            line.ratio_hi += line.ratio_hi_step;
        }
        hadamard16(&mut fed);
        for (i, line) in self.lines.iter_mut().enumerate() {
            // The in-loop diffusers, two nested stages (lossless —
            // passivity holds).
            let ap_out = line.ap.read_int(line.ap_len as usize);
            let mut fb = ap_out - self.ap_g * fed[i];
            line.ap.write(fed[i] + self.ap_g * fb);
            line.ap.advance();
            let ap2_out = line.ap2.read_int(line.ap2_len as usize);
            let y2 = ap2_out - self.ap_g * fb;
            line.ap2.write(fb + self.ap_g * y2);
            line.ap2.advance();
            fb = y2;
            let inject = if i % 2 == 0 { inj_l } else { inj_r };
            line.delay.write(inject + fb);
            line.delay.advance();
        }
        (wet_l * TAP_GAIN, wet_r * TAP_GAIN)
    }
}

/// The energy-preserving junction: a 16-point Walsh–Hadamard
/// butterfly scaled to unit norm — every line hears every line,
/// and the matrix itself neither adds nor removes energy.
#[inline]
pub(super) fn hadamard16(v: &mut [f32; LINES]) {
    let mut stride = 1usize;
    while stride < LINES {
        let mut i = 0;
        while i < LINES {
            for j in i..i + stride {
                let a = v[j];
                let b = v[j + stride];
                v[j] = a + b;
                v[j + stride] = a - b;
            }
            i += 2 * stride;
        }
        stride *= 2;
    }
    for x in v.iter_mut() {
        *x *= 0.25; // 1/sqrt(16)
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// The junction preserves energy: ‖Hv‖ = ‖v‖ for any v.
    #[test]
    fn the_junction_is_orthogonal() {
        let mut v = [
            0.7f32, -0.3, 0.5, 0.1, -0.9, 0.2, 0.4, -0.6, 0.3, -0.8, 0.6, -0.2, 0.15, -0.45, 0.55,
            -0.35,
        ];
        let before: f32 = v.iter().map(|x| x * x).sum();
        hadamard16(&mut v);
        let after: f32 = v.iter().map(|x| x * x).sum();
        assert!(
            (before - after).abs() < 1e-4,
            "energy moved: {before} → {after}"
        );
    }

    /// The tap vectors are balanced and mutually orthogonal — the
    /// stereo field's decorrelation is by construction.
    #[test]
    fn the_taps_are_orthogonal_and_balanced() {
        let dot: f32 = TAP_L.iter().zip(&TAP_R).map(|(a, b)| a * b).sum();
        assert_eq!(dot, 0.0, "L·R must be 0");
        assert_eq!(TAP_L.iter().sum::<f32>(), 0.0, "L must be balanced");
        assert_eq!(TAP_R.iter().sum::<f32>(), 0.0, "R must be balanced");
    }
}
