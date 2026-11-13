//! The linear-phase parametric EQ: the bands' combined magnitude as one
//! symmetric FIR, run by a partitioned convolver.

use super::band::{Band, Grid};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::spectral::{Convolver, FrequencySampling, Head, ImpulseResponse, Window};
use crate::units::{ms, Seconds};

/// The convolver partition: the latency it adds to the FIR's own.
pub(super) const BLOCK: usize = 256;

/// The FIR length at a rate: `length` in samples, odd, at least 3.
pub(super) fn taps_for(length: Seconds, sample_rate: f32) -> usize {
    let n = (length.get() * f64::from(sample_rate)).round().max(3.0) as usize;
    n | 1
}

/// One FIR design: a frequency-sampling plan over a band grid, and the
/// taps it writes. Reused for every redesign; never allocates after
/// [`new`](Self::new).
#[derive(Clone, Debug)]
pub(super) struct Designer {
    pub(super) sampler: FrequencySampling,
    pub(super) grid: Grid,
    taps: Vec<f64>,
    pub(super) taps32: Vec<f32>,
}

impl Designer {
    pub(super) fn new(len: usize, bands: usize) -> Self {
        let sampler = FrequencySampling::new(len, Window::Rectangular);
        let grid = Grid::new(sampler.points(), bands);
        Self {
            sampler,
            grid,
            taps: vec![0.0; len],
            taps32: vec![0.0; len],
        }
    }

    /// Designs the zero-phase `response(grid, k)` (grid point `k`) into
    /// [`taps32`](Self::taps32).
    pub(super) fn design(&mut self, mut response: impl FnMut(&Grid, usize) -> f64) {
        let last = (self.grid.points() - 1) as f64;
        let grid = &self.grid;
        self.sampler.design(
            |f| response(grid, (f * 2.0 * last).round() as usize),
            &mut self.taps,
        );
        // A Tukey taper: flat, with a half-cosine over the outer tenth
        // at each end. The bands' zero-phase responses decay well inside
        // the length, so truncating barely smooths them (a Hann window's
        // main lobe rounds a 30 Hz cut off by 0.3 dB at 85 ms; this
        // taper by 0.01), while the edges stop the truncation ringing.
        let len = self.taps.len();
        let edge = len / 20;
        for (k, (t, &h)) in self.taps32.iter_mut().zip(&self.taps).enumerate() {
            let from_end = k.min(len - 1 - k);
            let w = if from_end < edge {
                0.5 - 0.5 * (core::f64::consts::PI * (from_end as f64 + 0.5) / edge as f64).cos()
            } else {
                1.0
            };
            *t = (h * w) as f32;
        }
    }
}

/// A linear-phase parametric EQ: every [`Band`]'s magnitude, multiplied
/// together, as one symmetric FIR.
///
/// # Models
///
/// The cascade of the bands' cookbook biquads ([`super::MinimumPhase`])
/// in magnitude, with exactly linear phase: `H(f) = Π|H_b(f)|·e^{−2πi·f·D}`,
/// `D` the latency. No hardware claim. Linear phase moves energy ahead
/// of each transient: **pre-ringing**, mirroring the post-ringing a
/// minimum-phase filter has, and as long (a narrow bell rings both ways
/// for about `Q/(π·f)` per neper). Measured on a 48 kHz design
/// (`crates/stave-dsp/src/effects/eq/tests.rs::linear_phase_pre_ringing_is_as_documented`),
/// the response ahead of the main tap stays above −60 dB re the peak
/// for 1.5 ms on a +6 dB 1 kHz bell at Q 1, 5.5 ms at Q 8, and 3.9 ms
/// on a 100 Hz low cut; it is audible on percussive material with
/// steep, narrow or low bands, which is when to prefer the
/// minimum-phase EQ.
///
/// # Derivation
///
/// Frequency sampling ([`FrequencySampling`]): the product of the
/// biquads' magnitudes on a dense grid, inverse-transformed to its
/// zero-phase impulse response and truncated to `length` under a Tukey
/// taper (flat, half-cosine over the outer tenth: a Hann window's main
/// lobe would round a 30 Hz cut off by 0.3 dB at this length; odd taps,
/// symmetric, so the phase is exactly linear), then
/// convolved by a uniform [`Convolver`] in 256-sample partitions. A
/// band change redesigns on the next tick and swaps the response with a
/// one-partition crossfade ([`Convolver::swap_response`]), so automation
/// does not click.
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/eq/tests.rs::linear_phase_matches_the_biquads`:
/// against the biquad cascade's magnitude, measured from the impulse
/// response at 44.1, 48 and 96 kHz on a five-band EQ (cuts, shelves and
/// bells from 30 Hz to 18 kHz): within 0.05 dB from 40 Hz to 20 kHz
/// (worst 0.011 dB at 85 ms) and 0.1 dB from 20 Hz (worst 0.023), the
/// convolver running the design within 1e-5. Phase:
/// `crates/stave-dsp/src/effects/eq/tests.rs::linear_phase_is_exactly_linear`,
/// the impulse response symmetric about the reported latency to 1e-7
/// of its peak and the group delay constant within 0.01 samples where
/// the gain is above −40 dB (the measurement design's phase budget is
/// 0.2 ms); a bell automated in 1 dB steps every 1 024 samples on a
/// 1 kHz tone sprays −70 dB outside ±400 Hz (budget −60),
/// `crates/stave-dsp/src/effects/eq/tests.rs::linear_phase_band_changes_do_not_click`.
///
/// # Cost
///
/// Datasheet `"LinearPhase EQ 85 ms"`: one uniform convolver of the
/// FIR's length (4 081 taps at 48 kHz, 16 partitions of 256). A band
/// change costs one redesign on the next tick (an inverse transform of
/// eight times the length, and the partitions' transforms).
///
/// # Aliasing
///
/// None (linear).
///
/// # Valid ranges
///
/// The [`Band`]s' ranges (corners inside (1 Hz, 0.499·fs)); `length` from
/// 1 ms, and long enough for the bands' ringing: a bell at `f` needs
/// about `10·Q/(π·f)` (a 60 Hz bell at Q 8 misses by 3 dB at 85 ms and
/// needs ~400 ms). Latency `(taps − 1)/2 + 256` samples, reported exactly
/// (2 296 at 48 kHz for the default 85 ms).
///
/// ```
/// use stave_dsp::effects::eq::{Band, LinearPhase};
/// use stave_dsp::units::{db, hz};
/// use stave_dsp::Stage;
///
/// let mut eq = LinearPhase::new(&[
///     Band::LowCut { frequency: hz(40.0), q: 0.707 },
///     Band::Bell { frequency: hz(2_500.0), gain: db(-4.0), q: 2.0 },
/// ]);
/// eq.prepare(48_000.0);
/// // An impulse comes out at the reported latency, the FIR's centre.
/// let latency = eq.latency() as usize;
/// let y: Vec<f32> = (0..latency + 4).map(|n| eq.tick(if n == 0 { 1.0 } else { 0.0 })).collect();
/// let peak = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
/// assert_eq!(peak, latency);
/// ```
#[derive(Clone, Debug)]
pub struct LinearPhase {
    bands: Vec<Band>,
    length: Seconds,
    sample_rate: f32,
    designer: Designer,
    convolver: Convolver,
    /// A band changed; the next tick redesigns.
    pending: bool,
}

impl LinearPhase {
    /// The default FIR length, 85 ms (4 081 taps at 48 kHz): bands
    /// above about 40 Hz within 0.05 dB of the biquads.
    pub const LENGTH: Seconds = ms(85.0);

    /// An EQ of `bands` with the default [`LENGTH`](Self::LENGTH).
    pub fn new(bands: &[Band]) -> Self {
        Self::with_length(bands, Self::LENGTH)
    }

    /// An EQ of `bands` with a FIR `length` long: longer resolves lower
    /// and narrower bands, for more latency.
    pub fn with_length(bands: &[Band], length: Seconds) -> Self {
        let mut eq = Self {
            bands: bands.to_vec(),
            length,
            sample_rate: 0.0,
            designer: Designer::new(3, 0),
            convolver: Convolver::uniform(&ImpulseResponse::mono(&[1.0]), BLOCK, Head::Partitioned),
            pending: false,
        };
        eq.plan(48_000.0);
        eq
    }

    /// The bands.
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// The FIR length, in taps, at the prepared rate.
    pub fn taps(&self) -> usize {
        self.designer.taps32.len()
    }

    /// The designed FIR at the prepared rate (the taps the convolver
    /// runs, or will after a pending change).
    pub fn impulse_response(&mut self) -> &[f32] {
        if self.pending {
            self.redesign();
        }
        &self.designer.taps32
    }

    /// `true` while a band change waits to be designed and swapped in.
    pub fn is_pending(&self) -> bool {
        self.pending || self.convolver.is_swapping()
    }

    /// Replaces band `index`; the next tick redesigns and crossfades to
    /// the new response over one partition. Realtime: never allocates.
    ///
    /// # Panics
    ///
    /// If `index` is out of range.
    pub fn set_band(&mut self, index: usize, band: Band) {
        if self.bands[index] != band {
            self.bands[index] = band;
            self.pending = true;
        }
    }

    /// Plans every buffer for `sample_rate` (this allocates).
    fn plan(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let len = taps_for(self.length, sample_rate);
        self.designer = Designer::new(len, self.bands.len());
        self.redesign();
        self.convolver = Convolver::uniform(
            &ImpulseResponse::mono(&self.designer.taps32),
            BLOCK,
            Head::Partitioned,
        );
        self.convolver.reserve_swaps();
        self.pending = false;
    }

    fn redesign(&mut self) {
        self.designer
            .grid
            .set(self.bands.iter().copied(), self.sample_rate);
        self.designer.design(|grid, k| grid.magnitude(k));
    }
}

impl Stage for LinearPhase {
    /// Re-plans for the rate (this allocates) and clears the state.
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate || self.pending {
            self.plan(sample_rate);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.convolver.reset();
    }

    /// Realtime: never allocates; a pending band change redesigns here.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.pending && !self.convolver.is_swapping() {
            self.redesign();
            self.pending = !self.convolver.swap_response(&self.designer.taps32);
        }
        self.convolver.tick(x)
    }

    /// The FIR's centre plus the convolver's partition.
    fn latency(&self) -> u32 {
        ((self.taps() - 1) / 2) as u32 + self.convolver.latency()
    }

    fn tail(&self) -> Tail {
        self.convolver.tail()
    }
}
