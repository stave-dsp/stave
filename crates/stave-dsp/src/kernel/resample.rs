//! [`Oversampled2x`](crate::Oversampled2x)'s resampling machinery:
//! the linear-phase FIR half-band pairs it runs between — `front`
//! (133-tap, full steepness) and `nested` (73-tap, relaxed interior).
//! Crate-private: everything composes `Oversampled2x`.

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
/// Ring size (power of two ≥ HB_TAPS).
const HB_RING: usize = 256;
/// Kaiser β for ~96 dB stopband: β = 0.1102·(A − 8.7), A = 96.
const HB_BETA: f64 = 9.62;

/// Zeroth-order modified Bessel function of the first kind (series;
/// converges fast for the β range used here).
fn bessel_i0(x: f64) -> f64 {
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
const HB_TAPS_INNER: usize = 73;
const HB_M_INNER: usize = (HB_TAPS_INNER - 1) / 2; // 36
const HB_HALF_INNER: usize = (HB_TAPS_INNER - 1) / 4; // 18

/// Compute the nonzero half-band coefficients for a given tap count:
/// `(center, side[j])`, `side[j]` multiplying the symmetric pair at odd
/// offset `2j+1`. Normalised so the DC gain is exactly 1.
fn halfband_coeffs_n(taps: usize) -> (f64, [f64; HB_HALF]) {
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

fn halfband_coeffs() -> (f64, [f64; HB_HALF]) {
    halfband_coeffs_n(HB_TAPS)
}

/// One half-band FIR (running at the 2× rate). Exploits the half-band
/// zeros: only (taps−1)/4 + 1 multiplies per sample. `m`/`n_side` select
/// the outer (133-tap) or relaxed inner (73-tap) design.
#[derive(Clone)]
pub(crate) struct HalfbandFir {
    buf: [f32; HB_RING],
    idx: usize,
    center: f64,
    side: [f64; HB_HALF],
    m: usize,
    n_side: usize,
}

impl Default for HalfbandFir {
    fn default() -> Self {
        Self::new()
    }
}

impl HalfbandFir {
    pub fn new() -> Self {
        let (center, side) = halfband_coeffs();
        Self {
            buf: [0.0; HB_RING],
            idx: 0,
            center,
            side,
            m: HB_M,
            n_side: HB_HALF,
        }
    }

    /// The relaxed inner-stage design (73 taps — see `HB_TAPS_INNER`).
    pub fn inner() -> Self {
        let (center, side) = halfband_coeffs_n(HB_TAPS_INNER);
        Self {
            buf: [0.0; HB_RING],
            idx: 0,
            center,
            side,
            m: HB_M_INNER,
            n_side: HB_HALF_INNER,
        }
    }

    pub fn reset(&mut self) {
        self.buf = [0.0; HB_RING];
        self.idx = 0;
    }

    /// Filter one sample (f64 accumulation for a deep artifact floor).
    #[inline]
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

    /// Advance filter STATE without computing an output — the polyphase
    /// decimation trick: the discarded odd-phase output cost a full
    /// dot product; its only real effect was this ring write.
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.idx = (self.idx + 1) & (HB_RING - 1);
        self.buf[self.idx] = x;
    }

    /// Block decimation: consume `up` (2N samples at the inner rate),
    /// write N outputs. Per pair this is exactly `tick(even)` then
    /// `push(odd)` — same ring, same accumulation order, bit-identical
    /// to the per-sample path (the block win is the hoisted dispatch,
    /// not a different algorithm).
    pub fn process_block_decim(&mut self, up: &[f32], out: &mut [f32]) {
        debug_assert_eq!(up.len(), 2 * out.len());
        for (i, y) in out.iter_mut().enumerate() {
            *y = self.tick(up[2 * i]);
            self.push(up[2 * i + 1]);
        }
    }
}

/// Polyphase half-band UPSAMPLER. The zero-stuffed formulation ran
/// the full tap loop twice per input sample with half the reads landing
/// on inserted zeros; here the ring holds only real samples and each
/// output phase touches exactly the taps that see signal:
/// even phase = `center · sig[n−half]`, odd phase = the side-tap sum.
/// Product ORDER matches the zero-stuffed reference per output, and the
/// skipped terms were exact zeros — outputs are bit-identical (proven
/// by `polyphase_up_is_bit_identical_to_zero_stuffed`).
#[derive(Clone)]
pub(crate) struct HalfbandUp {
    sig: [f32; 128],
    n: usize,
    center: f64,
    side: [f64; HB_HALF],
    half: usize,
}

impl HalfbandUp {
    pub fn new() -> Self {
        let (center, side) = halfband_coeffs();
        Self {
            sig: [0.0; 128],
            n: 0,
            center,
            side,
            half: HB_HALF,
        }
    }

    pub fn inner() -> Self {
        let (center, side) = halfband_coeffs_n(HB_TAPS_INNER);
        Self {
            sig: [0.0; 128],
            n: 0,
            center,
            side,
            half: HB_HALF_INNER,
        }
    }

    pub fn reset(&mut self) {
        self.sig = [0.0; 128];
        self.n = 0;
    }

    /// Push one input sample (pre-scaled by the caller, matching the
    /// `2·x` of the zero-stuffed path) and return the two upsampled
    /// outputs `(even, odd)` in stream order.
    #[inline]
    pub fn process_pair(&mut self, x2: f32) -> (f32, f32) {
        self.n = (self.n + 1) & 127;
        self.sig[self.n] = x2;
        let sig = &self.sig;
        let at = |back: usize| sig[(self.n + 128 - back) & 127] as f64;
        let half = self.half;
        // Even phase: the center tap is the only one that sees signal.
        let even = self.center * at(half);
        // Odd phase: every side tap sees signal; center sees a zero.
        // Index map (derived from the zero-stuffed offsets m∓d,
        // d = 2j+1): newer = sig[n−half+j+1], older = sig[n−half−j].
        let mut odd = 0.0f64;
        for (j, &c) in self.side[..half].iter().enumerate() {
            odd += c * (at(half - j - 1) + at(half + j));
        }
        (even as f32, odd as f32)
    }

    /// Block interpolation: consume N input samples (unscaled — the 2×
    /// zero-insertion compensation is applied here), write 2N upsampled
    /// samples in stream order. Per input this is exactly
    /// `process_pair(2·x)` — bit-identical to the per-sample path.
    pub fn process_block(&mut self, input: &[f32], out: &mut [f32]) {
        debug_assert_eq!(out.len(), 2 * input.len());
        for (i, &x) in input.iter().enumerate() {
            let (e, o) = self.process_pair(2.0 * x);
            out[2 * i] = e;
            out[2 * i + 1] = o;
        }
    }
}

impl Default for HalfbandUp {
    fn default() -> Self {
        Self::new()
    }
}

/// 2× oversampling around a linear-phase FIR half-band pair: `latency()`
/// samples of group delay at the outer rate. Decimation keeps the even
/// phase so the latency is exactly integer.
#[derive(Clone)]
pub(crate) struct Oversampler2xFir {
    up: HalfbandUp,
    down: HalfbandFir,
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
            down: HalfbandFir::new(),
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
        let y = self.down.tick(inner(u0));
        self.down.push(inner(u1));
        y
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
        self.down.process_block_decim(up, io);
    }
}

/// The INNER 2× of a nested 4× stack — same API, relaxed 73-tap
/// design (see `HB_TAPS_INNER`), exactly-integer 18-sample latency at
/// the BASE rate (its own outer rate is 2× base, so 36 samples there).
#[derive(Clone)]
pub(crate) struct Oversampler2xFirInner {
    up: HalfbandUp,
    down: HalfbandFir,
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
            down: HalfbandFir::inner(),
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
        let y = self.down.tick(inner(u0));
        self.down.push(inner(u1));
        y
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
        self.down.process_block_decim(up, io);
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
}
