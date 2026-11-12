//! THE DIFFUSER — the room's first surfaces: a series cascade of
//! four Schroeder allpasses per channel that smears an arriving
//! wavefront into a dense cloud before it reaches the tank. An
//! allpass is transparent to ENERGY (|H(ω)| = 1) — it reorders
//! time, never tone — so the cascade thickens echo density at zero
//! spectral cost. The L and R cascades are detuned ~7% so the two
//! ears never receive the same cloud (decorrelation starts before
//! the tank does). Lengths scale with √size and ride the same
//! slew-capped glide as the tank's read heads.

use stave_dsp::DelayLine;

const STAGES: usize = 4;
/// Stage lengths at size 1.0, ms — mutually non-integer-ratio.
const BASE_MS: [f32; STAGES] = [4.7, 8.3, 13.1, 22.9];
/// Allpass coefficient at the DIFFUSE default 0.7 (0.62, the
/// pre-knob voicing preserved exactly).
const G_SCALE: f32 = 0.885_714_3;
/// Read-head slew cap (shared doctrine with the tank).
const MAX_SLEW: f32 = 0.02;

struct Stage {
    delay: DelayLine,
    len: f32,
    step: f32,
}

pub(super) struct Diffuser {
    stages: [Stage; STAGES],
    g: f32,
    /// L = 1.0, R ≈ 1.07 — the inter-ear detune.
    scale: f32,
    sr: f32,
}

impl Diffuser {
    pub fn new(scale: f32) -> Self {
        Self {
            stages: [
                Stage {
                    delay: DelayLine::new(),
                    len: 100.0,
                    step: 0.0,
                },
                Stage {
                    delay: DelayLine::new(),
                    len: 100.0,
                    step: 0.0,
                },
                Stage {
                    delay: DelayLine::new(),
                    len: 100.0,
                    step: 0.0,
                },
                Stage {
                    delay: DelayLine::new(),
                    len: 100.0,
                    step: 0.0,
                },
            ],
            scale,
            g: 0.62,
            sr: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        for (i, st) in self.stages.iter_mut().enumerate() {
            // √3 ≈ 1.74 covers size 3.0; headroom for the glide.
            let cap = (BASE_MS[i] * 0.001 * sr * self.scale * 1.8) as usize + 32;
            st.delay.prepare(cap);
            st.len = BASE_MS[i] * 0.001 * sr * self.scale;
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for st in self.stages.iter_mut() {
            st.delay.reset();
            st.step = 0.0;
        }
    }

    /// Block-rate: aim the stage lengths at √size; the DIFFUSE
    /// dial is the coefficient (0 = the cascade goes transparent —
    /// discrete echoes as a character).
    pub fn tune(&mut self, size: f32, diffuse: f32, frames: usize) {
        // Capped in the dense region — allpass density FALLS as
        // g→1 (the direct term dominates; measured on the plate).
        self.g = (G_SCALE * diffuse).clamp(0.0, 0.68);
        let n = frames.max(1) as f32;
        let s = size.max(0.05).sqrt();
        for (i, st) in self.stages.iter_mut().enumerate() {
            let target = BASE_MS[i] * 0.001 * self.sr * self.scale * s;
            let slew = MAX_SLEW * n;
            st.step = (target - st.len).clamp(-slew, slew) / n;
        }
    }

    /// One sample through the cascade.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let mut v = x;
        for st in self.stages.iter_mut() {
            // Linear: near-fixed lengths (√size glide only), and the
            // allpass recirculation tolerates a constant sub-sample
            // tilt (fixed fraction = fixed comb, not accumulating).
            let d = st.delay.read(st.len);
            st.len += st.step;
            let y = d - self.g * v;
            st.delay.write(v + self.g * y);
            st.delay.advance();
            v = y;
        }
        v
    }
}
