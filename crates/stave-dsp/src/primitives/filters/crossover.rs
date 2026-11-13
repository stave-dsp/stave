//! The N-band crossover: Linkwitz–Riley of a chosen [`Order`], or
//! linear phase (F6's windowed-sinc design run through F2's
//! convolver). The split behind [`Multiband`](crate::effects::dynamics::Multiband)
//! and any band-wise processing.

use crate::kernel::node::{Stage, Tail};
use crate::primitives::spectral::{Convolver, Fir, Head, Passband, Window};
use crate::units::{Db, Hz};

/// The slope of a Linkwitz–Riley crossover: the squared Butterworth of
/// half its order, `6·order` dB/octave.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// 12 dB/octave (squared first-order Butterworth; its highpass is
    /// inverted so the pair sums to an allpass).
    Lr2,
    /// 24 dB/octave, the standard.
    #[default]
    Lr4,
    /// 36 dB/octave (highpass inverted, as for [`Order::Lr2`]).
    Lr6,
    /// 48 dB/octave.
    Lr8,
}

impl Order {
    /// The Butterworth order squared: half the Linkwitz–Riley order.
    const fn butterworth(self) -> usize {
        match self {
            Order::Lr2 => 1,
            Order::Lr4 => 2,
            Order::Lr6 => 3,
            Order::Lr8 => 4,
        }
    }

    /// The slope past the corner, in dB per octave.
    pub const fn db_per_octave(self) -> f32 {
        6.0 * 2.0 * self.butterworth() as f32
    }
}

/// One f64 second-order section, transposed direct form II (a
/// first-order section has `b2 = a2 = 0`).
#[derive(Copy, Clone, Debug, Default)]
struct Section {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Section {
    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }

    fn retune(&mut self, other: Section) {
        self.b = other.b;
        self.a = other.a;
    }

    fn tail(&self) -> Tail {
        Tail::from_biquad(self.a[0], self.a[1])
    }
}

/// What a [`Cascade`] passes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    Lowpass,
    Highpass,
    Allpass,
}

/// Up to four sections in series: an LR8 side, or a Butterworth allpass.
#[derive(Copy, Clone, Debug, Default)]
struct Cascade {
    sections: [Section; 4],
    len: usize,
    sign: f64,
}

impl Cascade {
    /// The `kind` response of an LR order `order` at `hz` and `rate`,
    /// bilinear and prewarped at the corner. Lowpass and highpass are the
    /// Butterworth of half the order, twice; the allpass is that
    /// Butterworth's `B(−s)/B(s)`, what a matched pair sums to.
    fn design(kind: Kind, order: Order, hz: f32, rate: f32) -> Self {
        let n = order.butterworth();
        let f = f64::from(hz).clamp(1.0, 0.499 * f64::from(rate));
        let k = (core::f64::consts::PI * f / f64::from(rate)).tan();
        let mut prototype = [Section::default(); 2];
        let mut count = 0;
        for i in 1..=n / 2 {
            let phi = core::f64::consts::PI * (2 * i + n - 1) as f64 / (2 * n) as f64;
            let q = -1.0 / (2.0 * phi.cos());
            prototype[count] = second_order(kind, k, q);
            count += 1;
        }
        if n % 2 == 1 {
            prototype[count] = first_order(kind, k);
            count += 1;
        }
        let mut c = Cascade { sign: 1.0, ..Default::default() };
        let repeats = if kind == Kind::Allpass { 1 } else { 2 };
        for _ in 0..repeats {
            for s in &prototype[..count] {
                c.sections[c.len] = *s;
                c.len += 1;
            }
        }
        // An odd Butterworth's squared pair sums to an allpass only with
        // the highpass inverted: B(s)·B(−s) = 1 − s^2n.
        if kind == Kind::Highpass && n % 2 == 1 {
            c.sign = -1.0;
        }
        c
    }

    fn retune(&mut self, other: Cascade) {
        for (s, o) in self.sections.iter_mut().zip(other.sections) {
            s.retune(o);
        }
        self.sign = other.sign;
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let mut y = x;
        for s in &mut self.sections[..self.len] {
            y = s.tick(y);
        }
        self.sign * y
    }

    fn reset(&mut self) {
        for s in &mut self.sections {
            s.z = [0.0; 2];
        }
    }

    fn tail(&self) -> Tail {
        self.sections[..self.len]
            .iter()
            .fold(Tail::None, |t, s| t.then(s.tail()))
    }
}

/// The bilinear second-order `kind` at prewarped `k = tan(π·f/fs)`.
fn second_order(kind: Kind, k: f64, q: f64) -> Section {
    let norm = 1.0 / (1.0 + k / q + k * k);
    let a = [2.0 * (k * k - 1.0) * norm, (1.0 - k / q + k * k) * norm];
    let b = match kind {
        Kind::Lowpass => [k * k * norm, 2.0 * k * k * norm, k * k * norm],
        Kind::Highpass => [norm, -2.0 * norm, norm],
        Kind::Allpass => [a[1], a[0], 1.0],
    };
    Section { b, a, z: [0.0; 2] }
}

/// The bilinear first-order `kind`: `1/(1+s)`, `s/(1+s)`, `(1−s)/(1+s)`.
fn first_order(kind: Kind, k: f64) -> Section {
    let norm = 1.0 / (k + 1.0);
    let a1 = (k - 1.0) * norm;
    let b = match kind {
        Kind::Lowpass => [k * norm, k * norm, 0.0],
        Kind::Highpass => [norm, -norm, 0.0],
        Kind::Allpass => [a1, 1.0, 0.0],
    };
    Section { b, a: [a1, 0.0], z: [0.0; 2] }
}

/// The Linkwitz–Riley tree: band `i` is the highpasses below it, its
/// lowpass, and the allpasses of every corner above it.
#[derive(Clone, Debug)]
struct Tree<const N: usize> {
    order: Order,
    low: [Cascade; N],
    high: [Cascade; N],
    /// `align[i][k]`: band `i`'s copy of corner `k`'s allpass, `k > i`.
    align: [[Cascade; N]; N],
}

impl<const N: usize> Tree<N> {
    fn new(order: Order, corners: &[f32; N], rate: f32) -> Self {
        let mut t = Self {
            order,
            low: [Cascade::default(); N],
            high: [Cascade::default(); N],
            align: [[Cascade::default(); N]; N],
        };
        t.design(corners, rate, false);
        t
    }

    /// Coefficients for `corners` at `rate`; state kept when `keep`.
    fn design(&mut self, corners: &[f32; N], rate: f32, keep: bool) {
        for k in 0..N - 1 {
            self.corner(k, corners[k], rate, keep);
        }
    }

    fn corner(&mut self, k: usize, hz: f32, rate: f32, keep: bool) {
        let set = |c: &mut Cascade, new: Cascade| {
            if keep {
                c.retune(new);
            } else {
                *c = new;
            }
        };
        set(&mut self.low[k], Cascade::design(Kind::Lowpass, self.order, hz, rate));
        set(&mut self.high[k], Cascade::design(Kind::Highpass, self.order, hz, rate));
        let ap = Cascade::design(Kind::Allpass, self.order, hz, rate);
        for band in self.align.iter_mut().take(k) {
            set(&mut band[k], ap);
        }
    }

    #[inline]
    fn split(&mut self, x: f32) -> [f32; N] {
        let mut out = [0.0f32; N];
        let mut rest = f64::from(x);
        for i in 0..N - 1 {
            let mut band = self.low[i].tick(rest);
            for k in i + 1..N - 1 {
                band = self.align[i][k].tick(band);
            }
            out[i] = band as f32;
            rest = self.high[i].tick(rest);
        }
        out[N - 1] = rest as f32;
        out
    }

    fn reset(&mut self) {
        for c in self.low.iter_mut().chain(&mut self.high) {
            c.reset();
        }
        for c in self.align.iter_mut().flatten() {
            c.reset();
        }
    }

    fn tail(&self) -> Tail {
        let mut longest = Tail::None;
        let mut above = Tail::None;
        for i in 0..N - 1 {
            let mut band = above.then(self.low[i].tail());
            for k in i + 1..N - 1 {
                band = band.then(self.align[i][k].tail());
            }
            longest = longest.max(band);
            above = above.then(self.high[i].tail());
        }
        longest.max(above)
    }
}

/// The linear-phase split: `N − 1` lowpasses of one length, band `i`
/// the difference of consecutive ones, the top band the delayed input
/// less the highest lowpass, so the bands telescope to the delay.
#[derive(Clone, Debug)]
struct Linear<const N: usize> {
    transition: f32,
    attenuation: f32,
    rate: f32,
    taps: usize,
    lowpass: Vec<Convolver>,
    /// The input delayed by the design's `(taps − 1)/2`.
    delay: Vec<f32>,
    position: usize,
}

impl<const N: usize> Linear<N> {
    fn new(corners: &[f32; N], transition: f32, attenuation: f32, rate: f32) -> Self {
        let mut l = Self {
            transition,
            attenuation,
            rate: 0.0,
            taps: 1,
            lowpass: Vec::new(),
            delay: Vec::new(),
            position: 0,
        };
        l.design(corners, rate);
        l
    }

    /// Kaiser's length estimate for the attenuation over the transition,
    /// odd, then every lowpass at that length (this allocates).
    fn design(&mut self, corners: &[f32; N], rate: f32) {
        let df = f64::from(self.transition) / f64::from(rate);
        let a = f64::from(self.attenuation);
        let taps = (((a - 7.95) / (14.36 * df)).ceil().max(0.0) as usize + 1) | 1;
        let window = Window::kaiser_for(a);
        let largest = (taps / 4).next_power_of_two().clamp(64, 8_192);
        self.lowpass = corners[..N - 1]
            .iter()
            .map(|&hz| {
                let cutoff = (f64::from(hz) / f64::from(rate)).clamp(1e-6, 0.499);
                let fir = Fir::windowed_sinc(Passband::Lowpass { cutoff }, taps, window);
                Convolver::non_uniform(&fir.impulse_response(), 64, largest, Head::Direct)
            })
            .collect();
        self.taps = taps;
        self.rate = rate;
        self.delay = vec![0.0; (taps - 1) / 2 + 1];
        self.position = 0;
    }

    fn latency(&self) -> u32 {
        ((self.taps - 1) / 2) as u32
    }

    #[inline]
    fn split(&mut self, x: f32) -> [f32; N] {
        let len = self.delay.len();
        self.delay[self.position] = x;
        self.position = if self.position + 1 == len { 0 } else { self.position + 1 };
        let delayed = self.delay[self.position];
        let mut out = [0.0f32; N];
        let mut below = 0.0f32;
        for (band, lp) in out.iter_mut().zip(&mut self.lowpass) {
            let y = lp.tick(x);
            *band = y - below;
            below = y;
        }
        out[N - 1] = delayed - below;
        out
    }

    fn reset(&mut self) {
        for c in &mut self.lowpass {
            c.reset();
        }
        self.delay.fill(0.0);
        self.position = 0;
    }
}

#[derive(Clone, Debug)]
enum Split<const N: usize> {
    LinkwitzRiley(Tree<N>),
    LinearPhase(Linear<N>),
}

/// An N-band crossover: Linkwitz–Riley of a chosen [`Order`], or linear
/// phase.
///
/// # Models
///
/// Linkwitz and Riley's crossover (1976): each corner a squared
/// Butterworth lowpass/highpass pair, which sums to the Butterworth's
/// allpass `B(−s)/B(s)`. The bands form a tree (low band split off
/// first); every band below a corner passes that corner's allpass, so
/// the bands sum to the product of the corners' allpasses — flat
/// magnitude, phase rotated around each corner, no latency.
///
/// [`Crossover::linear_phase`] is the FIR alternative: Kaiser-windowed
/// sinc lowpasses of one length (F6), each convolved (F2), band `i` the
/// difference of the lowpasses at its edges and the top band the
/// delayed input less the highest lowpass. The bands telescope to a
/// pure delay of `(taps − 1)/2` samples: exact reconstruction, no phase
/// rotation, that much latency.
///
/// # Derivation
///
/// Butterworth sections of order `n = order/2` (pole angles
/// `π(2k + n − 1)/2n`, `Q = −1/(2 cos φ)`, a first-order section when
/// `n` is odd), bilinear and prewarped at the corner, f64, each side
/// two cascaded copies; the highpass inverted when `n` is odd
/// (`B(s)B(−s) = 1 − s²ⁿ`). The linear-phase length is Kaiser's
/// estimate `(A − 7.95)/(14.36·Δf) + 1`, rounded up to odd, for the
/// attenuation `A` across the transition `Δf`; each lowpass's −6 dB
/// point sits on its corner, mid-transition.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/crossover/tests.rs::linkwitz_riley_bands_sum_flat`:
/// 2-, 3-, 4- and 6-band splits of every order recombine within
/// 0.002 dB of flat from 20 Hz to 20 kHz at 44.1–192 kHz, and each
/// corner reads −6.02 dB within 0.01 dB on its two neighbours.
/// `crates/stave-dsp/src/primitives/filters/crossover/tests.rs::linear_phase_bands_sum_to_the_delay`:
/// the recombined impulse response is the latency's delayed impulse
/// within 2e-6 at every sample.
/// `crates/stave-dsp/src/primitives/filters/crossover/tests.rs::bands_are_isolated`:
/// two octaves past its corners a band is at least `24·n − 1` dB down
/// (LR of order `2n`: 24.6, 48.2, 72.2, 96.3 dB measured), and a whole
/// transition past them at least 80 dB down (linear phase at 80 dB:
/// 87.3 measured).
///
/// # Cost
///
/// `"Crossover (4 bands, LR4)"`, `"Crossover (4 bands, LR8)"` and
/// `"Crossover (4 bands, linear phase)"` on the datasheet.
///
/// # Aliasing
///
/// None: linear.
///
/// # Valid ranges
///
/// `N ≥ 2`; corners ascending inside (1 Hz, 0.499·fs). Linkwitz–Riley
/// corners move per sample with [`set_corner`](Self::set_corner)
/// (coefficients only, state kept); linear-phase corners are designed
/// at `prepare`, where a moved corner takes effect (the design
/// allocates). A linear-phase corner closer than half the transition to
/// 0 Hz or Nyquist loses its stopband there.
///
/// ```
/// use stave_dsp::primitives::filters::crossover::{Crossover, Order};
/// use stave_dsp::units::hz;
/// use stave_dsp::Stage;
///
/// let mut split = Crossover::<3>::linkwitz_riley(Order::Lr4, &[hz(200.0), hz(2_000.0)]);
/// split.prepare(48_000.0);
/// let [low, mid, high] = split.split(0.5);
/// assert!((low + mid + high).is_finite());
/// assert_eq!(split.latency(), 0);
/// ```
#[derive(Clone, Debug)]
pub struct Crossover<const N: usize> {
    corners: [f32; N],
    sample_rate: f32,
    split: Split<N>,
}

impl<const N: usize> Crossover<N> {
    fn checked(corners: &[Hz]) -> [f32; N] {
        const { assert!(N >= 2, "a crossover has at least two bands") };
        assert_eq!(corners.len(), N - 1, "{N} bands need {} corners", N - 1);
        let mut c = [0.0f32; N];
        for (k, hz) in corners.iter().enumerate() {
            c[k] = hz.get();
            assert!(c[k] > 0.0, "corner {k} at {} Hz", c[k]);
            assert!(k == 0 || c[k] > c[k - 1], "corners must ascend: {corners:?}");
        }
        c
    }

    /// A Linkwitz–Riley split of `order` at `corners` (`N − 1` of them,
    /// ascending).
    ///
    /// # Panics
    ///
    /// When there are not `N − 1` corners or they do not ascend.
    pub fn linkwitz_riley(order: Order, corners: &[Hz]) -> Self {
        let corners = Self::checked(corners);
        Self {
            corners,
            sample_rate: 48_000.0,
            split: Split::LinkwitzRiley(Tree::new(order, &corners, 48_000.0)),
        }
    }

    /// A linear-phase split at `corners`: `attenuation` of stopband
    /// beyond `transition / 2` from each corner (so the transition is
    /// the band the bands share). The length follows from the
    /// transition at the prepared rate: 80 dB over 100 Hz at 48 kHz is
    /// 2 409 taps, 1 204 samples of latency.
    ///
    /// # Panics
    ///
    /// As [`linkwitz_riley`](Self::linkwitz_riley), and for a
    /// transition or attenuation that is not positive.
    pub fn linear_phase(corners: &[Hz], transition: Hz, attenuation: Db) -> Self {
        let corners = Self::checked(corners);
        assert!(transition.get() > 0.0, "transition {transition:?}");
        assert!(attenuation.get() > 0.0, "attenuation {attenuation:?}");
        Self {
            corners,
            sample_rate: 48_000.0,
            split: Split::LinearPhase(Linear::new(
                &corners,
                transition.get(),
                attenuation.get(),
                48_000.0,
            )),
        }
    }

    /// The corners, ascending.
    pub fn corners(&self) -> [Hz; N] {
        self.corners.map(Hz::new)
    }

    /// Move corner `k` (between bands `k` and `k + 1`). Linkwitz–Riley:
    /// realtime, state kept. Linear phase: takes effect at the next
    /// `prepare`. Corners are not re-sorted; keep them ascending.
    pub fn set_corner(&mut self, k: usize, corner: Hz) {
        debug_assert!(k + 1 < N, "corner {k} of {}", N - 1);
        if k + 1 >= N {
            return;
        }
        self.corners[k] = corner.get();
        if let Split::LinkwitzRiley(t) = &mut self.split {
            t.corner(k, corner.get(), self.sample_rate, true);
        }
    }

    /// One sample split into its `N` bands, low to high.
    #[inline]
    pub fn split(&mut self, x: f32) -> [f32; N] {
        match &mut self.split {
            Split::LinkwitzRiley(t) => t.split(x),
            Split::LinearPhase(l) => l.split(x),
        }
    }
}

impl<const N: usize> Stage for Crossover<N> {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        match &mut self.split {
            Split::LinkwitzRiley(t) => t.design(&self.corners, sample_rate, false),
            Split::LinearPhase(l) => l.design(&self.corners, sample_rate),
        }
        self.reset();
    }
    fn reset(&mut self) {
        match &mut self.split {
            Split::LinkwitzRiley(t) => t.reset(),
            Split::LinearPhase(l) => l.reset(),
        }
    }
    /// The bands recombined: an allpass of the input (Linkwitz–Riley),
    /// or the input delayed by the latency (linear phase).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.split(x).iter().sum()
    }
    fn latency(&self) -> u32 {
        match &self.split {
            Split::LinkwitzRiley(_) => 0,
            Split::LinearPhase(l) => l.latency(),
        }
    }
    fn tail(&self) -> Tail {
        match &self.split {
            Split::LinkwitzRiley(t) => t.tail(),
            Split::LinearPhase(l) => Tail::samples(l.taps as u32),
        }
    }
}

#[cfg(test)]
mod tests;
