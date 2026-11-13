//! The phase vocoder's frame engine: analysis, phase reconstruction
//! (Průša's phase-gradient heap integration, or Laroche & Dolson's
//! identity phase locking) and synthesis of one frame at a time, with
//! one frame of look-ahead. The pitch shifter and the offline time
//! stretch drive it with their own hops.

use super::heap::Heap;
use crate::primitives::spectral::{Complex, Fft, Window};
use core::f32::consts::{PI, TAU};

/// How the phase vocoder builds the synthesis phase.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Reconstruction {
    /// Phase-gradient heap integration (Průša & Holighaus, *Phase
    /// vocoder done right*, EUSIPCO 2017, on the real-time PGHI of
    /// Průša & Søndergaard, DAFx 2016): both partial derivatives of the
    /// analysis phase, integrated outward from the strongest
    /// coefficients, so sinusoids, chirps and impulses all keep their
    /// shape. No peak picking, no transient detection. The default.
    #[default]
    Prusa,
    /// Identity phase locking (Laroche & Dolson, *Improved phase vocoder
    /// time-scale modification of audio*, IEEE TSAP 1999): each
    /// spectral peak's phase advanced by its instantaneous frequency,
    /// the bins around it locked to it with the analysis phase
    /// differences. Cheaper (no heap); smears transients and chirps.
    LarocheDolson,
}

/// `x` wrapped into `[−π, π]`.
#[inline]
pub(super) fn princarg(x: f32) -> f32 {
    x - TAU * (x / TAU).round()
}

/// Bins below this fraction of the louder frame's peak get a random
/// phase rather than an integrated one (Průša's `tol`; 10⁻⁵ is −100 dB,
/// near `f32`'s floor for a frame's magnitudes).
const TOLERANCE: f32 = 1e-5;

/// One analysed frame: its magnitudes, its phases in the centred
/// convention (the window's middle is time zero, so a stationary
/// sinusoid's phase does not alternate across bins), and the hop from
/// the frame before it.
#[derive(Clone, Debug)]
struct Analysed {
    magnitude: Vec<f32>,
    phase: Vec<f32>,
    hop: usize,
}

/// The frame engine: plan once, then [`analyse`](Self::analyse) a frame
/// per hop and [`synthesise`](Self::synthesise) the frame before it.
/// Never allocates after [`new`](Self::new).
#[derive(Clone, Debug)]
pub(super) struct Vocoder {
    size: usize,
    bins: usize,
    reconstruction: Reconstruction,
    fft: Fft<f32>,
    window: Vec<f32>,
    frame: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    /// Frames `n − 1`, `n`, `n + 1` by `index % 3`.
    frames: [Analysed; 3],
    /// Frames analysed so far.
    analysed: usize,
    /// Frames synthesised so far.
    synthesised: usize,
    /// The synthesis phase of the last synthesised frame.
    synthesis: Vec<f32>,
    /// The time derivative of the analysis phase at the last
    /// synthesised frame, radians per sample.
    dt_last: Vec<f32>,
    dt: Vec<f32>,
    df: Vec<f32>,
    phase: Vec<f32>,
    done: Vec<bool>,
    heap: Heap,
    rng: u32,
    /// The synthesised frame, windowed, `1/size` folded in.
    out: Vec<f32>,
}

impl Vocoder {
    /// An engine for frames of `size` samples (a power of two), Hann
    /// windowed at analysis and synthesis.
    pub(super) fn new(size: usize, reconstruction: Reconstruction) -> Self {
        let bins = size / 2 + 1;
        let analysed = || Analysed {
            magnitude: vec![0.0; bins],
            phase: vec![0.0; bins],
            hop: 1,
        };
        Self {
            size,
            bins,
            reconstruction,
            fft: Fft::new(size),
            window: Window::Hann.periodic(size).iter().map(|&w| w as f32).collect(),
            frame: vec![0.0; size],
            spectrum: vec![Complex::default(); bins],
            frames: [analysed(), analysed(), analysed()],
            analysed: 0,
            synthesised: 0,
            synthesis: vec![0.0; bins],
            dt_last: vec![0.0; bins],
            dt: vec![0.0; bins],
            df: vec![0.0; bins],
            phase: vec![0.0; bins],
            done: vec![false; bins],
            heap: Heap::with_capacity(2 * bins),
            rng: 0x2545_f491,
            out: vec![0.0; size],
        }
    }

    /// The analysis window (periodic Hann).
    pub(super) fn window(&self) -> &[f32] {
        &self.window
    }

    pub(super) fn reset(&mut self) {
        self.analysed = 0;
        self.synthesised = 0;
        self.synthesis.fill(0.0);
        self.dt_last.fill(0.0);
        self.rng = 0x2545_f491;
    }

    /// Frames analysed and not yet synthesised.
    pub(super) fn waiting(&self) -> usize {
        self.analysed - self.synthesised
    }

    /// Analyses one frame: `fill` writes its `size` samples, oldest
    /// first; `hop` is how far it starts after the previous frame.
    pub(super) fn analyse(&mut self, fill: impl FnOnce(&mut [f32]), hop: usize) {
        fill(&mut self.frame);
        for (x, &w) in self.frame.iter_mut().zip(&self.window) {
            *x *= w;
        }
        self.fft.forward_real(&self.frame, &mut self.spectrum);
        let slot = &mut self.frames[self.analysed % 3];
        slot.hop = hop.max(1);
        for (m, z) in self.spectrum.iter().enumerate() {
            slot.magnitude[m] = z.norm();
            // Centred: multiply by e^{iπm}, the window's middle as zero.
            let odd = if m % 2 == 1 { PI } else { 0.0 };
            slot.phase[m] = princarg(z.arg() + odd);
        }
        self.analysed += 1;
    }

    /// Synthesises the oldest waiting frame (needs the frame after it
    /// analysed, or `last` for the final one), placed `synthesis_hop`
    /// samples after the previous synthesised frame, with bins from
    /// `cutoff` up silenced. The windowed frame is
    /// [`output`](Self::output), to overlap-add with weights
    /// [`window`](Self::window)² for normalisation.
    pub(super) fn synthesise(&mut self, synthesis_hop: usize, cutoff: usize, last: bool) {
        let n = self.synthesised;
        debug_assert!(n < self.analysed && (last || n + 1 < self.analysed));
        let bins = self.bins;
        let (prev, cur, next) = ((n + 2) % 3, n % 3, (n + 1) % 3);
        let has_prev = n > 0;
        let has_next = n + 1 < self.analysed;
        // Time derivative, centred where both neighbours exist:
        // heterodyned differences (PVDR eqs. 13–15).
        for m in 0..bins {
            let wm = TAU * m as f32 / self.size as f32;
            let p = self.frames[cur].phase[m];
            let diff = |a: f32, b: f32, hop: usize| {
                princarg(b - a - wm * hop as f32) / hop as f32 + wm
            };
            let back = has_prev.then(|| {
                let f = &self.frames[prev];
                diff(f.phase[m], p, self.frames[cur].hop)
            });
            let fwd = has_next.then(|| {
                let f = &self.frames[next];
                diff(p, f.phase[m], f.hop)
            });
            self.dt[m] = match (back, fwd) {
                (Some(b), Some(f)) => 0.5 * (b + f),
                (Some(d), None) | (None, Some(d)) => d,
                (None, None) => wm,
            };
        }
        let alpha = synthesis_hop as f32 / self.frames[cur].hop as f32;
        let hop_s = synthesis_hop as f32;
        if !has_prev {
            self.phase.copy_from_slice(&self.frames[cur].phase);
        } else {
            match self.reconstruction {
                Reconstruction::Prusa => self.heap_integrate(prev, cur, hop_s, alpha),
                Reconstruction::LarocheDolson => self.identity_lock(cur, hop_s),
            }
        }
        // Back to the frame-start convention, inverse transform, window.
        let mag = &self.frames[cur].magnitude;
        for m in 0..bins {
            self.spectrum[m] = if m < cutoff {
                let odd = if m % 2 == 1 { PI } else { 0.0 };
                let (s, c) = (self.phase[m] - odd).sin_cos();
                Complex::new(mag[m] * c, mag[m] * s)
            } else {
                Complex::default()
            };
        }
        self.fft.inverse_real(&mut self.spectrum, &mut self.out);
        let scale = 1.0 / self.size as f32;
        for (y, &w) in self.out.iter_mut().zip(&self.window) {
            *y *= w * scale;
        }
        for m in 0..bins {
            self.synthesis[m] = princarg(self.phase[m]);
        }
        core::mem::swap(&mut self.dt_last, &mut self.dt);
        self.synthesised += 1;
    }

    /// The last synthesised frame, windowed.
    pub(super) fn output(&self) -> &[f32] {
        &self.out
    }

    /// A uniform phase in `[−π, π)` for a coefficient below tolerance.
    fn random_phase(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * TAU - PI
    }

    /// Phase-gradient heap integration of the current frame (PVDR
    /// Algorithm 1): from the previous frame's strongest coefficients
    /// along time (trapezoidal, the time derivative), then within the
    /// frame along frequency (the frequency derivative scaled by the
    /// stretch `alpha`), always from the strongest coefficient known.
    fn heap_integrate(&mut self, prev: usize, cur: usize, hop_s: f32, alpha: f32) {
        let bins = self.bins;
        // Frequency derivative: centred phase differences across bins
        // (PVDR eqs. 16–18), per bin.
        {
            let p = &self.frames[cur].phase;
            for m in 0..bins {
                let back = (m > 0).then(|| princarg(p[m] - p[m - 1]));
                let fwd = (m + 1 < bins).then(|| princarg(p[m + 1] - p[m]));
                self.df[m] = match (back, fwd) {
                    (Some(b), Some(f)) => 0.5 * (b + f),
                    (Some(d), None) | (None, Some(d)) => d,
                    (None, None) => 0.0,
                };
            }
        }
        let peak = self.frames[cur]
            .magnitude
            .iter()
            .chain(&self.frames[prev].magnitude)
            .fold(0.0f32, |a, &b| a.max(b));
        let floor = TOLERANCE * peak;
        let mut remaining = 0;
        for m in 0..bins {
            let significant = self.frames[cur].magnitude[m] > floor;
            self.done[m] = !significant;
            if significant {
                remaining += 1;
            } else {
                self.phase[m] = self.random_phase();
            }
        }
        self.heap.clear();
        for m in 0..bins {
            if !self.done[m] {
                self.heap.push(self.frames[prev].magnitude[m], m as u32);
            }
        }
        let bins32 = bins as u32;
        while remaining > 0 {
            let Some(top) = self.heap.pop() else {
                // Nothing known to spread from: start at the strongest
                // coefficient left, from its analysis phase.
                let m = (0..bins)
                    .filter(|&m| !self.done[m])
                    .max_by(|&a, &b| {
                        self.frames[cur].magnitude[a].total_cmp(&self.frames[cur].magnitude[b])
                    })
                    .unwrap_or(0);
                self.phase[m] = self.frames[cur].phase[m];
                self.done[m] = true;
                remaining -= 1;
                self.heap.push(self.frames[cur].magnitude[m], bins32 + m as u32);
                continue;
            };
            if top < bins32 {
                let m = top as usize;
                if !self.done[m] {
                    self.phase[m] =
                        self.synthesis[m] + 0.5 * hop_s * (self.dt_last[m] + self.dt[m]);
                    self.done[m] = true;
                    remaining -= 1;
                    self.heap.push(self.frames[cur].magnitude[m], bins32 + top);
                }
            } else {
                let m = (top - bins32) as usize;
                for (k, sign) in [(m + 1, 1.0f32), (m.wrapping_sub(1), -1.0)] {
                    if k < bins && !self.done[k] {
                        self.phase[k] =
                            self.phase[m] + sign * 0.5 * alpha * (self.df[m] + self.df[k]);
                        self.done[k] = true;
                        remaining -= 1;
                        self.heap.push(self.frames[cur].magnitude[k], bins32 + k as u32);
                    }
                }
            }
        }
    }

    /// Identity phase locking: peaks (larger than two neighbours each
    /// side) advance by their instantaneous frequency; every other bin
    /// keeps its analysis phase offset from the peak whose region it is
    /// in (regions split at the weakest bin between peaks).
    fn identity_lock(&mut self, cur: usize, hop_s: f32) {
        let bins = self.bins;
        let mag = &self.frames[cur].magnitude;
        let pa = &self.frames[cur].phase;
        let is_peak = |m: usize| {
            let v = mag[m];
            (m < 1 || v > mag[m - 1])
                && (m < 2 || v > mag[m - 2])
                && (m + 1 >= bins || v > mag[m + 1])
                && (m + 2 >= bins || v > mag[m + 2])
        };
        let mut start = 0;
        let mut last_peak: Option<usize> = None;
        let mut m = 0;
        while m <= bins {
            let peak = (m < bins && is_peak(m)).then_some(m);
            if m == bins || peak.is_some() {
                // Close the region of the previous peak: up to the
                // weakest bin between it and this one.
                let end = match (last_peak, peak) {
                    (Some(a), Some(b)) => (a..=b)
                        .min_by(|&x, &y| mag[x].total_cmp(&mag[y]))
                        .unwrap_or(b),
                    _ => m,
                };
                if let Some(p) = last_peak {
                    let locked = self.synthesis[p] + hop_s * self.dt[p];
                    for k in start..end.max(start) {
                        self.phase[k] = locked + pa[k] - pa[p];
                    }
                    start = end;
                } else if peak.is_none() {
                    // No peak at all: the classic vocoder, bin by bin.
                    for k in 0..bins {
                        self.phase[k] = self.synthesis[k] + hop_s * self.dt[k];
                    }
                }
                last_peak = peak;
            }
            m += 1;
        }
    }
}
