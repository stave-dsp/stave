//! THE PLATE — a dedicated 2D-membrane skeleton, not a flavor of
//! the room (the topologies arc's ratifying critique: one skeleton,
//! many shaders = every preset sounds like the same room; the old
//! "PLATE" preset was the room's FDN with the ER muted).
//!
//! A plate reverb is a stiff steel sheet: wave transit is
//! MILLISECONDS (the room's lines are 20-70 ms — 10-40x too long),
//! echo density is effectively instant, and DISPERSION IS THE
//! VOICE — bending-wave speed grows with frequency, so every
//! reflection chirps (highs arrive first, lows smear behind). The
//! decay is DAMPER-controlled (a felt plate moved toward the
//! sheet), not air-and-walls.
//!
//! The skeleton (second voicing — the first was sixteen SUB-MS
//! lines and it rang like a cheap Schroeder, field-confirmed: a
//! 1 ms loop is a comb every 1 kHz and at plate T60s each peak is
//! a screaming Q; transit time is NOT the loop time): INSTANT
//! density comes from an INPUT DIFFUSION chain per channel
//! (Dattorro's lesson), smoothness from sixteen 9-43 ms loop
//! lines (comb spacing 23-110 Hz), the chirp from a deep
//! dispersion cascade per line, and a WHISPER of per-line motion
//! (±0.35 samples — a real sheet's mode density is thousands; at
//! sixteen lines a static loop rings, measured). Damper
//! absorption + DC bridge + bass shelf + declared T60, Hadamard
//! junction, true-stereo drive points, decorrelated pickups,
//! pickup-brightness tilt from TONE. No geometry stages at all —
//! the CPU answer falls out of the architecture.

use stave_dsp::{t60_gain, DelayLine};

use super::tank::hadamard16;

const LINES: usize = 16;

/// Loop lengths at size 1 — 27-90 ms on a GEOMETRIC ladder with
/// jitter. THREE lessons live here: (1) the first table was
/// near-ARITHMETIC (~2.4 ms steps) — equal increments align every
/// line's comb into a super-comb (meter: 0.756 vs room 0.491);
/// (2) geometric spacing never repeats an interval; (3) SHORT
/// lines put comb spacing at 65-110 Hz — the PITCHED-metallic
/// percept range — while long lines sit at 11-37 Hz, which the
/// ear reads as texture. Mode density is the loop's job, onset
/// density is the input diffusion's; transit time is NEITHER.
const BASE_MS: [f32; LINES] = [
    26.83, 29.31, 32.09, 35.47, 38.93, 42.71, 46.13, 50.87, 54.79, 59.63, 63.91, 68.99, 73.61,
    78.83, 83.27, 89.51,
];

/// TWO nested in-loop diffusion allpasses per line (integer
/// lengths, irrational spread) — the room tank's twice-proven
/// ridge-killer; the first plate voicing had NONE (its dispersion
/// cascade rotates phase but barely diffuses TIME).
const LOOP_AP_MS: [f32; LINES] = [
    2.31, 3.17, 2.73, 3.91, 2.11, 3.43, 2.87, 3.67, 2.53, 3.29, 2.97, 3.79, 2.23, 3.53, 2.67, 3.97,
];
const LOOP_AP2_MS: [f32; LINES] = [
    5.71, 4.93, 6.37, 5.23, 6.91, 4.73, 6.13, 5.47, 6.67, 4.83, 6.29, 5.03, 7.13, 4.63, 5.93, 5.57,
];
const LOOP_AP_G: f32 = 0.5;

/// The INPUT DIFFUSION chain (per channel): four short allpasses
/// smear the strike into instant density BEFORE the loop — the
/// onset is dense even though the loop lines are long.
const IN_DIFF: usize = 6;
const IN_DIFF_MS: [f32; IN_DIFF] = [1.13, 2.87, 5.31, 8.71, 11.93, 14.39];
const IN_DIFF_G: f32 = 0.68;

/// The floor of motion: even at MOTION 0 a whisper (±0.15
/// samples) stays — sixteen static lines ring (measured); a real
/// sheet's mode density is thousands. MOTION deepens it.
const SHEEN_FLOOR: f32 = 0.55;
const SHEEN_SPAN: f32 = 7.0;

/// THE PICKUPS ARE PLACES ON THE SHEET (the stereo-honesty
/// slice): each line carries a pseudo-mode shape sin(pi*u*x) *
/// sin(pi*v*y) with ODD wavenumbers (odd modes are all alive at
/// the center — converged pickups lose no lines), and each
/// pickup's tap weight is that shape read AT ITS POSITION. WIDTH
/// moves the pickups apart: 0 = coincident (mono wet, by
/// geometry), 1 = a healthy stereo pair, 2 = far corners (mode
/// sign flips = honest hyper-wide). No post M/S in the plate path.
const MODE_U: [f32; LINES] = [
    1.0, 3.0, 5.0, 1.0, 3.0, 5.0, 1.0, 3.0, 5.0, 1.0, 3.0, 5.0, 1.0, 3.0, 5.0, 3.0,
];
const MODE_V: [f32; LINES] = [
    3.0, 1.0, 3.0, 5.0, 5.0, 1.0, 1.0, 3.0, 5.0, 3.0, 1.0, 5.0, 5.0, 1.0, 3.0, 3.0,
];
/// Pickup positions as irrational line fractions (output timing
/// decoupled from the loop period).
const TAP_POS: [f32; LINES] = [
    0.618, 0.382, 0.786, 0.472, 0.694, 0.528, 0.847, 0.361, 0.573, 0.729, 0.417, 0.653, 0.809,
    0.446, 0.567, 0.712,
];
/// Second pickup set: the same pickups, hearing the in-loop
/// diffusers' interior at a decorrelated lag (Dattorro's many-tap
/// lesson — 32 arrivals per channel for one integer read).
const TAP_GAIN: f32 = 0.177;
const TAP2_GAIN: f32 = 0.11;

/// The dispersion cascade — the plate's voice. Eight stages per
/// line, and the pole is STAGGERED PER LINE (±25% around the
/// STIFF-dialed center): sixteen identical cascades put one
/// concentrated "zing" on every line — the same coloration
/// sixteen times over is itself a metallic signature. First-order
/// allpass poles toward +1 disperse (the dynamo wire's lesson).
const DISP_STAGES: usize = 8;
const DISP_A_MIN: f32 = 0.35;
const DISP_A_MAX: f32 = 0.88;

/// AP group-delay compensation factor (energy-weighted middle —
/// calibrated in the tank by the t60 law; same first-order family).
const AP_DC_FACTOR: f32 = 0.55;

const MAX_SLEW: f32 = 0.02;

/// Bass shelf crossover (the plate's low world rings at decay ×
/// bass — one shelf; there is no modal handoff to protect).
const BASS_XOVER_HZ: f32 = 250.0;

struct PLine {
    delay: DelayLine,
    len: f32,
    step: f32,
    g: f32,
    g_step: f32,
    lp: f32,
    dc_x1: f32,
    dc_y1: f32,
    low: f32,
    ratio_low: f32,
    ratio_low_step: f32,
    disp: [f32; DISP_STAGES],
    /// This line's own dispersion pole (staggered around STIFF).
    disp_a: f32,
    /// Pickup weights (from the pickup positions), per-sample
    /// glided (a per-block step on an output tap is a zipper).
    w_l: f32,
    w_l_step: f32,
    w_r: f32,
    w_r_step: f32,
    ap: DelayLine,
    ap_len: usize,
    ap2: DelayLine,
    ap2_len: usize,
    sheen_ph: f32,
    sheen_rate: f32,
}

/// One input-diffusion stage (short fixed integer allpass).
struct InDiff {
    delay: DelayLine,
    len: usize,
}

pub(super) struct Plate {
    lines: [PLine; LINES],
    /// [L chain, R chain].
    in_diff: [[InDiff; IN_DIFF]; 2],
    in_g: f32,
    /// Pickup brightness (TONE): one-pole tilt per channel.
    tilt_lp: [f32; 2],
    k_tilt: f32,
    tilt_b: f32,
    dc_r: f32,
    k_bass: f32,
    disp_a: f32,
    sr: f32,
    cold: bool,
}

impl Plate {
    pub fn new() -> Self {
        Self {
            lines: core::array::from_fn(|_| PLine {
                delay: DelayLine::new(),
                len: 100.0,
                step: 0.0,
                g: 0.5,
                g_step: 0.0,
                lp: 0.0,
                dc_x1: 0.0,
                dc_y1: 0.0,
                low: 0.0,
                ratio_low: 1.0,
                ratio_low_step: 0.0,
                disp: [0.0; DISP_STAGES],
                disp_a: 0.5,
                w_l: 0.0,
                w_l_step: 0.0,
                w_r: 0.0,
                w_r_step: 0.0,
                ap: DelayLine::new(),
                ap_len: 96,
                ap2: DelayLine::new(),
                ap2_len: 192,
                sheen_ph: 0.0,
                sheen_rate: 0.5,
            }),
            in_diff: core::array::from_fn(|_| {
                core::array::from_fn(|_| InDiff {
                    delay: DelayLine::new(),
                    len: 48,
                })
            }),
            in_g: IN_DIFF_G,
            tilt_lp: [0.0; 2],
            k_tilt: 0.15,
            tilt_b: 1.0,
            dc_r: 0.9974,
            k_bass: 0.05,
            disp_a: 0.5,
            sr: 48_000.0,
            cold: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.dc_r = 1.0 - core::f32::consts::TAU * 20.0 / sr;
        self.k_bass = 1.0 - (-core::f32::consts::TAU * BASS_XOVER_HZ / sr).exp();
        for (i, line) in self.lines.iter_mut().enumerate() {
            // Capacity for size 3 (transit scale √3) + headroom.
            let cap = (BASE_MS[i] * 0.001 * sr * 1.8) as usize + 16;
            line.delay.prepare(cap);
            line.len = BASE_MS[i] * 0.001 * sr;
            line.sheen_ph = (i as f32) * 2.399_963;
            line.sheen_rate = 0.4 * (0.55 + 0.9 * ((i as f32 * 0.618_034) % 1.0));
            line.ap_len = (LOOP_AP_MS[i] * 0.001 * sr).round() as usize;
            line.ap.prepare(line.ap_len + 8);
            line.ap2_len = (LOOP_AP2_MS[i] * 0.001 * sr).round() as usize;
            line.ap2.prepare(line.ap2_len + 8);
        }
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            for (k, st) in chain.iter_mut().enumerate() {
                // R detuned 1.07 (the diffuser convention).
                let det = if ch == 0 { 1.0 } else { 1.07 };
                st.len = (IN_DIFF_MS[k] * det * 0.001 * sr).round() as usize;
                st.delay.prepare(st.len + 8);
            }
        }
        self.k_tilt = 1.0 - (-core::f32::consts::TAU * 1_600.0 / sr).exp();
        self.reset();
        self.cold = true;
    }

    pub fn reset(&mut self) {
        for line in self.lines.iter_mut() {
            line.delay.reset();
            line.step = 0.0;
            line.g_step = 0.0;
            line.ratio_low_step = 0.0;
            line.lp = 0.0;
            line.dc_x1 = 0.0;
            line.dc_y1 = 0.0;
            line.low = 0.0;
            line.disp = [0.0; DISP_STAGES];
            line.ap.reset();
            line.ap2.reset();
        }
        for (i, line) in self.lines.iter_mut().enumerate() {
            // Transport stop is a KNOWN PLACE (the dynamo lesson).
            line.sheen_ph = (i as f32) * 2.399_963;
        }
        for chain in self.in_diff.iter_mut() {
            for st in chain.iter_mut() {
                st.delay.reset();
            }
        }
        self.tilt_lp = [0.0; 2];
    }

    /// Block-rate voicing: transit scale from SIZE, T60 gains from
    /// the dialed decay (loop length INCLUDES the dispersion
    /// cascade's group delay — the AP compensation lesson), bass
    /// shelf, dispersion depth from STIFF. All per-sample-glided
    /// (the robustness doctrine).
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        t60_s: f32,
        bass: f32,
        stiff: f32,
        diffuse: f32,
        tone: f32,
        motion: f32,
        width: f32,
        frames: usize,
    ) {
        let n = frames.max(1) as f32;
        // Transit scale: a bigger plate, not a bigger room —
        // sub-linear (wave transit grows with the sheet's edge).
        let scale = size.clamp(0.25, 3.0).sqrt();
        self.disp_a = DISP_A_MIN + (DISP_A_MAX - DISP_A_MIN) * stiff.clamp(0.0, 1.0);
        let ap_dc = AP_DC_FACTOR * (1.0 + LOOP_AP_G) / (1.0 - LOOP_AP_G);
        // DIFFUSE drives the strike's smear (capped in the dense
        // allpass region — the density-peak lesson).
        self.in_g = (IN_DIFF_G * diffuse / 0.7).clamp(0.0, 0.72);
        // TONE is the PICKUP's brightness (a contact mic hears the
        // sheet, not the room's walls).
        self.tilt_b = 2f32.powf(tone.clamp(-1.0, 1.0) * 1.2);
        // THE PICKUP POSITIONS from WIDTH — asymmetric in BOTH
        // axes, on purpose: for ODD modes a mirror pair hears
        // IDENTICAL x-weights (sin(pi*u*(1-x)) = sin(pi*u*x)), so
        // symmetric pickups barely decorrelate (measured corr
        // 0.82 at width 1). Real plate pickups are never placed
        // symmetrically either.
        let spread = (width * 0.5).clamp(0.0, 1.0);
        let (xl, yl) = (0.5 - 0.37 * spread, 0.5 - 0.19 * spread);
        let (xr, yr) = (0.5 + 0.26 * spread, 0.5 + 0.33 * spread);
        let shape = |u: f32, v: f32, x: f32, y: f32| {
            (core::f32::consts::PI * u * x).sin() * (core::f32::consts::PI * v * y).sin()
        };
        let mut wl = [0.0f32; LINES];
        let mut wr = [0.0f32; LINES];
        let (mut el, mut er) = (0.0f32, 0.0f32);
        for i in 0..LINES {
            wl[i] = shape(MODE_U[i], MODE_V[i], xl, yl);
            wr[i] = shape(MODE_U[i], MODE_V[i], xr, yr);
            el += wl[i] * wl[i];
            er += wr[i] * wr[i];
        }
        // Constant pickup energy at any spread (a moved mic does
        // not get quieter).
        let gl = (LINES as f32 / el.max(1e-6)).sqrt();
        let gr = (LINES as f32 / er.max(1e-6)).sqrt();
        let block_s = n / self.sr;
        // MOTION on the sheet: the sheen's depth (polymorphic —
        // the same dial that runs the room tank's chorus).
        let sheen = SHEEN_FLOOR + SHEEN_SPAN * motion * motion;
        for (i, line) in self.lines.iter_mut().enumerate() {
            // ONE pole for every line: dispersion is a MATERIAL
            // property — per-line stagger made the measured chirp
            // hostage to where the pickups happen to sit (weights
            // can mute any line; ±25% stagger read 10 ms → 3 ms
            // when the pickups landed wrong). The zing the stagger
            // decorrelated is already handled by the in-loop
            // diffusers, the geometric line ladder and the sheen —
            // the metal meter referees that claim.
            line.disp_a = self.disp_a;
            let disp_dc = AP_DC_FACTOR * DISP_STAGES as f32 * (1.0 + line.disp_a)
                / (1.0 - line.disp_a).max(0.2);
            line.sheen_ph = (line.sheen_ph + core::f32::consts::TAU * line.sheen_rate * block_s)
                % core::f32::consts::TAU;
            let base = BASE_MS[i] * 0.001 * self.sr * scale + sheen * line.sheen_ph.sin();
            let slew = MAX_SLEW * n;
            let loop_len = base + disp_dc + (line.ap_len + line.ap2_len) as f32 * ap_dc;
            let g_new = t60_gain(loop_len, t60_s * self.sr);
            let g_low = t60_gain(loop_len, t60_s * bass * self.sr);
            if self.cold {
                line.len = base;
                line.step = 0.0;
                line.g = g_new;
                line.g_step = 0.0;
                line.ratio_low = g_low / g_new.max(1e-6);
                line.ratio_low_step = 0.0;
                line.w_l = wl[i] * gl;
                line.w_l_step = 0.0;
                line.w_r = wr[i] * gr;
                line.w_r_step = 0.0;
            } else {
                line.w_l_step = (wl[i] * gl - line.w_l) / n;
                line.w_r_step = (wr[i] * gr - line.w_r) / n;
                line.step = (base - line.len).clamp(-slew, slew) / n;
                line.g_step = (g_new - line.g) / n;
                line.ratio_low_step = (g_low / g_new.max(1e-6) - line.ratio_low) / n;
            }
        }
        self.cold = false;
    }

    /// One sample across the sheet.
    #[inline]
    pub fn tick(&mut self, inj_l: f32, inj_r: f32, damp_k: f32) -> (f32, f32) {
        // THE STRIKE SMEARS FIRST: four short allpasses per
        // channel turn the hit into instant density before it
        // ever enters the loop (Dattorro's lesson — onset density
        // is the input's job, smoothness is the loop's).
        let mut inj = [inj_l, inj_r];
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            let mut v = inj[ch];
            for st in chain.iter_mut() {
                let d = st.delay.read_int(st.len);
                let y = d - self.in_g * v;
                st.delay.write(v + self.in_g * y);
                st.delay.advance();
                v = y;
            }
            inj[ch] = v;
        }
        let (inj_l, inj_r) = (inj[0], inj[1]);
        let mut fed = [0.0f32; LINES];
        let mut wet_l = 0.0f32;
        let mut wet_r = 0.0f32;
        for (i, line) in self.lines.iter_mut().enumerate() {
            let y = line.delay.read_cubic(line.len);
            let yo = line.delay.read(line.len * TAP_POS[i]);
            line.len += line.step;
            wet_l += line.w_l * yo;
            wet_r += line.w_r * yo;
            line.w_l += line.w_l_step;
            line.w_r += line.w_r_step;
            // Damper absorption, DC bridge, the bass shelf, T60.
            line.lp += damp_k * (y - line.lp);
            let dc = line.lp - line.dc_x1 + self.dc_r * line.dc_y1;
            line.dc_x1 = line.lp;
            line.dc_y1 = dc;
            line.low += self.k_bass * (dc - line.low);
            let x1 = dc + (line.ratio_low - 1.0) * line.low;
            fed[i] = line.g * x1;
            line.g += line.g_step;
            line.ratio_low += line.ratio_low_step;
        }
        hadamard16(&mut fed);
        for (i, line) in self.lines.iter_mut().enumerate() {
            // True-stereo drive points: L and R strike the sheet
            // at different spots (alternate lines). The strike
            // enters BEFORE the dispersion — even the first
            // arrival chirps (the strike propagates dispersively
            // from driver to pickup; injected after, the onset
            // measured clean and the chirp law caught it).
            let inject = if i % 2 == 0 { inj_l } else { inj_r };
            let mut fb = fed[i] + inject;
            // The in-loop diffusers (lossless — passivity holds):
            // the modal structure smears in TIME every trip.
            let ap_out = line.ap.read_int(line.ap_len);
            let y1 = ap_out - LOOP_AP_G * fb;
            line.ap.write(fb + LOOP_AP_G * y1);
            line.ap.advance();
            let ap2_out = line.ap2.read_int(line.ap2_len);
            // The second pickup set: the same two pickups hearing
            // the diffusers' interior at a decorrelated lag.
            let yo2 = line.ap2.read_int(line.ap2_len / 2);
            wet_l += line.w_l * yo2 * (TAP2_GAIN / TAP_GAIN);
            wet_r += line.w_r * yo2 * (TAP2_GAIN / TAP_GAIN);
            let y2 = ap2_out - LOOP_AP_G * y1;
            line.ap2.write(y1 + LOOP_AP_G * y2);
            line.ap2.advance();
            fb = y2;
            // THE VOICE: the dispersion cascade — every trip
            // through the sheet chirps (lossless allpasses;
            // passivity holds by construction; the pole is THIS
            // line's own).
            let a = line.disp_a;
            for z in line.disp.iter_mut() {
                let ap = -a * fb + *z;
                *z = fb + a * ap;
                fb = ap;
            }
            line.delay.write(fb);
            line.delay.advance();
        }
        // The pickup tilt (TONE): brightness at the contact mic.
        let mut out = [wet_l * TAP_GAIN, wet_r * TAP_GAIN];
        for ch in 0..2 {
            self.tilt_lp[ch] += self.k_tilt * (out[ch] - self.tilt_lp[ch]);
            out[ch] = self.tilt_lp[ch] + self.tilt_b * (out[ch] - self.tilt_lp[ch]);
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// THE CASCADE RUNS THE TOP FIRST — the dispersion physics,
    /// proven on the cascade alone (no reverb confounds): an
    /// impulse through the 8 stages at the STIFF ceiling arrives
    /// high-first, low-last, by a margin the ear can use.
    #[test]
    fn the_cascade_runs_the_top_first() {
        let a = DISP_A_MAX;
        let mut z = [0.0f32; DISP_STAGES];
        let n = 4_096usize;
        let mut out = vec![0.0f32; n];
        for (i, o) in out.iter_mut().enumerate() {
            let mut v = if i == 0 { 1.0 } else { 0.0 };
            for zz in z.iter_mut() {
                let ap = -a * v + *zz;
                *zz = v + a * ap;
                v = ap;
            }
            *o = v;
        }
        // Band energy centroids of the cascade's own response.
        let centroid = |hz: f32| -> f32 {
            let (mut num, mut den) = (0.0f32, 0.0f32);
            let k = 1.0 - (-core::f32::consts::TAU * hz / 48_000.0).exp();
            // One-pole band probe: LP at hz vs LP at hz/2 —
            // difference isolates energy near hz.
            let (mut z1, mut z2) = (0.0f32, 0.0f32);
            let k2 = 1.0 - (-core::f32::consts::TAU * hz * 0.5 / 48_000.0).exp();
            for (i, x) in out.iter().enumerate() {
                z1 += k * (x - z1);
                z2 += k2 * (x - z2);
                let b = z1 - z2;
                num += i as f32 * b * b;
                den += b * b;
            }
            num / den.max(1e-20)
        };
        let low = centroid(200.0);
        let high = centroid(6_000.0);
        assert!(
            low - high > 24.0,
            "the cascade must disperse: low centroid {low:.0} vs high {high:.0} samples"
        );
    }
}
