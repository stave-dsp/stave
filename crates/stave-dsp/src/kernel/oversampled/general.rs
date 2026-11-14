//! [`Oversampled`]: a clock domain any whole number of times faster,
//! with linear-phase or minimum-phase half-bands.

use crate::kernel::node::{Stage, Tail};
use crate::kernel::resample::allpass::Allpass;
use crate::kernel::resample::polyphase::{edges, kaiser_length, Polyphase, ATTENUATION_DB};
use crate::kernel::resample::{HalfbandDown, HalfbandUp, HB_TAPS, HB_TAPS_INNER};
use crate::units::Db;

/// Block chunk at the outer rate for [`Stage::process`].
const CHUNK: usize = 64;

/// One factor of the region: a 2× half-band pair (linear or
/// minimum phase) or an odd polyphase pair.
// The stages sit in a Vec built once; the half-band rings stay inline
// so the hot loop follows no extra pointer.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum Resampler {
    Halfband {
        up: HalfbandUp,
        down: HalfbandDown,
        taps: usize,
    },
    Allpass(Allpass),
    Polyphase(Polyphase),
}

/// The transition half-width of a 2× stage at outer rate `r`× the base,
/// in cycles per inner sample around the quarter rate: the narrower of
/// the gaps from the pass edge and the stop edge to it.
fn halfband_transition(r: usize) -> f64 {
    let (pass, stop) = edges(r);
    let rate = 2.0 * r as f64;
    (0.25 - pass / rate).min(stop / rate - 0.25)
}

/// A linear-phase half-band length for a stage at `r ≥ 4`: Kaiser's
/// estimate, `≡ 1 (mod 4)`, with `(taps − 1)` a multiple of `2r` so its
/// round trip is whole base samples.
fn deep_halfband_taps(r: usize) -> usize {
    let est = kaiser_length(2.0 * halfband_transition(r));
    let step = 2 * r;
    ((est - 1.0) / step as f64).ceil() as usize * step + 1
}

impl Resampler {
    fn halfband(r: usize, minimum: bool) -> Self {
        if minimum {
            return Self::Allpass(Allpass::new(Db::new(ATTENUATION_DB as f32), halfband_transition(r)));
        }
        let taps = match r {
            1 => HB_TAPS,
            2 => HB_TAPS_INNER,
            _ => deep_halfband_taps(r),
        };
        let (up, down) = match r {
            1 => (HalfbandUp::new(), HalfbandDown::new()),
            2 => (HalfbandUp::inner(), HalfbandDown::inner()),
            _ => (HalfbandUp::with_taps(taps), HalfbandDown::with_taps(taps)),
        };
        Self::Halfband { up, down, taps }
    }

    fn factor(&self) -> usize {
        match self {
            Self::Halfband { .. } | Self::Allpass(_) => 2,
            Self::Polyphase(p) => p.factor(),
        }
    }

    /// The round trip's delay at this stage's outer rate (exact; at DC
    /// for the allpass design).
    fn delay(&self) -> f64 {
        match self {
            Self::Halfband { taps, .. } => (taps - 1) as f64 / 2.0,
            Self::Allpass(a) => a.delay(),
            Self::Polyphase(p) => p.delay(),
        }
    }

    /// The round trip's ring-out at this stage's outer rate: the FIR
    /// support, or the slowest pole's decay to −120 dB through both
    /// cascades of the allpass design.
    fn ring_out(&self) -> u32 {
        match self {
            Self::Halfband { taps, .. } => *taps as u32,
            Self::Polyphase(p) => p.support(),
            Self::Allpass(a) => {
                let n = Tail::from_pole(a.slowest_pole())
                    .to_samples()
                    .unwrap_or(u32::MAX);
                n.saturating_mul(2).saturating_add(a.delay().ceil() as u32)
            }
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Halfband { up, down, .. } => {
                up.reset();
                down.reset();
            }
            Self::Allpass(a) => a.reset(),
            Self::Polyphase(p) => p.reset(),
        }
    }

    fn up(&mut self, input: &[f32], out: &mut [f32]) {
        match self {
            Self::Halfband { up, .. } => up.process_block(input, out),
            Self::Allpass(a) => a.up(input, out),
            Self::Polyphase(p) => p.up(input, out),
        }
    }

    fn down(&mut self, input: &[f32], out: &mut [f32]) {
        match self {
            Self::Halfband { down, .. } => down.process_block(input, out),
            Self::Allpass(a) => a.down(input, out),
            Self::Polyphase(p) => p.down(input, out),
        }
    }
}

/// The prime factors of `n`, ascending (so every 2 comes first).
fn primes(mut n: u32) -> Vec<usize> {
    let mut out = Vec::new();
    let mut p = 2;
    while n > 1 {
        if n % p == 0 {
            out.push(p as usize);
            n /= p;
        } else {
            p += 1;
        }
    }
    out
}

/// A stage run in a clock domain `factor` times faster than its host:
/// upsample, run the interior at `factor`× the rate, decimate.
///
/// One type for every ratio and both phase responses; the ratio and the
/// phase are the constructor's: [`linear`](Self::linear) or
/// [`minimum`](Self::minimum), each with any whole `factor ≥ 1`. The
/// factor is built from its primes, outermost first: every 2 is a
/// half-band pair, every odd prime a polyphase pair. The outermost stage
/// carries the steep filter that guards the host's Nyquist; the stages
/// inside it only keep their images clear of what it passes, so they
/// are short. [`Oversampled2x`](crate::Oversampled2x) stays the 2×/4×
/// form plugins already build; `Oversampled::linear(s, 2)` and
/// `Oversampled::linear(s, 4)` run its exact filters, sample for sample.
///
/// ```
/// use stave_dsp::{Oversampled, Stage};
///
/// let mut x6 = Oversampled::linear((), 6);
/// x6.prepare(48_000.0);
/// assert_eq!(x6.latency(), 73); // 66 for the 2× front, 7 for the 3× inside it
/// let y = x6.tick_with(0.5, |(), s| (4.0 * s).tanh());
/// assert!(y.is_finite());
///
/// // The IIR half-band: about 4 samples at low frequencies instead of 66.
/// let x2 = Oversampled::minimum((), 2);
/// assert_eq!(x2.latency(), 4);
/// assert!((x2.latency_exact() - 3.94).abs() < 0.01);
/// ```
///
/// # Models
///
/// A sample-rate change by a whole factor around an interior stage:
/// zero-stuffing interpolation and decimation, each through a lowpass
/// at the host's band edge. Linear phase: Kaiser windowed-sinc FIRs
/// (half-bands for 2, Nyquist-band polyphase filters for odd primes).
/// Minimum phase: odd-order elliptic half-bands as two allpass cascades.
///
/// # Derivation
///
/// Polyphase interpolation and decimation (Crochiere & Rabiner,
/// *Multirate Digital Signal Processing*, 1983, §3.3); multistage
/// designs whose inner stages relax to the band the outer stage keeps
/// (§5.2); Kaiser's window and length formulas (Oppenheim & Schafer
/// §7.5.3), sized for a 96 dB stopband with the pass edge at 20 kHz of
/// 44.1 kHz. The allpass half-band of Valenzuela and Constantinides
/// (IEE Proc. G, 1983), its coefficients in closed form from the
/// elliptic modulus (de Soras, HIIR).
///
/// # Verified
///
/// `crates/stave-dsp/src/kernel/oversampled/tests.rs::every_factor_meets_its_spec`
/// (at 44.1 kHz, for 2, 3, 4, 6 and 8 in both phases: passband flat
/// within 0.001 dB to 20 kHz, linear and minimum alike; stopband and
/// images at least 90 dB down; the measured group delay within 0.01
/// samples of [`latency_exact`](Self::latency_exact)),
/// `crates/stave-dsp/src/kernel/oversampled/tests.rs::linear_two_and_four_are_oversampled2x`
/// (bit for bit) and
/// `crates/stave-dsp/src/kernel/oversampled/tests.rs::block_and_tick_agree`.
///
/// # Cost
///
/// Datasheet `"Oversampled linear 2x (wire)"`, `"Oversampled linear 3x (wire)"`,
/// `"Oversampled linear 4x (wire)"`, `"Oversampled linear 6x (wire)"`,
/// `"Oversampled linear 8x (wire)"`, `"Oversampled linear 16x (wire)"`,
/// `"Oversampled minimum 2x (wire)"`, `"Oversampled minimum 4x (wire)"`
/// and `"Oversampled minimum 8x (wire)"`: the filters alone, around an
/// interior that does nothing. The linear half-bands compute eight
/// outputs at a time in a block, each in its own `f64` lane (DSP.md P4),
/// so a block costs well under the tick loop and returns its samples bit
/// for bit.
/// Minimum phase buys latency, not cycles: its allpass cascades run
/// every phase in f64, so at 4x and 8x it costs more than linear.
///
/// # Aliasing
///
/// What it is for: a nonlinearity inside folds only what lies above
/// `factor` × the host's Nyquist, and the decimator rejects everything
/// that would fold below 20 kHz (at 44.1 kHz) by at least the measured
/// stopband. The shoulder between 20 kHz and the host's Nyquist folds
/// onto itself, above 20 kHz.
///
/// # Valid ranges
///
/// Any `factor ≥ 1` (1 is a wire); the cost grows with each odd prime
/// (a 3× front stage is 199 taps). Latency: exact in
/// [`latency_exact`](Self::latency_exact); [`Stage::latency`] rounds it
/// to the nearest sample (half up). The linear designs are whole
/// samples by construction, so they round only an interior's own
/// fraction; the minimum-phase delay is the group delay at DC, longer
/// toward the band edge. An odd factor has no 2× stage to make
/// minimum-phase, so `minimum(s, 3)` is `linear(s, 3)`.
pub struct Oversampled<S: Stage> {
    /// The stage in the faster domain.
    pub inner: S,
    factor: u32,
    stages: Vec<Resampler>,
    /// The interior buffers, outermost first: level `i + 1` (the output
    /// of stage `i`) at `offsets[i]`, `CHUNK · rate` long.
    levels: Vec<f32>,
    offsets: Vec<usize>,
    /// The filters' own round trip at the outer rate, exact.
    delay: f64,
    /// The filters' own ring-out at the outer rate.
    ring_out: u32,
}

impl<S: Stage + Clone> Clone for Oversampled<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            factor: self.factor,
            stages: self.stages.clone(),
            levels: self.levels.clone(),
            offsets: self.offsets.clone(),
            delay: self.delay,
            ring_out: self.ring_out,
        }
    }
}

impl<S: Stage> Oversampled<S> {
    /// Linear-phase filters: a constant delay at every frequency (66
    /// samples for the 2× front, 66 for a 3× front), so a parallel dry
    /// path aligns with a plain integer delay.
    ///
    /// # Panics
    ///
    /// If `factor` is 0.
    pub fn linear(inner: S, factor: u32) -> Self {
        Self::build(inner, factor, false)
    }

    /// Minimum-phase IIR half-bands for the factor's 2s: the same
    /// passband and stopband for a few samples of delay at low
    /// frequencies. Its phase is not linear: mixing it with a dry path
    /// combs near the band edge. The odd primes stay linear phase (short
    /// inside a 2× front; the whole steep filter for an odd factor).
    ///
    /// # Panics
    ///
    /// If `factor` is 0.
    pub fn minimum(inner: S, factor: u32) -> Self {
        Self::build(inner, factor, true)
    }

    fn build(inner: S, factor: u32, minimum: bool) -> Self {
        assert!(factor >= 1, "Oversampled: factor must be at least 1");
        let mut stages = Vec::new();
        let mut offsets = Vec::new();
        let (mut r, mut len, mut delay, mut ring_out) = (1usize, 0usize, 0.0f64, 0u32);
        for p in primes(factor) {
            let stage = if p == 2 {
                Resampler::halfband(r, minimum)
            } else {
                Resampler::Polyphase(Polyphase::new(p, r))
            };
            delay += stage.delay() / r as f64;
            ring_out = ring_out.saturating_add(stage.ring_out().div_ceil(r as u32));
            r *= stage.factor();
            offsets.push(len);
            len += CHUNK * r;
            stages.push(stage);
        }
        Self {
            inner,
            factor,
            stages,
            levels: vec![0.0; len],
            offsets,
            delay,
            ring_out,
        }
    }

    /// The clock-domain factor between outside and inside.
    pub fn factor(&self) -> u32 {
        self.factor
    }

    /// The region's latency in outer samples, exactly: the filters'
    /// round trip plus the interior's latency divided back. A fraction
    /// is kept; [`Stage::latency`] rounds it to the nearest sample.
    pub fn latency_exact(&self) -> f64 {
        self.delay + f64::from(self.inner.latency()) / f64::from(self.factor)
    }

    /// One outer sample through a closure interior: `body` runs once per
    /// inner sample with the interior stage and that sample, as
    /// [`Oversampled2x::tick_with`](crate::Oversampled2x::tick_with).
    #[inline]
    pub fn tick_with(&mut self, x: f32, mut body: impl FnMut(&mut S, f32) -> f32) -> f32 {
        let mut io = [x];
        self.run(&mut io, |inner, block| {
            for s in block {
                *s = body(inner, *s);
            }
        });
        io[0]
    }

    /// Upsample `chunk` (at most [`CHUNK`]) through every stage, run
    /// `body` once over the innermost block, decimate back in place.
    fn run(&mut self, chunk: &mut [f32], mut body: impl FnMut(&mut S, &mut [f32])) {
        let Self {
            inner,
            stages,
            levels,
            offsets,
            ..
        } = self;
        let n = chunk.len();
        if stages.is_empty() {
            body(inner, chunk);
            return;
        }
        // Level i+1's span: at offsets[i], n · (rate after stage i).
        let mut spans = [(0usize, 0usize); 32];
        let mut rate = 1;
        for (i, s) in stages.iter().enumerate() {
            rate *= s.factor();
            spans[i] = (offsets[i], n * rate);
        }
        for i in 0..stages.len() {
            let (at, len) = spans[i];
            if i == 0 {
                stages[0].up(chunk, &mut levels[at..at + len]);
            } else {
                let (prev, this) = levels.split_at_mut(at);
                let (pat, plen) = spans[i - 1];
                stages[i].up(&prev[pat..pat + plen], &mut this[..len]);
            }
        }
        let (at, len) = spans[stages.len() - 1];
        body(inner, &mut levels[at..at + len]);
        for i in (0..stages.len()).rev() {
            let (at, len) = spans[i];
            if i == 0 {
                stages[0].down(&levels[at..at + len], chunk);
            } else {
                let (prev, this) = levels.split_at_mut(at);
                let (pat, plen) = spans[i - 1];
                stages[i].down(&this[..len], &mut prev[pat..pat + plen]);
            }
        }
    }
}

impl<S: Stage> Stage for Oversampled<S> {
    fn prepare(&mut self, sample_rate: f32) {
        self.stages.iter_mut().for_each(Resampler::reset);
        self.inner.prepare(sample_rate * self.factor as f32);
    }
    fn reset(&mut self) {
        self.stages.iter_mut().for_each(Resampler::reset);
        self.inner.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_with(x, |inner, s| inner.tick(s))
    }
    /// Upsample a chunk, run the interior's block form once over it,
    /// decimate back: the same samples as the tick loop whenever the
    /// interior's `process` is.
    fn process(&mut self, block: &mut [f32]) {
        for chunk in block.chunks_mut(CHUNK) {
            self.run(chunk, |inner, b| inner.process(b));
        }
    }
    /// [`latency_exact`](Oversampled::latency_exact) rounded to the
    /// nearest sample, half up.
    fn latency(&self) -> u32 {
        (self.latency_exact() + 0.5).floor() as u32
    }
    /// The interior's tail at the outer rate (rounded up), then the
    /// filters' own ring-out.
    fn tail(&self) -> Tail {
        self.inner.tail().div_ceil(self.factor).plus(self.ring_out)
    }
}
