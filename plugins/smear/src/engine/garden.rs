//! The garden — one lane's resonator lattice: 84 chromatic strings,
//! their coefficients, coupling state, and tuning drift. This is a
//! LEAF (a dense cross-coupled core);
//! the kernel's job is the plumbing around it.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use stave_dsp::Xorshift32;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Resonators per channel — a chromatic lattice. 84 semitones = 7 octaves
/// from A1 (≈55 Hz) up to ≈7 kHz, covering the musically useful range.
pub(super) const N_RES: usize = 84;
/// Lowest resonator pitch (A1).
pub(super) const F_LOW: f32 = 55.0;
/// Lane capacity: 5.1. Active lanes follow the negotiated
/// buffer at runtime — a loop bound, not audio-path dispatch, so the
/// monomorphized-variants concern does not apply; idle gardens
/// cost memory only (~4 KB each), never samples.
pub(super) const MAX_CHANS: usize = 6;
pub(super) const TWO_PI: f32 = core::f32::consts::TAU;
/// Coefficients (and drift) update once per this many samples — cos() per
/// resonator per sample would be wasteful and they need not be sample-exact.
pub(super) const CTRL_INTERVAL: usize = 32;
pub(super) const DENORM: f32 = 1.0e-25;

pub(super) struct ChanState {
    pub(super) base_freq: Vec<f32>, // lattice pitch of each resonator (Hz)
    pub(super) y1: Vec<f32>,
    pub(super) y2: Vec<f32>,
    pub(super) out_prev: Vec<f32>, // last sample's output (for inter-resonator coupling)
    pub(super) out_cur: Vec<f32>,
    pub(super) a1: Vec<f32>, // resonator coefficients (updated per control block)
    pub(super) a2: Vec<f32>,
    pub(super) b0: Vec<f32>,
    pub(super) detune: Vec<f32>,     // current detune multiplier (drifts)
    pub(super) detune_tgt: Vec<f32>, // drift target
    pub(super) tone_state: f32,
    pub(super) rng: Xorshift32,
}

/// Uniform in [−1, 1] over the full 32-bit draw (the garden's
/// historical mapping; stereo is bit-pinned to it).
#[inline]
fn rand_bi(rng: &mut Xorshift32) -> f32 {
    (rng.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl ChanState {
    pub(super) fn new(seed: u32) -> Self {
        let base_freq: Vec<f32> = (0..N_RES)
            .map(|i| F_LOW * 2f32.powf(i as f32 / 12.0))
            .collect();
        let mut s = Self {
            base_freq,
            y1: vec![0.0; N_RES],
            y2: vec![0.0; N_RES],
            out_prev: vec![0.0; N_RES],
            out_cur: vec![0.0; N_RES],
            a1: vec![0.0; N_RES],
            a2: vec![0.0; N_RES],
            b0: vec![0.0; N_RES],
            detune: vec![1.0; N_RES],
            detune_tgt: vec![1.0; N_RES],
            tone_state: 0.0,
            rng: Xorshift32::new(seed | 1),
        };
        s.reset();
        s
    }

    pub(super) fn reset(&mut self) {
        self.y1.fill(0.0);
        self.y2.fill(0.0);
        self.out_prev.fill(0.0);
        self.out_cur.fill(0.0);
        self.detune.fill(1.0);
        self.detune_tgt.fill(1.0);
        self.tone_state = 0.0;
    }

    /// Recompute resonator coefficients from the current decay (→ ring
    /// time) and detune. Called once per control block.
    pub(super) fn update_coeffs(&mut self, sr: f32, decay: f32, motion: f32, flux: f32) {
        // Ring time → pole radius r. Interpolate (1−r) geometrically so the
        // knob spans ~14 ms (plucky) to several seconds (drone). r must sit
        // very close to 1 for musical sustain.
        let one_minus_r = (0.0015f32).powf(1.0 - decay) * (3.0e-6f32).powf(decay);
        let r = (1.0 - one_minus_r).clamp(0.5, 0.999995);
        let b0v = 1.0 - r; // normalises resonance peak to ~unity

        // Drift the detune targets occasionally (random walk), then ease
        // each detune toward its target. FLUX = how often the target moves,
        // MOTION = how far it can stray (chorus depth, ≤ ~1.2%).
        let drift = motion * 0.012;
        let nyq = sr * 0.47;
        for i in 0..N_RES {
            if flux > 0.0 && (self.rng.next_u32() & 0x3f) < (flux * 40.0) as u32 {
                self.detune_tgt[i] = 1.0 + rand_bi(&mut self.rng) * drift;
            }
            self.detune[i] += 0.05 * (self.detune_tgt[i] - self.detune[i]);
            let f = (self.base_freq[i] * self.detune[i]).min(nyq);
            let w = TWO_PI * f / sr;
            self.a1[i] = 2.0 * r * w.cos();
            self.a2[i] = r * r;
            self.b0[i] = b0v;
        }
    }
}
