//! `Tank` — the reverb: two allpass diffusion chains feeding an
//! eight-line feedback delay network (Hadamard mixing, per-line
//! damping, slow per-line wobble so the tail blooms instead of
//! ringing metallic). Line lengths are in core TICKS, so when the
//! clock stretches the space stretches with it — program IV's TIME
//! is exactly that.

use stave_dsp::DelayLine;

use super::path::flush;

const LINES: usize = 8;
/// Prime lengths, 35–87 ms at the nominal 32 kHz tick.
const LENGTHS: [f32; LINES] = [1123.0, 1361.0, 1597.0, 1801.0, 2039.0, 2251.0, 2503.0, 2777.0];
/// Per-line wobble rates (Hz at the nominal tick) — incommensurate.
const WOBBLE_HZ: [f32; LINES] = [0.11, 0.17, 0.23, 0.29, 0.37, 0.43, 0.53, 0.61];
const WOBBLE_TICKS: f32 = 7.0;
const DIFFUSE_L: [usize; 4] = [142, 107, 379, 277];
const DIFFUSE_R: [usize; 4] = [151, 113, 367, 293];
const DIFFUSE_G: f32 = 0.62;
/// Which lines each ear injects into / reads from, and with what sign.
const SIGN_L: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0];
const SIGN_R: [f32; LINES] = [1.0, 1.0, -1.0, -1.0, -1.0, 1.0, 1.0, -1.0];

struct Allpass {
    line: DelayLine,
    len: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let d = self.line.read_int(self.len);
        let v = flush(x + DIFFUSE_G * d);
        self.line.write(v);
        self.line.advance();
        d - DIFFUSE_G * v
    }
}

pub(super) struct Tank {
    lines: [DelayLine; LINES],
    damp: [f32; LINES],
    gain: [f32; LINES],
    damp_a: f32,
    /// Input scale that energy-normalizes the decay: a long tail rings
    /// LONGER, not louder (√(1 − g²) of the mean loop gain).
    in_gain: f32,
    /// Quadrature oscillators for the wobble (cos, sin) + rotation.
    osc: [(f32, f32); LINES],
    rot: [(f32, f32); LINES],
    diffuse: [[Allpass; 4]; 2],
}

impl Tank {
    pub(super) fn new() -> Self {
        let ap = |len: usize| Allpass { line: DelayLine::new(), len };
        Self {
            lines: core::array::from_fn(|_| DelayLine::new()),
            damp: [0.0; LINES],
            gain: [0.0; LINES],
            damp_a: 1.0,
            in_gain: 1.0,
            osc: [(1.0, 0.0); LINES],
            rot: [(1.0, 0.0); LINES],
            diffuse: [DIFFUSE_L.map(ap), DIFFUSE_R.map(ap)],
        }
    }

    /// Allocate at the nominal tick rate `fs`.
    pub(super) fn prepare(&mut self, fs: f32) {
        for (i, l) in self.lines.iter_mut().enumerate() {
            l.prepare((LENGTHS[i] + WOBBLE_TICKS) as usize + 8);
        }
        for chain in &mut self.diffuse {
            for ap in chain.iter_mut() {
                ap.line.prepare(ap.len + 2);
            }
        }
        for i in 0..LINES {
            let w = 2.0 * core::f32::consts::PI * WOBBLE_HZ[i] / fs;
            self.rot[i] = (w.cos(), w.sin());
            let ph = i as f32 * 0.7;
            self.osc[i] = (ph.cos(), ph.sin());
        }
        self.reset();
    }

    pub(super) fn reset(&mut self) {
        for l in &mut self.lines {
            l.reset();
        }
        for chain in &mut self.diffuse {
            for ap in chain.iter_mut() {
                ap.line.reset();
            }
        }
        self.damp = [0.0; LINES];
    }

    /// Decay as RT60 in seconds (at the nominal tick), damping as a
    /// one-pole corner in Hz. Block rate.
    pub(super) fn voice(&mut self, fs: f32, rt60: f32, damp_hz: f32) {
        for i in 0..LINES {
            self.gain[i] = 10f32.powf(-3.0 * LENGTHS[i] / (rt60.max(0.05) * fs));
        }
        self.damp_a = 1.0 - (-2.0 * core::f32::consts::PI * damp_hz / fs).exp();
        let mean = self.gain.iter().sum::<f32>() / LINES as f32;
        self.in_gain = (1.0 - mean * mean).sqrt();
    }

    #[inline]
    pub(super) fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut dl = x[0] * self.in_gain;
        let mut dr = x[1] * self.in_gain;
        for ap in self.diffuse[0].iter_mut() {
            dl = ap.process(dl);
        }
        for ap in self.diffuse[1].iter_mut() {
            dr = ap.process(dr);
        }

        let mut v = [0.0f32; LINES];
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for i in 0..LINES {
            let (c, s) = self.osc[i];
            let (rc, rs) = self.rot[i];
            self.osc[i] = (c * rc - s * rs, c * rs + s * rc);
            let y = self.lines[i].read(LENGTHS[i] + WOBBLE_TICKS * s);
            self.damp[i] = flush(self.damp[i] + self.damp_a * (y - self.damp[i]));
            v[i] = self.damp[i] * self.gain[i];
            out_l += SIGN_L[i] * y;
            out_r += SIGN_R[i] * y;
        }
        // Renormalize the oscillators once a tick (cheap, drift-free).
        for o in &mut self.osc {
            let k = 1.5 - 0.5 * (o.0 * o.0 + o.1 * o.1);
            *o = (o.0 * k, o.1 * k);
        }
        hadamard(&mut v);
        let half = LINES / 2;
        for i in 0..LINES {
            let inj = if i < half { SIGN_L[i] * dl } else { SIGN_R[i] * dr };
            self.lines[i].write(flush(v[i] + inj));
            self.lines[i].advance();
        }
        [out_l * 0.3, out_r * 0.3]
    }
}

/// Unitary 8-point Hadamard (butterflies, scaled by 1/√8).
#[inline]
fn hadamard(v: &mut [f32; LINES]) {
    let mut h = 1;
    while h < LINES {
        let mut i = 0;
        while i < LINES {
            for j in i..i + h {
                let (a, b) = (v[j], v[j + h]);
                v[j] = a + b;
                v[j + h] = a - b;
            }
            i += 2 * h;
        }
        h *= 2;
    }
    let k = 1.0 / (LINES as f32).sqrt();
    for x in v.iter_mut() {
        *x *= k;
    }
}
