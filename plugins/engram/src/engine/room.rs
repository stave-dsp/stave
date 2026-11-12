//! `Room` — the reservoir at room scale, with a SHAPE you can draw.
//! Sixteen delay lines are the neurons; each sits at a point on the
//! display's field (`geometry.rs` says where):
//!
//! * a line's LENGTH follows its node's distance from the field's
//!   centre — pull a node outward and that part of the room grows;
//! * the MIXING between lines follows how close their nodes are: a
//!   dense Hadamard base (so any drawing still diffuses) plus a
//!   coupling that falls off with distance, projected to the NEAREST
//!   ORTHOGONAL matrix (Newton–Schulz polar iteration) and guarded to
//!   unit norm — so whatever is drawn, the mix neither gains nor
//!   loses energy, and the room is stable by construction.
//!
//! Per line: damping (TONE), the mix, its T60 gain (DECAY), a soft
//! ceiling on the write (the network's nonlinearity — linear at normal
//! levels).
//!
//! The GENERATED room (space.rs) replaces DECAY/TONE's single T60 and
//! one-pole damping, by `Shape::gen`, with each line's 7-band
//! attenuation filter (decay_eq.rs): every octave decays at the decoded
//! room's own T60. Its per-octave targets blend the knobs' response
//! (gain × damping, at each octave) with the room's, so SPACE morphs
//! continuously; at SPACE 0 the classic path runs untouched.

use core::f32::consts::TAU;

use stave_dsp::DelayLine;

use super::decay_eq::{DecayEq, Design, OCTAVES};

pub(super) const LINES: usize = 16;
/// A per-line length spread, so two nodes at the same radius never
/// make identical (metallic) lines.
const SPREAD: [f32; LINES] = [
    0.931, 1.047, 0.967, 1.071, 0.953, 1.019, 0.989, 1.061, 0.943, 1.033, 0.977, 1.053, 0.961, 1.011, 0.997, 1.067,
];
/// SIZE 0 … 1: the room's span, ms (a line at the field's rim).
const SPAN_MS: (f32, f32) = (30.0, 190.0);
/// A node at the centre makes a line this share of the span.
const MIN_SHARE: f32 = 0.25;
const DAMP_HZ: (f32, f32) = (1_500.0, 16_000.0);
const WANDER_MS: f32 = 0.35;
const INPUT_GAIN: f32 = 0.5;
const CEILING: f32 = 1.5;
/// How strongly closeness couples lines, against the Hadamard base,
/// and how far (in field heights) closeness reaches.
const COUPLING: f32 = 1.0;
const REACH: f32 = 0.22;
const CENTRES: [f32; OCTAVES] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
/// DIFFUSION in every line: two Schroeder allpasses (lossless — they
/// change no decay time) at these lengths (ms, per line, detuned so no
/// two lines share one), gain DIFFUSE. Each pass multiplies the echo
/// density: without them the network's first echoes are sparse clumps
/// at the lines' lengths (measured: nothing until 55 ms, then isolated
/// arrivals) and its modes too few — the "algorithmic", metallic tail.
const DIFF_MS: [(f32, f32); 2] = [(1.3, 3.9), (0.37, 1.31)];
const DIFFUSE: f32 = 0.55;
const REFIT: usize = 4;

/// A Schroeder allpass's group delay (samples) at ω: M(1−g²)/(1 − 2g·cos ωM + g²).
fn allpass_delay(m: f32, g: f32, w: f32) -> f32 {
    m * (1.0 - g * g) / (1.0 - 2.0 * g * (w * m).cos() + g * g)
}

/// A Schroeder allpass: y = −g·x + d, written x + g·y.
struct Allpass {
    line: DelayLine,
    len: usize,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32, g: f32) -> f32 {
        let d = self.line.read_int(self.len);
        let y = -g * x + d;
        self.line.write(x + g * y);
        self.line.advance();
        y
    }
}

/// The knobs' damping one-pole's gain at `hz`, dB.
fn onepole_db(b: f32, hz: f32, sr: f32) -> f32 {
    let w = TAU * hz.min(0.45 * sr) / sr;
    let (re, im) = (1.0 - b * w.cos(), b * w.sin());
    20.0 * ((1.0 - b) / (re * re + im * im).sqrt()).max(1e-9).log10()
}

#[inline]
fn ceiling(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 {
        x
    } else {
        let over = ((a - 1.0) / (CEILING - 1.0)).min(3.0);
        x.signum() * (1.0 + (CEILING - 1.0) * over * (27.0 + over * over) / (27.0 + 9.0 * over * over))
    }
}

type Mat = [f32; LINES * LINES];

fn matmul(a: &Mat, b: &Mat, out: &mut Mat) {
    for i in 0..LINES {
        for j in 0..LINES {
            let mut s = 0.0;
            for k in 0..LINES {
                s += a[i * LINES + k] * b[k * LINES + j];
            }
            out[i * LINES + j] = s;
        }
    }
}

fn transpose(a: &Mat, out: &mut Mat) {
    for i in 0..LINES {
        for j in 0..LINES {
            out[j * LINES + i] = a[i * LINES + j];
        }
    }
}

/// The 16-point Hadamard matrix, orthonormal.
fn hadamard() -> Mat {
    let mut m = [0.0; LINES * LINES];
    for i in 0..LINES {
        for j in 0..LINES {
            let bits = (i & j).count_ones();
            m[i * LINES + j] = if bits % 2 == 0 { 0.25 } else { -0.25 };
        }
    }
    m
}

/// The nearest orthogonal matrix to `a` (Newton–Schulz polar
/// iteration), then scaled so its largest singular value is ≤ 1.
fn orthogonalise(a: &Mat) -> Mat {
    let fro: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
    let mut x: Mat = core::array::from_fn(|k| a[k] / fro);
    let (mut xt, mut xtx, mut xxtx) = ([0.0; LINES * LINES], [0.0; LINES * LINES], [0.0; LINES * LINES]);
    for _ in 0..18 {
        transpose(&x, &mut xt);
        matmul(&xt, &x, &mut xtx);
        matmul(&x, &xtx, &mut xxtx);
        for k in 0..LINES * LINES {
            x[k] = 1.5 * x[k] - 0.5 * xxtx[k];
        }
    }
    // Guard: estimate the largest singular value; never exceed 1.
    let mut v = [1.0f32; LINES];
    let mut sigma = 1.0;
    for _ in 0..24 {
        let mut w = [0.0f32; LINES];
        for i in 0..LINES {
            for j in 0..LINES {
                w[i] += x[i * LINES + j] * v[j];
            }
        }
        let mut u = [0.0f32; LINES];
        for i in 0..LINES {
            for j in 0..LINES {
                u[j] += x[i * LINES + j] * w[i];
            }
        }
        let n = u.iter().map(|a| a * a).sum::<f32>().sqrt().max(1e-12);
        sigma = n.sqrt();
        for (a, b) in v.iter_mut().zip(&u) {
            *a = b / n;
        }
    }
    if sigma > 1.0 {
        for e in &mut x {
            *e /= sigma;
        }
    }
    x
}

pub(super) struct Room {
    sr: f32,
    lines: [DelayLine; LINES],
    lp: [f32; LINES],
    /// Each line's share of the span (from its node's radius): where it
    /// is heading, and where it is (gliding — a dragged node bends the
    /// room, never steps it).
    share_to: [f32; LINES],
    share: [f32; LINES],
    glide: f32,
    lens: [f32; LINES],
    phase: [f32; LINES],
    energy: [f32; LINES],
    energy_k: f32,
    /// The mix in use, and the one the drawing asks for (glided per block).
    mix: Mat,
    mix_to: Mat,
    /// The generated late decay: 1 / T60 at low frequencies, and each
    /// line's absorption pole (from `aim_late`, per block).
    /// The generated decay: each line's attenuation filter (fitted per
    /// block by `aim_late`), and whether it's in use.
    design: Design,
    eqs: [DecayEq; LINES],
    /// Each line's diffusers' mean group delay per octave (samples): the
    /// loop is that much longer there, so it needs that much more
    /// attenuation per pass (lossless allpasses still lengthen the loop
    /// — uncounted, the lows rang ~25% long).
    diff_delay: [[f32; OCTAVES]; LINES],
    refit: usize,
    /// Each line's allpass-interpolator state (its last output).
    interp: [f32; LINES],
    gen_on: bool,
    diffusers: [[Allpass; 2]; LINES],
}

/// Per-frame settings.
#[derive(Copy, Clone)]
pub(super) struct Shape {
    pub(super) size: f32,
    pub(super) decay_s: f32,
    pub(super) tone: f32,
    pub(super) hold: bool,
    /// 0 = the knobs' room … 1 = the generated room's decay.
    pub(super) gen: f32,
}

impl Room {
    pub(super) fn new() -> Self {
        Self {
            sr: 48_000.0,
            lines: core::array::from_fn(|_| DelayLine::new()),
            lp: [0.0; LINES],
            share_to: [0.6; LINES],
            share: [0.6; LINES],
            glide: 0.0,
            lens: [1.0; LINES],
            phase: core::array::from_fn(|i| i as f32 / LINES as f32),
            energy: [0.0; LINES],
            energy_k: 0.0,
            mix: hadamard(),
            mix_to: hadamard(),
            design: Design::new(48_000.0),
            interp: [0.0; LINES],
            refit: 0,
            diff_delay: [[0.0; OCTAVES]; LINES],
            eqs: core::array::from_fn(|_| DecayEq::new()),
            gen_on: false,
            diffusers: core::array::from_fn(|_| core::array::from_fn(|_| Allpass { line: DelayLine::new(), len: 1 })),
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let cap = ((SPAN_MS.1 * 1.08 + 2.0 * WANDER_MS) * 0.001 * sr) as usize + 8;
        for l in &mut self.lines {
            l.prepare(cap);
        }
        self.lp = [0.0; LINES];
        self.energy = [0.0; LINES];
        self.energy_k = 1.0 - (-1.0 / (0.08 * sr)).exp();
        self.glide = 1.0 - (-1.0 / (0.04 * sr)).exp();
        self.share = self.share_to;
        self.mix = self.mix_to;
        self.design = Design::new(sr);
        self.interp = [0.0; LINES];
        self.eqs = core::array::from_fn(|_| DecayEq::new());
        for (i, pair) in self.diffusers.iter_mut().enumerate() {
            for (j, ap) in pair.iter_mut().enumerate() {
                let (lo, hi) = DIFF_MS[j];
                // Spread across the range by a golden-ratio walk.
                let t = (i as f32 * 0.618_034 + j as f32 * 0.31).fract();
                ap.len = ((lo + (hi - lo) * t) * 0.001 * sr).round().max(1.0) as usize;
                ap.line.prepare(ap.len + 2);
            }
        }
        for i in 0..LINES {
            for (k, &c) in CENTRES.iter().enumerate() {
                // Mean over the octave (log-spaced points).
                let n = 32;
                self.diff_delay[i][k] = (0..n)
                    .map(|j| {
                        let hz = (c / 2f32.sqrt() * 2f32.powf(j as f32 / (n - 1) as f32)).min(0.45 * sr);
                        let w = TAU * hz / sr;
                        self.diffusers[i].iter().map(|ap| allpass_delay(ap.len as f32, DIFFUSE, w)).sum::<f32>()
                    })
                    .sum::<f32>()
                    / n as f32;
            }
        }
    }

    /// Re-shape the room from its nodes: `radius` (0 centre … 1 rim)
    /// sets each line's length; `closeness[i][j]` (0 far … 1 touching)
    /// sets how strongly two lines feed each other. Runs the polar
    /// projection (≈100 k flops): call per block, not per frame.
    pub(super) fn reshape(&mut self, radius: &[f32; LINES], dist: &[[f32; LINES]; LINES]) {
        for i in 0..LINES {
            self.share_to[i] = (MIN_SHARE + (1.0 - MIN_SHARE) * radius[i].clamp(0.0, 1.0)) * SPREAD[i];
        }
        let base = hadamard();
        let mut a = base;
        for i in 0..LINES {
            for j in 0..LINES {
                if i != j {
                    let d = dist[i][j] / REACH;
                    a[i * LINES + j] += COUPLING * 0.25 * (-d * d).exp();
                }
            }
        }
        self.mix_to = orthogonalise(&a);
        // Glide the mix a third of the way per block (a convex blend of
        // two norm-≤1 matrices stays norm-≤1: still stable).
        for (m, t) in self.mix.iter_mut().zip(&self.mix_to) {
            *m += (t - *m) * 0.33;
        }
    }

    /// Aim the generated decay: T60 at low and at high frequencies,
    /// seconds. Per block (the poles follow the lines' current lengths).
    /// Fit every line's attenuation filter: per octave, the knobs'
    /// response (DECAY `decay_s`, TONE's damping) blended by `gen` with
    /// the generated room's `t60`. Per block (the inputs glide upstream,
    /// so the filters never step).
    pub(super) fn aim_late(&mut self, t60: &[f32; OCTAVES], s: Shape) {
        let was = self.gen_on;
        self.gen_on = s.gen > 0.0;
        if !self.gen_on {
            return;
        }
        // Each line refits every REFIT-th block, staggered (rooms glide
        // slowly; a fit is ~1k filter evaluations) — all at once when
        // the generated path has just switched on.
        self.refit = self.refit.wrapping_add(1);
        let fc = DAMP_HZ.0 * (DAMP_HZ.1 / DAMP_HZ.0).powf(s.tone);
        let damp = (-TAU * fc.min(0.45 * self.sr) / self.sr).exp();
        for i in 0..LINES {
            if was && !(self.refit + i).is_multiple_of(REFIT) {
                continue;
            }
            let len = self.lens[i];
            let gen: [f32; OCTAVES] = if s.hold {
                [0.0; OCTAVES]
            } else {
                core::array::from_fn(|k| -60.0 * (len + self.diff_delay[i][k]) / (t60[k].max(0.05) * self.sr))
            };
            let knob_g = if s.hold { 0.0 } else { -60.0 * len / (s.decay_s * self.sr) };
            let target: [f32; OCTAVES] = core::array::from_fn(|k| {
                let knob = knob_g + onepole_db(damp, CENTRES[k], self.sr);
                knob + s.gen * (gen[k] - knob)
            });
            let eq = &mut self.eqs[i];
            eq.gain = self.design.fit(&target, &mut eq.filters);
        }
    }

    /// Settle instantly on the drawing (activation, presets).
    pub(super) fn settle(&mut self) {
        self.share = self.share_to;
        self.mix = self.mix_to;
    }

    /// One frame: the room hears `x` (L into the even lines, R into the
    /// odd) and answers with its [L, R].
    #[inline]
    pub(super) fn tick(&mut self, x: [f32; 2], s: Shape) -> [f32; 2] {
        let span = (SPAN_MS.0 + (SPAN_MS.1 - SPAN_MS.0) * s.size) * 0.001 * self.sr;
        let fc = DAMP_HZ.0 * (DAMP_HZ.1 / DAMP_HZ.0).powf(s.tone);
        let damp = (-TAU * fc.min(0.45 * self.sr) / self.sr).exp();
        let wander = WANDER_MS * 0.001 * self.sr;
        let mut v = [0.0f32; LINES];
        for i in 0..LINES {
            self.share[i] += (self.share_to[i] - self.share[i]) * self.glide;
            self.phase[i] = (self.phase[i] + (0.07 + 0.023 * i as f32) / self.sr).fract();
            let len = (self.share[i] * span).max(8.0) + wander * (1.0 + (TAU * self.phase[i]).sin());
            self.lens[i] = len;
            // ALLPASS interpolation: unity gain at every frequency. Any
            // interpolating read (linear, even cubic) is a slight lowpass
            // at fractional delays, compounded over hundreds of passes —
            // measured: highs at 0.5–0.9 and lows at 1.2–2.5 of their
            // white share, flat with integer delays. The decay filters
            // alone now shape the tail's spectrum.
            let m = (len - 0.5).floor().max(1.0);
            let frac = len - m; // 0.5 … 1.5
            let eta = (1.0 - frac) / (1.0 + frac);
            let (a, b) = (self.lines[i].read_int(m as usize), self.lines[i].read_int(m as usize + 1));
            self.interp[i] = eta * (a - self.interp[i]) + b;
            let mut y = self.interp[i];
            for ap in &mut self.diffusers[i] {
                y = ap.tick(y, DIFFUSE);
            }
            if self.gen_on {
                // (lp doubles as the lines' outputs for plasticity.)
                self.lp[i] = self.eqs[i].shape(y);
            } else {
                self.lp[i] = y + damp * (self.lp[i] - y);
            }
            v[i] = self.lp[i];
            self.energy[i] += (v[i].abs() - self.energy[i]) * self.energy_k;
        }
        let (mut l, mut r) = (0.0, 0.0);
        for i in 0..LINES {
            let sign = if (i / 2) % 2 == 0 { 1.0 } else { -1.0 };
            if i % 2 == 0 {
                l += sign * v[i];
            } else {
                r += sign * v[i];
            }
        }
        for i in 0..LINES {
            let row = &self.mix[i * LINES..(i + 1) * LINES];
            let mixed: f32 = row.iter().zip(&v).map(|(m, x)| m * x).sum();
            let g = if s.hold {
                1.0
            } else if self.gen_on {
                self.eqs[i].gain
            } else {
                10f32.powf(-3.0 * self.lens[i] / (s.decay_s * self.sr))
            };
            let sign = if i % 4 < 2 { 1.0 } else { -1.0 };
            let w = ceiling(g * mixed + sign * INPUT_GAIN * x[i % 2]);
            self.lines[i].write(w);
            self.lines[i].advance();
        }
        [l * 0.35, r * 0.35]
    }

    /// The lines' current outputs (for the plasticity's correlations).
    pub(super) fn outputs(&self) -> &[f32; LINES] {
        &self.lp
    }

    pub(super) fn energy(&self, i: usize) -> f32 {
        self.energy[i]
    }

    #[cfg(test)]
    pub(super) fn mix(&self) -> &Mat {
        &self.mix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn singulars_ok(m: &Mat) -> bool {
        // ‖M v‖ ≤ ‖v‖ for a handful of probe vectors.
        (0..32u32).all(|s| {
            let v: [f32; LINES] = core::array::from_fn(|i| (((s * 31 + i as u32 * 17) % 13) as f32 - 6.0) / 6.0);
            let n0: f32 = v.iter().map(|a| a * a).sum::<f32>().sqrt();
            let mut w = [0.0f32; LINES];
            for i in 0..LINES {
                for j in 0..LINES {
                    w[i] += m[i * LINES + j] * v[j];
                }
            }
            let n1: f32 = w.iter().map(|a| a * a).sum::<f32>().sqrt();
            n1 <= n0 * 1.0005
        })
    }

    #[test]
    fn any_drawing_is_stable() {
        let mut room = Room::new();
        // Every node in one spot: the worst clustering.
        let all_close = [[0.0f32; LINES]; LINES];
        room.reshape(&[0.5; LINES], &all_close);
        assert!(singulars_ok(room.mix()), "a glide toward a clustered drawing gained energy");
        room.settle();
        assert!(singulars_ok(room.mix()), "a fully clustered drawing gained energy");
        // Spread far apart: the plain Hadamard room.
        let far = [[10.0f32; LINES]; LINES];
        room.reshape(&[0.5; LINES], &far);
        room.settle();
        assert!(singulars_ok(room.mix()));
        // And it is (nearly) orthogonal: no energy LOST in the mix.
        let v = [0.25f32; LINES];
        let mut w = [0.0f32; LINES];
        for i in 0..LINES {
            for j in 0..LINES {
                w[i] += room.mix()[i * LINES + j] * v[j];
            }
        }
        let (n0, n1): (f32, f32) = (v.iter().map(|a| a * a).sum::<f32>().sqrt(), w.iter().map(|a| a * a).sum::<f32>().sqrt());
        assert!((n1 / n0 - 1.0).abs() < 0.01, "the far drawing lost energy: {}", n1 / n0);
    }
}
