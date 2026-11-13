//! A cabinet from captured impulse responses: microphones placed between
//! their captured positions and mixed.

use super::{arrival, shift, SHIFT_HALF_WIDTH};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::spectral::{Convolver, Head, ImpulseResponse};
use crate::units::SampleRate;

/// One captured position of a microphone: where it was (on an axis of
/// the caller's choosing — centimetres from the dust cap's centre,
/// distance from the grille …) and the mono response captured there.
#[derive(Clone, Debug)]
pub struct Position {
    pub at: f32,
    pub ir: ImpulseResponse,
}

impl Position {
    pub fn new(at: f32, ir: ImpulseResponse) -> Self {
        Self { at, ir }
    }
}

/// What is blended between positions.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// Each response as captured, its arrival removed: the blend keeps
    /// the captures' own phase, and a microphone at a captured position
    /// is that capture exactly.
    #[default]
    Captured,
    /// Each response's minimum-phase version (same magnitude), its
    /// arrival kept as a pure delay (Kistler & Wightman's model): blends
    /// without the phase differences between captures.
    MinimumPhase,
}

/// When the microphones arrive.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Timing {
    /// Each microphone at its own (interpolated) arrival: the
    /// differences in distance between microphones stay, and so does
    /// the comb their sum makes.
    #[default]
    AsCaptured,
    /// Every microphone moved to the earliest arrival among all the
    /// captures: summed microphones line up in time.
    Aligned,
}

/// How loud each capture is taken.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// As captured.
    #[default]
    AsCaptured,
    /// Every capture scaled to unit energy, so moving a microphone or a
    /// fader between microphones changes tone, not loudness.
    Matched,
}

/// One microphone, prepared.
#[derive(Clone, Debug)]
struct Mic {
    at: Vec<f32>,
    arrivals: Vec<f64>,
    /// Each position's shape (as captured, or minimum phase), level
    /// applied, and the arrival it carries (its own, or none).
    shapes: Vec<Vec<f64>>,
    offsets: Vec<f64>,
    scratch: Vec<f64>,
    response: Vec<f64>,
    taps: Vec<f32>,
    conv: Option<Box<Convolver>>,
    /// Placement wanted, and the one the convolver holds (`NaN`: none).
    placement: f32,
    applied: f32,
    gain: f32,
    gain_now: f32,
    inverted: bool,
}

/// A cabinet from captured impulse responses: one or more microphones,
/// each with responses captured at positions along an axis, placed
/// between them and mixed.
///
/// # Models
/// Linear, time-invariant convolution with a microphone's response at
/// any placement between its captures, the response interpolated as a
/// delay plus a shape (Kistler & Wightman 1992's minimum-phase-plus-delay
/// decomposition, applied to cabinet captures): the arrival interpolated
/// linearly in placement, the shapes (as captured or minimum phase)
/// blended linearly with their arrivals aligned. Microphones sum with
/// their own gains and polarities, at their own arrivals or aligned.
/// What the captures don't hold — a placement outside them, the way a
/// cone's response really changes between two positions — is not
/// modelled: between captures is an interpolation.
///
/// # Derivation
/// Each response's arrival is its delay against its own minimum-phase
/// version ([`arrival`]): the lag of the peak of their
/// cross-correlation, refined between samples on its windowed-sinc
/// interpolation. A placement's response is the two bracketing shapes,
/// each moved by windowed sinc ([`shift`]) from its own
/// arrival to the placement's (interpolated, or the earliest of all
/// when aligned) and weighted by the placement — at a captured position,
/// a move of nothing, so the capture itself — and it reaches the
/// convolver ([`Convolver::non_uniform`], zero latency) through
/// [`Convolver::swap_response`]'s crossfade.
///
/// # Verified
/// `crates/stave-dsp/src/physical/cabinet/tests.rs`: at a captured
/// position the stage is that capture, −138 dB residual
/// (`a_captured_position_is_its_capture`); arrivals recovered to 0.0064
/// samples (`arrivals_are_recovered`); two microphones of one shape
/// 15 and 13.62 samples apart, one inverted, null to −101 and −75 dB
/// aligned, where as captured they comb at −5 dB
/// (`aligned_microphones_null`); halfway between two captures of one
/// shape 18.5 samples apart the response is that shape at the halfway
/// arrival to −98 dB, where a plain blend is −9 dB
/// (`placement_interpolates_the_arrival`); minimum-phase shapes keep the
/// magnitude within 0.0006 dB (`minimum_phase_shapes_keep_the_magnitude`);
/// matched levels give unit energy (`matched_levels_have_equal_energy`);
/// a placement sweep under a steady tone keeps its largest second
/// difference (`placement_moves_are_click_free`). The arrival needs the
/// whole decay: a capture cut off while still ringing (−44 dB at 3000
/// samples of a 60/s decay) moves its minimum-phase reference, and the
/// null above falls to −44 dB.
///
/// # Cost
/// `"Captured (2 mics, 8k taps)"` on the datasheet: one zero-latency
/// convolver per microphone. A placement move costs two windowed-sinc
/// shifts (32 multiply-adds a tap each) and one transform per partition,
/// once, at the next sample its convolver is not mid-swap — a block's
/// spike, so move microphones at control rate.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// Mono responses, at least one position per microphone, positions in
/// increasing `at`; placements are clamped to the captured range. A
/// response with a recorded rate is resampled to the prepared rate
/// (prepare allocates when the rate changes, as the convolver does).
#[derive(Clone, Debug)]
pub struct Captured {
    sources: Vec<Vec<Position>>,
    shape: Shape,
    timing: Timing,
    level: Level,
    block: usize,
    rate: f32,
    /// The earliest arrival of all the captures (Timing::Aligned's).
    earliest: f64,
    len: usize,
    /// Per microphone: placement, gain and polarity as set (kept across
    /// a re-plan).
    wanted: Vec<(f32, f32, bool)>,
    mics: Vec<Mic>,
    gain_coeff: f32,
}

impl Captured {
    /// Microphones, each its captured positions. Planned (allocated)
    /// at `prepare`.
    ///
    /// # Panics
    ///
    /// When a microphone has no positions, a response is not mono, or
    /// positions are not in increasing `at`.
    pub fn new(mics: Vec<Vec<Position>>) -> Self {
        for (m, positions) in mics.iter().enumerate() {
            assert!(!positions.is_empty(), "Captured: microphone {m} has no positions");
            for (k, p) in positions.iter().enumerate() {
                assert!(p.ir.path(0, 0).is_some() && p.ir.path(0, 1).is_none() && p.ir.path(1, 1).is_none(),
                    "Captured: microphone {m}, position {k} is not a mono response");
                if k > 0 {
                    assert!(p.at > positions[k - 1].at, "Captured: microphone {m}'s positions are not increasing");
                }
            }
        }
        let wanted = mics.iter().map(|ps| (ps[0].at, 1.0, false)).collect();
        Self {
            wanted,
            sources: mics,
            shape: Shape::default(),
            timing: Timing::default(),
            level: Level::default(),
            block: 64,
            rate: 0.0,
            earliest: 0.0,
            len: 0,
            mics: Vec::new(),
            gain_coeff: 1.0,
        }
    }

    /// The shapes blended (before `prepare`: it re-plans).
    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self.rate = 0.0;
        self
    }

    /// How captures are levelled (before `prepare`: it re-plans).
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self.rate = 0.0;
        self
    }

    /// The convolvers' smallest partition (a power of two; before
    /// `prepare`).
    pub fn with_block(mut self, block: usize) -> Self {
        self.block = block.max(1).next_power_of_two();
        self.rate = 0.0;
        self
    }

    /// Microphones.
    pub fn mics(&self) -> usize {
        self.sources.len()
    }

    /// Microphone timing. Realtime-safe: every microphone's response is
    /// re-placed at its next free sample.
    pub fn set_timing(&mut self, timing: Timing) {
        if timing != self.timing {
            self.timing = timing;
            for m in &mut self.mics {
                m.applied = f32::NAN;
            }
        }
    }

    /// Place microphone `mic` at `at` on its positions' axis (clamped to
    /// the captured range). Realtime-safe.
    pub fn set_placement(&mut self, mic: usize, at: f32) {
        let Some(ps) = self.sources.get(mic) else { return };
        let at = at.clamp(ps[0].at, ps[ps.len() - 1].at);
        self.wanted[mic].0 = at;
        if let Some(m) = self.mics.get_mut(mic) {
            m.placement = at;
        }
    }

    /// Microphone `mic`'s level (linear) and polarity; levels glide over
    /// 10 ms. Realtime-safe.
    pub fn set_gain(&mut self, mic: usize, gain: f32, inverted: bool) {
        if let Some(w) = self.wanted.get_mut(mic) {
            *w = (w.0, gain, inverted);
        }
        if let Some(m) = self.mics.get_mut(mic) {
            m.gain = gain;
            m.inverted = inverted;
        }
    }

    /// Microphone `mic`'s arrival at its current placement, samples at
    /// the prepared rate (as captured, before any alignment).
    pub fn arrival(&self, mic: usize) -> f64 {
        self.mics.get(mic).map_or(0.0, |m| {
            let (k, g) = bracket(&m.at, m.placement);
            interpolate(&m.arrivals, k, g)
        })
    }

    /// Microphone `mic`'s captured arrivals, samples at the prepared rate.
    pub fn arrivals(&self, mic: usize) -> &[f64] {
        self.mics.get(mic).map_or(&[], |m| &m.arrivals)
    }

    /// Whether any microphone's response is still changing.
    pub fn moving(&self) -> bool {
        self.mics.iter().any(|m| m.conv.as_ref().is_some_and(|c| c.is_swapping()) || !same(m.placement, m.applied))
    }

    /// Plan every microphone at `rate` (allocates).
    fn plan(&mut self, rate: f32) {
        let sr = SampleRate::new(rate);
        let at_rate = |ir: &ImpulseResponse| -> Vec<f32> {
            let ir = if ir.sample_rate().is_some() { ir.resampled(sr) } else { ir.clone() };
            ir.path(0, 0).unwrap_or(&[]).to_vec()
        };
        let responses: Vec<Vec<Vec<f32>>> =
            self.sources.iter().map(|ps| ps.iter().map(|p| at_rate(&p.ir)).collect()).collect();
        let arrivals: Vec<Vec<f64>> =
            responses.iter().map(|ps| ps.iter().map(|h| arrival(h)).collect()).collect();
        let latest = arrivals.iter().flatten().fold(0.0f64, |m, &a| m.max(a));
        self.earliest = arrivals.iter().flatten().fold(f64::INFINITY, |m, &a| m.min(a));
        let longest = responses.iter().flatten().map(Vec::len).max().unwrap_or(1);
        // Room for the widest move between arrivals and the sinc's reach.
        self.len = longest + (latest - self.earliest).ceil() as usize + 2 * SHIFT_HALF_WIDTH;
        let mut mics = Vec::with_capacity(responses.len());
        for (m, (ps, arr)) in responses.iter().zip(&arrivals).enumerate() {
            let mut shapes = Vec::with_capacity(ps.len());
            let mut offsets = Vec::with_capacity(ps.len());
            for (h, &a) in ps.iter().zip(arr) {
                let (shape, offset): (Vec<f64>, f64) = match self.shape {
                    Shape::Captured => (h.iter().map(|&v| f64::from(v)).collect(), a),
                    Shape::MinimumPhase => {
                        let min = ImpulseResponse::mono(h).minimum_phase();
                        (min.path(0, 0).unwrap_or(&[]).iter().map(|&v| f64::from(v)).collect(), 0.0)
                    }
                };
                let scale = match self.level {
                    Level::AsCaptured => 1.0,
                    Level::Matched => {
                        1.0 / h.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt().max(1e-30)
                    }
                };
                shapes.push(shape.into_iter().map(|v| v * scale).collect());
                offsets.push(offset);
            }
            let at: Vec<f32> = self.sources[m].iter().map(|p| p.at).collect();
            let (placement, gain, inverted) = self.wanted[m];
            mics.push(Mic {
                at,
                arrivals: arr.clone(),
                offsets,
                shapes,
                scratch: vec![0.0; self.len],
                response: vec![0.0; self.len],
                taps: vec![0.0; self.len],
                conv: None,
                placement,
                applied: f32::NAN,
                gain,
                gain_now: gain,
                inverted,
            });
        }
        self.mics = mics;
        self.rate = rate;
        // Each convolver starts with its microphone in place, no fade.
        let largest = (self.block * 64).max(4096);
        for i in 0..self.mics.len() {
            self.respond(i);
            let m = &mut self.mics[i];
            let mut conv = Convolver::non_uniform(&ImpulseResponse::mono(&m.taps), self.block, largest, Head::Direct);
            conv.prepare(rate);
            conv.reserve_swaps();
            m.conv = Some(Box::new(conv));
            m.applied = m.placement;
        }
    }

    /// Microphone `i`'s response at its placement, into its taps:
    /// `Σ gₖ·shapeₖ` with each shape moved from its own arrival to the
    /// placement's.
    fn respond(&mut self, i: usize) {
        let (timing, earliest) = (self.timing, self.earliest);
        let m = &mut self.mics[i];
        let (k, g) = bracket(&m.at, m.placement);
        let target = match timing {
            Timing::AsCaptured => interpolate(&m.arrivals, k, g),
            Timing::Aligned => earliest,
        };
        let k1 = (k + 1).min(m.shapes.len() - 1);
        let g = f64::from(g);
        m.response.iter_mut().for_each(|v| *v = 0.0);
        for (j, w) in [(k, 1.0 - g), (k1, g)] {
            if w == 0.0 {
                continue;
            }
            shift(&m.shapes[j], target - m.offsets[j], &mut m.scratch);
            for (r, &s) in m.response.iter_mut().zip(&m.scratch) {
                *r += w * s;
            }
        }
        for (t, &r) in m.taps.iter_mut().zip(&m.response) {
            *t = r as f32;
        }
    }

    /// Re-place microphone `i` through its convolver's crossfade; `false`
    /// while the convolver is still mid-swap.
    fn place(&mut self, i: usize) -> bool {
        if self.mics[i].conv.as_ref().is_some_and(|c| c.is_swapping()) {
            return false;
        }
        self.respond(i);
        let m = &mut self.mics[i];
        m.applied = m.placement;
        m.conv.as_mut().is_some_and(|c| c.swap_response(&m.taps))
    }
}

/// The position below `at` and the fraction of the way to the next.
fn bracket(at: &[f32], x: f32) -> (usize, f32) {
    if at.len() < 2 || x <= at[0] {
        return (0, 0.0);
    }
    for k in 0..at.len() - 1 {
        if x <= at[k + 1] {
            return (k, (x - at[k]) / (at[k + 1] - at[k]));
        }
    }
    (at.len() - 1, 0.0)
}

fn interpolate(v: &[f64], k: usize, g: f32) -> f64 {
    let k1 = (k + 1).min(v.len() - 1);
    (1.0 - f64::from(g)) * v[k] + f64::from(g) * v[k1]
}

/// Equal, or both unset.
fn same(a: f32, b: f32) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

impl Stage for Captured {
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.rate {
            self.plan(sample_rate);
        }
        self.gain_coeff = 1.0 - (-1.0 / (0.010 * sample_rate)).exp();
        self.reset();
    }

    fn reset(&mut self) {
        for m in &mut self.mics {
            if let Some(c) = &mut m.conv {
                c.reset();
            }
            m.gain_now = if m.inverted { -m.gain } else { m.gain };
        }
    }

    /// One sample through every microphone, summed.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = 0.0;
        for i in 0..self.mics.len() {
            if !same(self.mics[i].placement, self.mics[i].applied) {
                self.place(i);
            }
            let k = self.gain_coeff;
            let m = &mut self.mics[i];
            let want = if m.inverted { -m.gain } else { m.gain };
            m.gain_now += k * (want - m.gain_now);
            y += m.gain_now * m.conv.as_mut().map_or(0.0, |c| c.tick(x));
        }
        y
    }

    fn latency(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::Samples(self.len as u32)
    }
}
