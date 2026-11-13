//! The play head's losses as an FIR designed from their formula.

use super::PlayHead;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::spectral::{Complex, Fft};

/// The filter's span: 10.7 ms each side of its centre at every rate,
/// enough for the thickness loss's cusp at DC (the response within
/// 0.35 dB of the formula from 20 Hz at 3.75 ips; the error halves with
/// each doubling of speed).
const HALF_SPAN_SECONDS: f64 = 0.0107;
/// The design grid, in multiples of the filter length.
const OVERSAMPLE: usize = 8;
/// The magnitude floor of the minimum-phase design's logarithm (−140 dB).
const FLOOR: f64 = 1e-7;

/// How the loss filter's phase is chosen.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Zero phase about the filter's centre, delayed by half its length
    /// (10.7 ms): the head's spatial response is symmetric, so this is
    /// the physical one.
    #[default]
    Linear,
    /// The minimum-phase filter with the same magnitude: no latency.
    Minimum,
}

/// The play head reading the tape: spacing, thickness and gap losses at
/// the tape's speed.
///
/// # Models
/// The playback losses of a magnetic tape (Chowdhury, DAFx-19, eq. 13,
/// after Kadis and Bertram), as a function of wave number `k = 2π·f/v`:
/// spacing `e^(−k·d)`, thickness `(1 − e^(−k·δ))/(k·δ)` and gap
/// `sin(k·g/2)/(k·g/2)`, at head spacing `d`, tape thickness `δ`, play
/// gap `g` and tape speed `v`. The gap's nulls (`f = n·v/g`) and the
/// speed dependence follow.
///
/// # Derivation
/// An FIR by frequency sampling, as the paper does: the formula on a
/// grid 8× the filter's length, inverse FFT, a Blackman window over
/// 21.4 ms (linear phase), or the real cepstrum's minimum-phase filter
/// of the same magnitude under the window's decaying half. Redesigned
/// when the speed or a head dimension changes, into buffers sized at
/// `prepare`: no allocation after it.
///
/// # Verified
/// `crates/stave-test/tests/tape_matches_reference.rs::loss_filter_matches_the_gap_loss_formula_at_every_speed`
/// (within 0.5 dB of eq. 13 from 20 Hz to 20 kHz wherever the formula
/// is above −60 dB, at 3.75, 7.5, 15 and 30 ips, linear and minimum
/// phase) and
/// `crates/stave-test/tests/tape_matches_reference.rs::loss_filter_matches_chowdhury_fig_3`.
///
/// # Cost
/// Datasheet `"Playback losses (7.5 ips)"`: a 1029-tap FIR at 48 kHz.
///
/// # Aliasing
/// None: linear.
///
/// # Valid ranges
/// Any speed and head geometry; the response is held from 20 Hz.
#[derive(Clone, Debug)]
pub struct Playback {
    head: PlayHead,
    thickness: f64,
    speed: f64,
    phase: Phase,
    rate: f64,
    len: usize,
    /// The taps, reversed (oldest sample first).
    taps: Vec<f64>,
    history: Vec<f64>,
    pos: usize,
    fft: Fft<f64>,
    work: Vec<Complex<f64>>,
}

impl Playback {
    /// Prepared at 48 kHz.
    pub fn new(head: PlayHead, thickness: f64, speed: f64, phase: Phase) -> Self {
        let mut p = Self {
            head,
            thickness,
            speed,
            phase,
            rate: 0.0,
            len: 0,
            taps: Vec::new(),
            history: Vec::new(),
            pos: 0,
            fft: Fft::new(2),
            work: Vec::new(),
        };
        p.prepare(48_000.0);
        p
    }

    /// The loss at `hz` (eq. 13), as a signed gain: the gap's sinc goes
    /// negative past its first null.
    pub fn loss(&self, hz: f64) -> f64 {
        loss(self.head, self.thickness, self.speed, hz)
    }

    /// Tape speed, metres per second. Realtime: a redesign of
    /// `O(n log n)` into the prepared buffers.
    pub fn set_speed(&mut self, metres_per_second: f64) {
        self.speed = metres_per_second.max(1e-6);
        self.design();
    }

    pub fn set_head(&mut self, head: PlayHead, thickness: f64) {
        self.head = head;
        self.thickness = thickness;
        self.design();
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// The taps, in time order.
    pub fn taps(&self) -> impl Iterator<Item = f64> + '_ {
        self.taps.iter().rev().copied()
    }

    fn design(&mut self) {
        let (n, m) = (self.len, self.work.len());
        let bin = self.rate / m as f64;
        let (head, thickness, speed) = (self.head, self.thickness, self.speed);
        let at = |k: usize| loss(head, thickness, speed, bin * k.min(m - k) as f64);
        let scale = 1.0 / m as f64;
        match self.phase {
            Phase::Linear => {
                for (k, w) in self.work.iter_mut().enumerate() {
                    *w = Complex::new(at(k), 0.0);
                }
                self.fft.inverse(&mut self.work);
                let centre = (n - 1) / 2;
                for j in 0..n {
                    let h = self.work[(j + m - centre) % m].re * scale;
                    self.taps[n - 1 - j] = h * blackman(j, n);
                }
            }
            Phase::Minimum => {
                // Real cepstrum of the log magnitude, folded onto positive
                // quefrency, exponentiated back.
                for (k, w) in self.work.iter_mut().enumerate() {
                    *w = Complex::new(at(k).abs().max(FLOOR).ln(), 0.0);
                }
                self.fft.inverse(&mut self.work);
                for (q, w) in self.work.iter_mut().enumerate() {
                    let fold = match q {
                        0 => 1.0,
                        q if q == m / 2 => 1.0,
                        q if q < m / 2 => 2.0,
                        _ => 0.0,
                    };
                    *w = Complex::new(w.re * scale * fold, 0.0);
                }
                self.fft.forward(&mut self.work);
                for w in self.work.iter_mut() {
                    let r = w.re.exp();
                    *w = Complex::new(r * w.im.cos(), r * w.im.sin());
                }
                self.fft.inverse(&mut self.work);
                for j in 0..n {
                    // The decaying half of a window twice as long.
                    let h = self.work[j].re * scale;
                    self.taps[n - 1 - j] = h * blackman(n + j, 2 * n);
                }
            }
        }
    }
}

/// `Σ a[k]·b[k]` in eight running sums: a fixed order the compiler
/// vectorises (one sum is a chain of 1029 dependent adds).
#[inline]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sums = [0.0f64; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            sums[k] += x[k] * y[k];
        }
    }
    let tail: f64 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
    ((sums[0] + sums[4]) + (sums[1] + sums[5])) + ((sums[2] + sums[6]) + (sums[3] + sums[7])) + tail
}

/// Blackman window point `j` of `n`.
fn blackman(j: usize, n: usize) -> f64 {
    let x = std::f64::consts::TAU * j as f64 / (n - 1) as f64;
    0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
}

/// Chowdhury eq. 13 at `hz`.
pub(super) fn loss(head: PlayHead, thickness: f64, speed: f64, hz: f64) -> f64 {
    let k = std::f64::consts::TAU * hz.abs() / speed;
    let spacing = (-k * head.spacing).exp();
    let kd = k * thickness;
    let thickness_loss = if kd < 1e-9 { 1.0 - 0.5 * kd } else { -(-kd).exp_m1() / kd };
    let x = 0.5 * k * head.gap;
    let gap = if x < 1e-9 { 1.0 } else { x.sin() / x };
    spacing * thickness_loss * gap
}

impl Stage for Playback {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = f64::from(sample_rate);
        let n = 2 * (self.rate * HALF_SPAN_SECONDS).round() as usize + 1;
        let m = (OVERSAMPLE * n).next_power_of_two();
        if n != self.len || m != self.work.len() {
            self.len = n;
            self.taps = vec![0.0; n];
            self.history = vec![0.0; 2 * n];
            self.work = vec![Complex::new(0.0, 0.0); m];
            self.fft = Fft::new(m);
            self.pos = 0;
        }
        self.design();
    }
    fn reset(&mut self) {
        self.history.iter_mut().for_each(|h| *h = 0.0);
        self.pos = 0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        let n = self.len;
        let x = f64::from(x);
        self.history[self.pos] = x;
        self.history[self.pos + n] = x;
        let y = dot(&self.history[self.pos + 1..=self.pos + n], &self.taps);
        self.pos += 1;
        if self.pos == n {
            self.pos = 0;
        }
        y as f32
    }
    fn latency(&self) -> u32 {
        match self.phase {
            Phase::Linear => ((self.len - 1) / 2) as u32,
            Phase::Minimum => 0,
        }
    }
    fn tail(&self) -> Tail {
        Tail::Samples(self.len as u32)
    }
}
