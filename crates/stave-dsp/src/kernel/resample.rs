//! [`Oversampled2x`](crate::Oversampled2x)'s resampling machinery:
//! the linear-phase FIR half-band pairs it runs between — `front`
//! (133-tap, full steepness) and `nested` (73-tap, relaxed interior).
//! Crate-private: everything composes `Oversampled2x`, or
//! [`Oversampled`](crate::Oversampled), which also runs the odd-factor
//! polyphase stages of [`polyphase`] and the IIR half-bands of
//! [`allpass`].

pub(crate) mod allpass;
pub(crate) mod polyphase;

use crate::kernel::simd::Lanes;

// ===== Linear-phase FIR half-band oversampler =====
//
// A Butterworth IIR cascade is phase-benign and latency-free, but its
// transition band is the measured ceiling on alias rejection for hard
// nonlinearities (~−34 dB alias-to-signal on hot bright material — the
// filter, not the oversampling factor, is the wall; 4× tested identical).
// This FIR half-band replaces that ceiling with ~96 dB of stopband at the
// cost of a fixed, exactly-integer group delay that callers must
// compensate (`Oversampled2x` reports it as its `latency`).
//
// Design: Kaiser-windowed ideal half-band (cutoff at the outer Nyquist).
// Half-band symmetry ⇒ every even-offset tap except the center is exactly
// zero, so only 33 + 1 multiplies run per filter call. The tap count and
// β are chosen so that, at a 44.1 kHz outer rate (the tightest case):
//
//   - passband is flat (±0.0002 dB design ripple) to 20.0 kHz
//     (0.907·Nyquist);
//   - everything that would fold *below* 20 kHz is rejected by ≥ ~96 dB
//     (stopband starts at 24.1 kHz, which folds to 20 kHz);
//   - the transition shoulder (22.05–24.1 kHz) folds only onto itself
//     (20–22.05 kHz) — the standard half-band trade: partially-rejected
//     fold-through lands at the edge of hearing, never in the midband.
//
// TAPS = 133 (≡ 1 mod 4) makes the round-trip group delay an exact
// integer at the outer rate (66 samples) AND at half the outer rate
// (33 samples) — so a nested 4× stack (outer + inner) has integer total
// latency (99), and dry-path compensation is a plain integer delay with
// no half-sample comb error.

/// Half-band FIR length. Must be ≡ 1 (mod 4): (TAPS−1)/2 is the per-filter
/// delay and (TAPS−1)/4 the per-oversampler base-rate delay — both integer.
pub(crate) const HB_TAPS: usize = 133;
/// Center-tap index = per-filter group delay in samples (at the filter's
/// own running rate, i.e. 2× the outer rate).
const HB_M: usize = (HB_TAPS - 1) / 2; // 66
/// Number of distinct nonzero off-center coefficients (odd offsets 1..=65,
/// symmetric pairs share one coefficient).
const HB_HALF: usize = (HB_TAPS - 1) / 4; // 33
/// The reference filter's ring size (power of two ≥ HB_TAPS).
#[cfg(test)]
const HB_RING: usize = 256;
/// Kaiser β for ~96 dB stopband: β = 0.1102·(A − 8.7), A = 96.
pub(crate) const HB_BETA: f64 = 9.62;

/// Zeroth-order modified Bessel function of the first kind (series;
/// converges fast for the β range used here).
pub(crate) fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0f64;
    let mut term = 1.0f64;
    let half = 0.5 * x;
    let mut k = 1.0f64;
    loop {
        term *= (half / k) * (half / k);
        sum += term;
        if term < sum * 1e-14 {
            return sum;
        }
        k += 1.0;
    }
}

/// Inner-stage tap count: the inner
/// 2× of a nested 4× stack decimates 4fs → 2fs, where the protected band
/// (≤ 20 kHz) is only ~0.42 of ITS outer Nyquist — a transition ~4×
/// wider than the outer stage's. 73 taps (≡ 1 mod 4 ⇒ 18 base samples,
/// exactly integer) delivers the same ~96 dB stopband there at 45 % of
/// the outer filter's cost.
pub(crate) const HB_TAPS_INNER: usize = 73;

/// The same taps as `primitives::spectral::Fir::windowed_sinc` with a
/// quarter-rate lowpass and `Window::Kaiser { beta: HB_BETA }`, bit for
/// bit (pinned by `the_oversampler_half_bands_are_kaiser_designs`); the
/// kernel sits below `primitives`, so it keeps its own copy.
///
/// Compute the nonzero half-band coefficients for a given tap count:
/// `(center, side[j])`, `side[j]` multiplying the symmetric pair at odd
/// offset `2j+1`. Normalised so the DC gain is exactly 1.
pub(crate) fn halfband_coeffs_n(taps: usize) -> (f64, [f64; HB_HALF]) {
    let m = ((taps - 1) / 2) as f64;
    let n_side = (taps - 1) / 4;
    let inv_i0 = 1.0 / bessel_i0(HB_BETA);
    let mut side = [0.0f64; HB_HALF];
    for (j, s) in side.iter_mut().take(n_side).enumerate() {
        let k = (2 * j + 1) as f64; // odd offset from center
                                    // Ideal half-band lowpass: h[k] = sin(πk/2)/(πk), k odd.
        let ideal = (core::f64::consts::FRAC_PI_2 * k).sin() / (core::f64::consts::PI * k);
        let r = k / m;
        let w = bessel_i0(HB_BETA * (1.0 - r * r).max(0.0).sqrt()) * inv_i0;
        *s = ideal * w;
    }
    let mut center = 0.5f64;
    let dc = center + 2.0 * side[..n_side].iter().sum::<f64>();
    let g = 1.0 / dc;
    center *= g;
    for s in side.iter_mut() {
        *s *= g;
    }
    (center, side)
}

/// Outputs a half-band computes at once in a block: eight `f64`
/// accumulators, four NEON (or SSE2) registers, each output's sum in its
/// own lane in the scalar order — so a block is bit-identical to the
/// tick loop (DSP.md P4).
const LANES: usize = 8;
type F64s<const N: usize> = Lanes<f64, N>;

/// Look-back every design needs: the upsampler reads `2·half − 1`
/// samples behind the newest, the decimator's odd phase `2·half`.
const KEEP: usize = 2 * HB_HALF;
/// A history line's length; a block is cut into runs of at most
/// [`RUN`] samples so each fits after the kept look-back.
const LINE: usize = 256;
const RUN: usize = 128;

/// The upsampler's windows for the `n` outputs whose inputs sit at
/// `p..p + n`, `h` side taps: the centre tap's `n` samples from `p − h`,
/// and the side taps' `2h − 1 + n` from `p + 1 − 2h` (newer =
/// sig[p−h+j+1], older = sig[p−h−j]). `outputs` slices each from its
/// start and its reader (`widen`, `side_taps`) cuts it to its length, the
/// form the lane loads compile best from; the proof holds the whole range
/// inside the line.
#[inline(always)]
fn up_windows(p: usize, h: usize, n: usize) -> (core::ops::Range<usize>, core::ops::Range<usize>) {
    (p - h..p - h + n, p + 1 - 2 * h..p + n)
}

/// The decimator's windows for the `n` outputs whose pairs sit at
/// `q..q + n`: the even line's `n` samples from `q − h` (the centre
/// tap), the odd line's `2h − 1 + n` from `q − 2h` (the side taps).
#[inline(always)]
fn down_windows(q: usize, h: usize, n: usize) -> (core::ops::Range<usize>, core::ops::Range<usize>) {
    (q - h..q - h + n, q - 2 * h..q + n - 1)
}

/// `acc + Σ_j side[j]·(w[h+j] + w[h−1−j])` for `N` outputs, `h` the
/// side taps: the symmetric pairs around a window's middle, newer first,
/// in the scalar order. `w` is cut to exactly the window first, so the
/// tap loads carry no bounds checks of their own.
#[inline(always)]
fn side_taps<const N: usize>(mut acc: F64s<N>, side: &[f64], w: &[f32]) -> F64s<N> {
    let h = side.len();
    let w = &w[..2 * h - 1 + N];
    for (j, &c) in side.iter().enumerate() {
        let pair = F64s::<N>::widen(&w[h + j..]) + F64s::widen(&w[h - 1 - j..]);
        acc = acc.mul_add(F64s::splat(c), pair);
    }
    acc
}

/// A sample history kept contiguous: the newest samples end at `end`,
/// the [`KEEP`] before them are the look-back, and when the line is full
/// the look-back moves to the front (one copy every few hundred
/// samples). Unlike a ring, any window of it is one slice, so a run of
/// outputs reads its taps as lanes.
#[derive(Clone)]
struct Line {
    buf: [f32; LINE],
    end: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0.0; LINE],
            end: KEEP,
        }
    }

    /// Room for `n ≤ RUN` more samples after `end`.
    #[inline]
    fn room(&mut self, n: usize) {
        debug_assert!(n <= RUN);
        if let Some(back) = look_back(self.end, n) {
            self.buf.copy_within(back, 0);
            self.end = KEEP;
        }
    }
}

/// The look-back `room` moves to the front when `n` more samples after
/// `end` would overrun the line (the newest [`KEEP`]), or `None` when
/// they fit; after a move the line ends at `KEEP`.
#[inline(always)]
fn look_back(end: usize, n: usize) -> Option<core::ops::Range<usize>> {
    (end + n > LINE).then(|| end - KEEP..end)
}

/// Polyphase half-band UPSAMPLER. The zero-stuffed formulation ran
/// the full tap loop twice per input sample with half the reads landing
/// on inserted zeros; here the history holds only real samples and each
/// output phase touches exactly the taps that see signal:
/// even phase = `center · sig[n−half]`, odd phase = the side-tap sum.
/// Product ORDER matches the zero-stuffed reference per output, and the
/// skipped terms were exact zeros — outputs are bit-identical (proven
/// by `polyphase_outer_is_bit_identical_to_zero_stuffed`). A block
/// computes [`LANES`] outputs at a time, each in its own lane.
#[derive(Clone)]
pub(crate) struct HalfbandUp {
    sig: Line,
    center: f64,
    side: [f64; HB_HALF],
    half: usize,
}

impl HalfbandUp {
    pub fn new() -> Self {
        Self::with_taps(HB_TAPS)
    }

    pub fn inner() -> Self {
        Self::with_taps(HB_TAPS_INNER)
    }

    /// The Kaiser half-band at any length `taps ≡ 1 (mod 4)`, up to
    /// [`HB_TAPS`] (the deeper levels of an
    /// [`Oversampled`](crate::Oversampled) stack).
    pub fn with_taps(taps: usize) -> Self {
        debug_assert!(taps % 4 == 1 && taps <= HB_TAPS);
        let (center, side) = halfband_coeffs_n(taps);
        Self {
            sig: Line::new(),
            center,
            side,
            half: (taps - 1) / 4,
        }
    }

    pub fn reset(&mut self) {
        self.sig = Line::new();
    }

    /// The `(even, odd)` outputs of the `N` inputs at `p..p + N`.
    /// Index map (from the zero-stuffed offsets m∓d, d = 2j+1): newer =
    /// sig[p−half+j+1], older = sig[p−half−j].
    #[inline(always)]
    fn outputs<const N: usize>(&self, p: usize) -> (F64s<N>, F64s<N>) {
        let (buf, h) = (&self.sig.buf, self.half);
        let (centre, sides) = up_windows(p, h, N);
        // Even phase: the center tap is the only one that sees signal.
        let even = F64s::<N>::widen(&buf[centre.start..]) * self.center;
        // Odd phase: every side tap sees signal; center sees a zero.
        let odd = side_taps(F64s::default(), &self.side[..h], &buf[sides.start..]);
        (even, odd)
    }

    /// Push one input sample (pre-scaled by the caller, matching the
    /// `2·x` of the zero-stuffed path) and return the two upsampled
    /// outputs `(even, odd)` in stream order.
    #[inline]
    pub fn process_pair(&mut self, x2: f32) -> (f32, f32) {
        self.sig.room(1);
        let p = self.sig.end;
        self.sig.buf[p] = x2;
        self.sig.end += 1;
        let (even, odd) = self.outputs::<1>(p);
        (even.0[0] as f32, odd.0[0] as f32)
    }

    /// Block interpolation: consume N input samples (unscaled — the 2×
    /// zero-insertion compensation is applied here), write 2N upsampled
    /// samples in stream order. Per input this is exactly
    /// `process_pair(2·x)` — bit-identical to the per-sample path.
    pub fn process_block(&mut self, input: &[f32], out: &mut [f32]) {
        debug_assert_eq!(out.len(), 2 * input.len());
        for (input, out) in input.chunks(RUN).zip(out.chunks_mut(2 * RUN)) {
            let n = input.len();
            self.sig.room(n);
            let p0 = self.sig.end;
            for (s, &x) in self.sig.buf[p0..p0 + n].iter_mut().zip(input) {
                *s = 2.0 * x;
            }
            self.sig.end += n;
            let mut i = 0;
            while i + LANES <= n {
                let (even, odd) = self.outputs::<LANES>(p0 + i);
                let (even, odd) = (even.narrow(), odd.narrow());
                for (l, pair) in out[2 * i..2 * (i + LANES)].chunks_exact_mut(2).enumerate() {
                    pair[0] = even.0[l];
                    pair[1] = odd.0[l];
                }
                i += LANES;
            }
            for i in i..n {
                let (even, odd) = self.outputs::<1>(p0 + i);
                out[2 * i] = even.0[0] as f32;
                out[2 * i + 1] = odd.0[0] as f32;
            }
        }
    }
}

impl Default for HalfbandUp {
    fn default() -> Self {
        Self::new()
    }
}

/// Polyphase half-band DECIMATOR: of each inner pair it keeps the even
/// phase's output and never computes the odd one. Its even and odd
/// samples live in two histories, so the centre tap reads the even line
/// and every side tap the odd one, contiguously: a block computes
/// [`LANES`] outputs at a time, each in its own lane, in the order the
/// full-rate filter sums them (`center·x[n−m] + Σ c·(newer + older)`) —
/// bit-identical to the full-rate filter's every other output (pinned by
/// `polyphase_outer_is_bit_identical_to_zero_stuffed`).
#[derive(Clone)]
pub(crate) struct HalfbandDown {
    even: Line,
    odd: Line,
    center: f64,
    side: [f64; HB_HALF],
    half: usize,
}

impl Default for HalfbandDown {
    fn default() -> Self {
        Self::new()
    }
}

impl HalfbandDown {
    pub fn new() -> Self {
        Self::with_taps(HB_TAPS)
    }

    /// The relaxed inner-stage design (73 taps — see `HB_TAPS_INNER`).
    pub fn inner() -> Self {
        Self::with_taps(HB_TAPS_INNER)
    }

    /// See [`HalfbandUp::with_taps`].
    pub fn with_taps(taps: usize) -> Self {
        debug_assert!(taps % 4 == 1 && taps <= HB_TAPS);
        let (center, side) = halfband_coeffs_n(taps);
        Self {
            even: Line::new(),
            odd: Line::new(),
            center,
            side,
            half: (taps - 1) / 4,
        }
    }

    pub fn reset(&mut self) {
        self.even = Line::new();
        self.odd = Line::new();
    }

    /// The outputs of the `N` pairs whose samples sit at `q..q + N`
    /// (f64 accumulation for a deep artifact floor). With m = 2·half and
    /// d = 2j+1, the full-rate filter's x[n−m] is even[q−half], its
    /// newer x[n−m+d] odd[q−half+j] and its older x[n−m−d]
    /// odd[q−half−j−1].
    #[inline(always)]
    fn outputs<const N: usize>(&self, q: usize) -> F64s<N> {
        let (even, odd, h) = (&self.even.buf, &self.odd.buf, self.half);
        let (centre, sides) = down_windows(q, h, N);
        let centre = F64s::<N>::widen(&even[centre.start..]) * self.center;
        side_taps(centre, &self.side[..h], &odd[sides.start..])
    }

    #[inline]
    fn room(&mut self, n: usize) {
        self.even.room(n);
        self.odd.room(n);
    }

    /// One inner pair in, the kept (even-phase) output out.
    #[inline]
    pub fn pair(&mut self, even: f32, odd: f32) -> f32 {
        self.room(1);
        let q = self.even.end;
        self.even.buf[q] = even;
        self.odd.buf[q] = odd;
        self.even.end += 1;
        self.odd.end += 1;
        self.outputs::<1>(q).0[0] as f32
    }

    /// Block decimation: consume `up` (2N samples at the inner rate),
    /// write N outputs; per pair exactly [`pair`](Self::pair).
    pub fn process_block(&mut self, up: &[f32], out: &mut [f32]) {
        debug_assert_eq!(up.len(), 2 * out.len());
        for (up, out) in up.chunks(2 * RUN).zip(out.chunks_mut(RUN)) {
            let n = out.len();
            self.room(n);
            let q0 = self.even.end;
            let lines = self.even.buf[q0..q0 + n].iter_mut().zip(&mut self.odd.buf[q0..q0 + n]);
            for ((e, o), pair) in lines.zip(up.chunks_exact(2)) {
                *e = pair[0];
                *o = pair[1];
            }
            self.even.end += n;
            self.odd.end += n;
            let mut i = 0;
            while i + LANES <= n {
                self.outputs::<LANES>(q0 + i).narrow().store(&mut out[i..]);
                i += LANES;
            }
            for i in i..n {
                out[i] = self.outputs::<1>(q0 + i).0[0] as f32;
            }
        }
    }
}

/// One half-band FIR at the full 2× rate, every output computed: the
/// zero-stuffed reference the polyphase pair is pinned against, and the
/// filter whose design spec the tests measure. Exploits the half-band
/// zeros: only (taps−1)/4 + 1 multiplies per sample.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct HalfbandFir {
    buf: [f32; HB_RING],
    idx: usize,
    center: f64,
    side: [f64; HB_HALF],
    m: usize,
    n_side: usize,
}

#[cfg(test)]
impl HalfbandFir {
    pub fn new() -> Self {
        Self::with_taps(HB_TAPS)
    }

    /// The relaxed inner-stage design (73 taps — see `HB_TAPS_INNER`).
    pub fn inner() -> Self {
        Self::with_taps(HB_TAPS_INNER)
    }

    pub fn with_taps(taps: usize) -> Self {
        let (center, side) = halfband_coeffs_n(taps);
        Self {
            buf: [0.0; HB_RING],
            idx: 0,
            center,
            side,
            m: (taps - 1) / 2,
            n_side: (taps - 1) / 4,
        }
    }

    /// Filter one sample (f64 accumulation for a deep artifact floor).
    pub fn tick(&mut self, x: f32) -> f32 {
        self.idx = (self.idx + 1) & (HB_RING - 1);
        self.buf[self.idx] = x;
        let buf = &self.buf;
        let m = self.m;
        let at = |d: usize| buf[(self.idx + HB_RING - d) & (HB_RING - 1)] as f64;
        let mut acc = self.center * at(m);
        for (j, &c) in self.side[..self.n_side].iter().enumerate() {
            let d = 2 * j + 1;
            acc += c * (at(m - d) + at(m + d));
        }
        acc as f32
    }
}

/// 2× oversampling around a linear-phase FIR half-band pair: `latency()`
/// samples of group delay at the outer rate. Decimation keeps the even
/// phase so the latency is exactly integer.
#[derive(Clone)]
pub(crate) struct Oversampler2xFir {
    up: HalfbandUp,
    down: HalfbandDown,
}

impl Default for Oversampler2xFir {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2xFir {
    pub fn new() -> Self {
        Self {
            up: HalfbandUp::new(),
            down: HalfbandDown::new(),
        }
    }

    /// Nothing is rate-dependent (the design is normalised to the outer
    /// Nyquist): clears state, for drop-in symmetry with the IIR
    /// reference in tests.
    #[cfg(test)]
    pub fn prepare(&mut self, _sample_rate: f32) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.up.reset();
        self.down.reset();
    }

    /// Group delay of the up+down chain, in samples at the OUTER rate.
    /// Exactly integer by construction (TAPS ≡ 1 mod 4).
    pub const fn latency() -> usize {
        HB_M // (133−1) inner samples round trip = 66 outer samples
    }

    /// Run `inner` twice (at 2× rate) between the interpolation and
    /// decimation half-bands. The `2·x` on the non-zero-stuffed sample
    /// compensates the zero-insertion energy loss (unity passband gain).
    #[inline]
    pub fn tick_with<F: FnMut(f32) -> f32>(&mut self, x: f32, mut inner: F) -> f32 {
        // Polyphase both directions: the upsampler computes only
        // signal-bearing taps per phase; the decimator's discarded odd
        // output becomes a state-only push. Bit-identical to the
        // zero-stuffed reference (see tests), ~half the MACs.
        let (u0, u1) = self.up.process_pair(2.0 * x);
        // Keep the EVEN decimation phase (return the first of the pair):
        // the impulse peak of the 132-sample round trip lands on an even
        // inner index, so the outer-rate latency is exactly 66 samples.
        let even = inner(u0);
        let odd = inner(u1);
        self.down.pair(even, odd)
    }

    /// Block form of [`tick_with`](Self::tick_with): upsample the
    /// whole block into `up_scratch` (≥ 2·`io.len()`), run `inner` ONCE
    /// over the 2× block, decimate back in place. Bit-identical to the
    /// per-sample path (each component sees the same sample sequence;
    /// `inner` sees the identical 2× stream) — pinned in
    /// `tests/block_api.rs` through `Oversampled2x::process`.
    pub fn process_with<F: FnMut(&mut [f32])>(
        &mut self,
        io: &mut [f32],
        up_scratch: &mut [f32],
        mut inner: F,
    ) {
        let up = &mut up_scratch[..2 * io.len()];
        self.up.process_block(io, up);
        inner(up);
        self.down.process_block(up, io);
    }
}

/// The INNER 2× of a nested 4× stack — same API, relaxed 73-tap
/// design (see `HB_TAPS_INNER`), exactly-integer 18-sample latency at
/// the BASE rate (its own outer rate is 2× base, so 36 samples there).
#[derive(Clone)]
pub(crate) struct Oversampler2xFirInner {
    up: HalfbandUp,
    down: HalfbandDown,
}

impl Default for Oversampler2xFirInner {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2xFirInner {
    pub fn new() -> Self {
        Self {
            up: HalfbandUp::inner(),
            down: HalfbandDown::inner(),
        }
    }
    pub fn reset(&mut self) {
        self.up.reset();
        self.down.reset();
    }
    /// Group delay at the BASE rate when used as the inner stage of a 4×
    /// stack: (73−1) samples at 4× = 18 base samples, exactly integer.
    pub const fn latency_base() -> usize {
        (HB_TAPS_INNER - 1) / 4
    }
    #[inline]
    pub fn tick_with<F: FnMut(f32) -> f32>(&mut self, x: f32, mut inner: F) -> f32 {
        let (u0, u1) = self.up.process_pair(2.0 * x);
        let even = inner(u0);
        let odd = inner(u1);
        self.down.pair(even, odd)
    }

    /// Block form — see [`Oversampler2xFir::process_with`]; same
    /// contract, relaxed inner design.
    pub fn process_with<F: FnMut(&mut [f32])>(
        &mut self,
        io: &mut [f32],
        up_scratch: &mut [f32],
        mut inner: F,
    ) {
        let up = &mut up_scratch[..2 * io.len()];
        self.up.process_block(io, up);
        inner(up);
        self.down.process_block(up, io);
    }
}

#[cfg(test)]
mod polyphase_bitexact {
    use super::*;

    /// The zero-stuffed algorithm, as the reference.
    struct RefFir {
        up: HalfbandFir,
        down: HalfbandFir,
    }

    impl RefFir {
        fn outer() -> Self {
            Self {
                up: HalfbandFir::new(),
                down: HalfbandFir::new(),
            }
        }
        fn inner_stage() -> Self {
            Self {
                up: HalfbandFir::inner(),
                down: HalfbandFir::inner(),
            }
        }
        fn tick_with<F: FnMut(f32) -> f32>(&mut self, x: f32, mut inner: F) -> f32 {
            let u0 = self.up.tick(2.0 * x);
            let u1 = self.up.tick(0.0);
            let y = self.down.tick(inner(u0));
            let _ = self.down.tick(inner(u1));
            y
        }
    }

    fn lcg_noise(n: usize, mut state: u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// A stateful nonlinear inner block — the realistic composition —
    /// so state skew between the two paths would surface immediately.
    fn make_inner() -> impl FnMut(f32) -> f32 {
        let mut z = 0.0f32;
        move |x: f32| {
            z = 0.7 * z + 0.3 * x;
            let d = x + 0.2 * z;
            d - d * d * d * (1.0 / 6.0)
        }
    }

    /// The polyphase path is BIT-IDENTICAL to the zero-stuffed
    /// reference. Achievable exactly because per-output product order
    /// is preserved and the skipped terms were exact zeros.
    #[test]
    fn polyphase_outer_is_bit_identical_to_zero_stuffed() {
        let noise = lcg_noise(50_000, 7);
        let mut new = Oversampler2xFir::new();
        let mut old = RefFir::outer();
        let mut inner_new = make_inner();
        let mut inner_old = make_inner();
        for (i, &x) in noise.iter().enumerate() {
            let a = new.tick_with(x, &mut inner_new);
            let b = old.tick_with(x, &mut inner_old);
            assert_eq!(a.to_bits(), b.to_bits(), "sample {i}: {a} vs {b}");
        }
    }

    #[test]
    fn polyphase_inner_is_bit_identical_to_zero_stuffed() {
        let noise = lcg_noise(50_000, 9);
        let mut new = Oversampler2xFirInner::new();
        let mut old = RefFir::inner_stage();
        let mut inner_new = make_inner();
        let mut inner_old = make_inner();
        for (i, &x) in noise.iter().enumerate() {
            let a = new.tick_with(x, &mut inner_new);
            let b = old.tick_with(x, &mut inner_old);
            assert_eq!(a.to_bits(), b.to_bits(), "sample {i}: {a} vs {b}");
        }
    }

    /// The lanes (DSP.md P4): blocks of every length, cut so runs end
    /// mid-lane-group and the history lines compact mid-block, at every
    /// half-band length an `Oversampled` stack builds, are the
    /// zero-stuffed reference sample for sample, bit for bit.
    #[test]
    fn lane_blocks_are_bit_identical_to_zero_stuffed() {
        let noise = lcg_noise(20_000, 11);
        for taps in [HB_TAPS, HB_TAPS_INNER, 41, 49, 17] {
            let mut up = HalfbandUp::with_taps(taps);
            let mut down = HalfbandDown::with_taps(taps);
            let mut old = RefFir {
                up: HalfbandFir::with_taps(taps),
                down: HalfbandFir::with_taps(taps),
            };
            let mut inner_new = make_inner();
            let mut inner_old = make_inner();
            let mut scratch = vec![0.0f32; 2 * 310];
            let (mut at, mut len) = (0, 1);
            while at + len <= noise.len() {
                let mut block = noise[at..at + len].to_vec();
                let inner = &mut scratch[..2 * len];
                up.process_block(&block, inner);
                for s in inner.iter_mut() {
                    *s = inner_new(*s);
                }
                down.process_block(inner, &mut block);
                for (i, &y) in block.iter().enumerate() {
                    let r = old.tick_with(noise[at + i], &mut inner_old);
                    assert_eq!(y.to_bits(), r.to_bits(), "{taps} taps, sample {}", at + i);
                }
                at += len;
                len = len % 299 + 7;
            }
        }
    }
}

/// Kani proof (DSP.md R10; `scripts/kani.sh`): the half-band pair's
/// history lines. Every read in `outputs` slices its line with bounds
/// checks (a panic in `tick`, against the crate's contract); this proves
/// none can fail, on the windows `outputs` slices (`up_windows`,
/// `down_windows`) and the reads `side_taps` makes inside them.
#[cfg(kani)]
mod proofs {
    use super::*;

    /// A line in any state the pair can leave it in (`KEEP ≤ end ≤
    /// LINE`), any design to [`HB_TAPS`] (`1 ≤ half ≤ HB_HALF`), any run
    /// of `n ≤ RUN` samples: `room` (its `look_back`) moves exactly the
    /// newest `KEEP` samples, from inside the line, to its front, keeps
    /// that state and leaves room for the run; the upsampler's and the
    /// decimator's windows for a lane group or one output anywhere in the
    /// run lie inside the line, each exactly the length its reader takes,
    /// and every side-tap read lies inside its window; after the run the
    /// state holds again. Indices only: the samples' values never decide
    /// an index (a buffer of floats made the proof time out).
    #[kani::proof]
    fn halfband_windows_stay_inside_their_lines() {
        let h: usize = kani::any_where(|h: &usize| (1..=HB_HALF).contains(h));
        let mut end: usize = kani::any_where(|e: &usize| (KEEP..=LINE).contains(e));
        let n: usize = kani::any_where(|n: &usize| (1..=RUN).contains(n));
        if let Some(back) = look_back(end, n) {
            assert!(back.end == end && back.len() == KEEP && back.end <= LINE);
            end = KEEP;
        }
        assert!(end >= KEEP && end + n <= LINE);
        let width = if kani::any() { LANES } else { 1 };
        let i: usize = kani::any_where(|i: &usize| *i < n && *i + width <= n);
        let at = end + i;
        for (centre, sides) in [up_windows(at, h, width), down_windows(at, h, width)] {
            assert!(centre.start <= centre.end && centre.end <= LINE && centre.len() == width);
            assert!(sides.start <= sides.end && sides.end <= LINE && sides.len() == 2 * h - 1 + width);
        }
        // side_taps: w[h + j..][..N] and w[h − 1 − j..][..N] for j < h.
        let j: usize = kani::any_where(|j: &usize| *j < h);
        assert!(h + j + width <= 2 * h - 1 + width && h - 1 >= j);
        end += n;
        assert!(end >= KEEP && end <= LINE);
    }
}
