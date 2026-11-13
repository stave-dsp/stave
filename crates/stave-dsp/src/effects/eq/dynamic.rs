//! The dynamic EQ: bands whose gain follows the level in their own
//! frequency range, each minimum- or linear-phase.

use super::band::{coefficients, Band};
use super::linear_phase::{taps_for, Designer, BLOCK};
use super::section::Section;
use crate::effects::dynamics::{Compressor, GainComputer, Rms, Topology};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::svf::Svf;
use crate::primitives::spectral::{Convolver, Head, ImpulseResponse};
use crate::units::{Db, Ratio, Seconds};

/// How a band is realised.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// A trapezoidal state-variable section: no latency, the phase
    /// turns around the band, and the gain moves every sample.
    #[default]
    Minimum,
    /// Part of the EQ's linear-phase FIRs: exactly linear phase, the
    /// EQ's latency, pre-ringing (see [`super::LinearPhase`]).
    Linear,
}

/// What a dynamic band's detector measures, before the gain computer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Detector {
    /// The rectified band signal, every sample (D1's peak detector with
    /// zero times: the attack and release smooth the gain in dB, as
    /// Giannoulis et al. recommend).
    Peak,
    /// The band's RMS over a window (D1's windowed [`Rms`]).
    Rms(Seconds),
}

/// A band's dynamics: as the level in the band rises past `threshold`,
/// the gain computer's reduction (D2) moves the band's gain from its
/// static gain toward `range` — down for a negative range
/// (compressing the band), up for a positive one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Dynamics {
    /// Where the band's level starts to move the gain.
    pub threshold: Db,
    /// The gain computer's ratio above the threshold.
    pub ratio: Ratio,
    /// The gain computer's knee width (0 is hard).
    pub knee: Db,
    /// The furthest the gain moves from the band's static gain, signed.
    pub range: Db,
    /// The time constant of the gain moving toward `range` (at least
    /// 0.5 ms, so a step never clicks).
    pub attack: Seconds,
    /// The time constant of its return.
    pub release: Seconds,
    /// What the detector measures.
    pub detector: Detector,
}

/// One band of a [`Dynamic`] EQ.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DynamicBand {
    /// The band at its static gain (the gain with no dynamics acting).
    pub band: Band,
    /// Minimum or linear phase.
    pub phase: Phase,
    /// The dynamics, or `None` for a static band. Cuts have no gain to
    /// move: their dynamics are ignored.
    pub dynamics: Option<Dynamics>,
}

impl DynamicBand {
    /// A static band.
    pub fn fixed(band: Band, phase: Phase) -> Self {
        Self {
            band,
            phase,
            dynamics: None,
        }
    }

    /// A band with `dynamics`.
    pub fn dynamic(band: Band, phase: Phase, dynamics: Dynamics) -> Self {
        Self {
            band,
            phase,
            dynamics: Some(dynamics),
        }
    }

    /// The dynamics that apply: none on a cut.
    fn active(&self) -> Option<Dynamics> {
        self.band.gain().and(self.dynamics)
    }
}

/// The attack's floor: a gain step faster than this would click.
const MIN_TIME_S: f64 = 5e-4;

/// The level detector a band runs.
#[derive(Clone, Debug)]
enum Level {
    Peak,
    Rms(Rms),
}

/// The running state of one band: its key filter, detector and gain
/// computer, and the gain it is at.
#[derive(Clone, Debug)]
struct BandState {
    key: Svf,
    /// The key's normalisation: `1/Q` for the bell's band-pass, 1 for the
    /// shelves' low- and high-pass.
    key_gain: f32,
    level: Level,
    computer: Compressor,
    /// The gain's offset from static, dB, as the computer last set it.
    offset_db: f32,
    /// The minimum-phase realisation (also built for linear bands, unused).
    section: Section,
    rate: f32,
}

/// A linear-phase dynamic band's FIR and its gain path.
#[derive(Clone, Debug)]
struct LinearBand {
    band: usize,
    convolver: Convolver,
    /// The static and far-end linear gains the FIR interpolates between.
    from: f32,
    to: f32,
    /// The interpolation weight, delayed by the EQ's latency so it meets
    /// the audio it was detected on.
    delay: Vec<f32>,
}

/// A dynamic EQ: parametric bands, each static or with [`Dynamics`],
/// each [`Phase::Minimum`] or [`Phase::Linear`].
///
/// # Models
///
/// Per dynamic band, a sidechain (the band's own region of the input: a
/// band-pass at a bell's centre and Q, a low- or high-pass at a shelf's
/// corner), a D1 detector, D2's gain computer with its attack and
/// release in dB (the [`Compressor`]'s log-domain smoothing), and the
/// band's gain moved by the result within its range. No hardware claim.
/// Minimum-phase bands run first, in series, as trapezoidal SVF
/// sections whose gain changes every sample; the linear-phase bands
/// follow as FIRs: one for the static linear bands together, and one
/// per dynamic linear band holding the change from its static gain to
/// the far end of its range, weighted by where the gain is,
///
/// `H = S + Σ_b c_b(t)·S·(|H_b(g_far)|/|H_b(g_static)| − 1)`,
///
/// `c_b = (g_b − g_static)/(g_far − g_static)` in linear gain, so a
/// linear band is exact at its static gain, at the end of its range and
/// at its centre for every gain, and close between (two linear bands
/// that overlap add their changes rather than multiplying them).
///
/// # Derivation
///
/// Giannoulis, Massberg & Reiss 2012 for the detector and gain
/// computer (D1, D2); Simper's linear trapezoidal SVF (2013) for the
/// minimum-phase bands, which are the cookbook biquads' responses with
/// a modulation-safe structure; frequency sampling (F6) and a uniform
/// [`Convolver`] (F2) for the linear bands, their gain weights delayed
/// by the latency so detection and audio stay aligned. A band change
/// redesigns the linear FIRs and crossfades to them over one partition.
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/eq/tests.rs::dynamic_bands_match_the_biquads_at_rest`:
/// held below threshold, both phases match the cookbook cascade from
/// 40 Hz to 20 kHz (minimum phase within 1e-4 dB, worst 4e-5; linear
/// within 0.05, worst 0.011); a minimum-phase section at gains from −15
/// to +12 dB matches its biquad within 1e-4 dB.
/// `crates/stave-dsp/src/effects/eq/tests.rs::dynamic_bands_reach_their_static_curve`:
/// a held tone at the band's centre settles to the gain computer's
/// curve within 0.1 dB (measured 0.001), both phases, with a static gain
/// under it.
/// `crates/stave-dsp/src/effects/eq/tests.rs::dynamic_bands_do_not_click`:
/// 34 dB tone bursts through a band at a 3 ms attack (RMS detector) leave
/// the spray beyond ±1 kHz of the tone at −63 dB (minimum) and −69 dB
/// (linear) re the tone, budget −60. A peak detector's ripple at twice
/// the tone modulates the gain at audio rate: distortion by design, as
/// in any fast compressor, not a click.
///
/// # Cost
///
/// Datasheet `"Dynamic EQ (3 min + 1 linear band)"` and `"Dynamic EQ
/// (4 minimum-phase bands)"`: a section, an SVF key, a detector and a
/// gain computer per minimum band; one convolver per dynamic linear
/// band plus one for the static linear bands.
///
/// # Aliasing
///
/// None from the filters (linear); the moving gain modulates, as any
/// compressor's does.
///
/// # Valid ranges
///
/// The [`Band`]s' ranges; [`Dynamics`] with a ratio ≥ 1 and times ≥ 0
/// (attack and release floored at 0.5 ms). Latency 0 when every band is
/// minimum-phase, else `(taps − 1)/2 + 256` samples (the FIR `length`,
/// 85 ms by default), fixed at construction.
///
/// ```
/// use stave_dsp::effects::eq::{Band, Detector, Dynamic, DynamicBand, Dynamics, Phase};
/// use stave_dsp::units::{db, hz, ms, Ratio};
/// use stave_dsp::Stage;
///
/// // A de-esser: 6 kHz pulled down by up to 8 dB when it gets loud.
/// let ess = Dynamics {
///     threshold: db(-30.0),
///     ratio: Ratio::new(4.0),
///     knee: db(6.0),
///     range: db(-8.0),
///     attack: ms(1.0),
///     release: ms(60.0),
///     detector: Detector::Peak,
/// };
/// let band = Band::Bell { frequency: hz(6_000.0), gain: db(0.0), q: 2.0 };
/// let mut eq = Dynamic::new(&[DynamicBand::dynamic(band, Phase::Minimum, ess)]);
/// eq.prepare(48_000.0);
/// assert_eq!(eq.latency(), 0);
/// for n in 0..4_800 {
///     eq.tick(0.5 * (n as f32 * core::f32::consts::TAU * 6_000.0 / 48_000.0).sin());
/// }
/// assert!(eq.gain_offset(0).get() < -6.0);
/// ```
#[derive(Clone, Debug)]
pub struct Dynamic {
    bands: Vec<DynamicBand>,
    length: Seconds,
    sample_rate: f32,
    states: Vec<BandState>,
    /// The linear section, when any band is linear-phase.
    linear: Option<Linear>,
}

/// The linear-phase FIRs: the static one and one per dynamic linear
/// band, sharing one designer.
#[derive(Clone, Debug)]
struct Linear {
    designer: Designer,
    fixed: Convolver,
    bands: Vec<LinearBand>,
    /// The latency, samples.
    latency: usize,
    /// Position in the weight delay lines (`latency + 1` long).
    pos: usize,
    /// A band changed: redesign when no swap is under way.
    pending: bool,
}

impl Dynamic {
    /// An EQ of `bands`, linear-phase FIRs (if any band is linear)
    /// 85 ms long ([`LinearPhase::LENGTH`](super::LinearPhase::LENGTH)).
    pub fn new(bands: &[DynamicBand]) -> Self {
        Self::with_length(bands, super::LinearPhase::LENGTH)
    }

    /// The same with the linear-phase FIRs `length` long.
    pub fn with_length(bands: &[DynamicBand], length: Seconds) -> Self {
        let mut eq = Self {
            bands: bands.to_vec(),
            length,
            sample_rate: 0.0,
            states: Vec::new(),
            linear: None,
        };
        eq.plan(48_000.0);
        eq
    }

    /// The bands.
    pub fn bands(&self) -> &[DynamicBand] {
        &self.bands
    }

    /// How far band `index`'s gain is from its static gain now (0 for a
    /// static band), for a meter.
    pub fn gain_offset(&self, index: usize) -> Db {
        Db::new(self.states[index].offset_db)
    }

    /// Replaces band `index`'s shape, static gain or dynamics; its phase
    /// stays what it was built with (the latency is fixed). A linear
    /// band's change redesigns the FIRs on the next tick and crossfades
    /// over one partition; a minimum-phase band's gain follows at once,
    /// its shape too (the section keeps its state). Realtime: never
    /// allocates.
    ///
    /// # Panics
    ///
    /// If `index` is out of range.
    pub fn set_band(&mut self, index: usize, band: Band, dynamics: Option<Dynamics>) {
        let phase = self.bands[index].phase;
        let new = DynamicBand {
            band,
            phase,
            dynamics,
        };
        if new == self.bands[index] {
            return;
        }
        let shape_changed = self.bands[index].band != band;
        self.bands[index] = new;
        let rate = self.sample_rate;
        let state = &mut self.states[index];
        if shape_changed {
            let (ic1, ic2) = state.section.state();
            state.section = Section::new(band, rate);
            state.section.restore(ic1, ic2);
            let (key, key_gain) = key_filter(band);
            state.key.set(key.0, key.1);
            state.key_gain = key_gain;
        }
        if let Some(d) = new.active() {
            configure(&mut state.computer, d);
        } else {
            state.offset_db = 0.0;
        }
        if phase == Phase::Linear {
            if let Some(linear) = self.linear.as_mut() {
                linear.pending = true;
            }
        }
    }

    /// Plans everything for `sample_rate` (this allocates).
    fn plan(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.states = self
            .bands
            .iter()
            .map(|b| BandState::new(b, sample_rate))
            .collect();
        let linear: Vec<usize> = (0..self.bands.len())
            .filter(|&i| self.bands[i].phase == Phase::Linear)
            .collect();
        self.linear = if linear.is_empty() {
            None
        } else {
            let len = taps_for(self.length, sample_rate);
            let latency = (len - 1) / 2 + BLOCK;
            let convolver = |taps: &[f32]| {
                let mut c = Convolver::uniform(&ImpulseResponse::mono(taps), BLOCK, Head::Partitioned);
                c.reserve_swaps();
                c
            };
            let designer = Designer::new(len, linear.len());
            let fixed = convolver(&designer.taps32);
            let bands = linear
                .iter()
                .filter(|&&i| self.bands[i].active().is_some())
                .map(|&i| LinearBand {
                    band: i,
                    convolver: convolver(&designer.taps32),
                    from: 1.0,
                    to: 1.0,
                    delay: vec![0.0; latency + 1],
                })
                .collect();
            let mut section = Linear {
                designer,
                fixed,
                bands,
                latency,
                pos: 0,
                pending: false,
            };
            section.redesign(&self.bands, sample_rate, true);
            Some(section)
        };
    }
}

/// The key filter for a band: `(frequency, q)` of the SVF and the
/// normalisation of its tap.
fn key_filter(band: Band) -> ((f32, f32), f32) {
    match band {
        Band::Bell { frequency, q, .. } => ((frequency.get(), q), 1.0 / q.max(1e-3)),
        other => ((other.frequency().get(), super::band::SHELF_Q), 1.0),
    }
}

/// Sets a gain computer's curve and times from `d`.
fn configure(computer: &mut Compressor, d: Dynamics) {
    computer.set_computer(GainComputer {
        threshold_db: d.threshold.get(),
        ratio: d.ratio.get(),
        knee_db: d.knee.get(),
    });
    computer.set_attack_ms((d.attack.get().max(MIN_TIME_S) * 1e3) as f32);
    computer.set_release_ms((d.release.get().max(MIN_TIME_S) * 1e3) as f32);
}

impl BandState {
    fn new(b: &DynamicBand, sample_rate: f32) -> Self {
        let ((hz, q), key_gain) = key_filter(b.band);
        let mut key = Svf::new(hz, q);
        key.prepare(sample_rate);
        let mut computer = Compressor::new(
            GainComputer {
                threshold_db: 0.0,
                ratio: 1.0,
                knee_db: 0.0,
            },
            Topology::FeedForward,
        );
        computer.prepare(sample_rate);
        let level = match b.dynamics.map(|d| d.detector) {
            Some(Detector::Rms(window)) => {
                let mut rms = Rms::new((window.get() * 1e3) as f32);
                rms.prepare(sample_rate);
                Level::Rms(rms)
            }
            _ => Level::Peak,
        };
        if let Some(d) = b.active() {
            configure(&mut computer, d);
        }
        Self {
            key,
            key_gain,
            level,
            computer,
            offset_db: 0.0,
            section: Section::new(b.band, sample_rate),
            rate: sample_rate,
        }
    }

    fn reset(&mut self) {
        self.key.reset();
        if let Level::Rms(rms) = &mut self.level {
            rms.reset();
        }
        self.computer.reset();
        self.offset_db = 0.0;
        self.section.reset();
    }

    /// The gain the band is at: static plus the dynamics' offset, dB.
    #[inline]
    fn target_db(&self, band: &DynamicBand) -> f32 {
        band.band.gain().map_or(0.0, |g| g.get()) + self.offset_db
    }

    /// The section's ring-out at the gain it is at (a band's tail).
    fn tail(&self, band: &DynamicBand) -> Tail {
        let mut b = crate::primitives::filters::biquad::Biquad::new(band.band.spec());
        b.prepare(self.rate);
        b.tail()
    }

    /// Detects `x` (the EQ's input) and moves the offset.
    #[inline]
    fn detect(&mut self, band: &DynamicBand, x: f32) {
        let Some(d) = band.active() else {
            return;
        };
        let taps = self.key.taps(x);
        let key = match band.band {
            Band::Bell { .. } => taps.bp * self.key_gain,
            Band::LowShelf { .. } => taps.lp,
            _ => taps.hp,
        };
        let level = match &mut self.level {
            Level::Peak => key.abs(),
            Level::Rms(rms) => rms.tick(key),
        };
        self.computer.tick_keyed(0.0, level);
        let r = self.computer.reduction_db();
        let range = d.range.get();
        self.offset_db = if range < 0.0 { (-r).max(range) } else { r.min(range) };
    }
}

impl Linear {
    /// Redesigns every FIR from `bands` and swaps them in (with
    /// `install`, at once: for a plan, where nothing is playing).
    fn redesign(&mut self, bands: &[DynamicBand], sample_rate: f32, install: bool) {
        let linear = || bands.iter().filter(|b| b.phase == Phase::Linear);
        self.designer.grid.set(linear().map(|b| b.band), sample_rate);
        // The static FIR: the linear bands' product.
        self.designer.design(|grid, k| grid.magnitude(k));
        let swapped = self.fixed.swap_response(&self.designer.taps32);
        debug_assert!(swapped, "a redesign waits for the last swap to finish");
        if install {
            self.fixed.reset();
        }
        // Each dynamic band: S·(|H_b(far)|/|H_b(static)| − 1), its
        // weight running from 0 at the static gain to 1 at the far end.
        for lb in &mut self.bands {
            let b = bands[lb.band];
            let Some(d) = b.active() else { continue };
            let slot = linear().take_while(|o| !core::ptr::eq(*o, &bands[lb.band])).count();
            let static_db = b.band.gain().map_or(0.0, |g| g.get());
            let far_db = static_db + d.range.get();
            lb.from = 10f32.powf(static_db / 20.0);
            lb.to = 10f32.powf(far_db / 20.0);
            let far = coefficients(b.band.with_gain(Db::new(far_db)), sample_rate);
            self.designer.design(|grid, k| {
                let at_static = grid.section(slot, k).max(1e-12);
                grid.magnitude(k) * (grid.magnitude_of(far, k) / at_static - 1.0)
            });
            let swapped = lb.convolver.swap_response(&self.designer.taps32);
            debug_assert!(swapped, "a redesign waits for the last swap to finish");
            if install {
                lb.convolver.reset();
            }
        }
        self.pending = false;
    }

    fn delay_len(&self) -> usize {
        self.latency + 1
    }

    fn swapping(&self) -> bool {
        self.fixed.is_swapping() || self.bands.iter().any(|b| b.convolver.is_swapping())
    }

    fn reset(&mut self) {
        self.fixed.reset();
        for b in &mut self.bands {
            b.convolver.reset();
            b.delay.fill(0.0);
        }
        self.pos = 0;
    }
}

impl Stage for Dynamic {
    /// Re-plans for the rate (this allocates) and clears the state.
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.plan(sample_rate);
        }
        self.reset();
    }

    fn reset(&mut self) {
        for s in &mut self.states {
            s.reset();
        }
        if let Some(linear) = self.linear.as_mut() {
            if linear.pending {
                // Nothing is playing: design and install at once (a
                // reset ends any swap under way).
                linear.reset();
                linear.redesign(&self.bands, self.sample_rate, true);
            }
            linear.reset();
        }
    }

    /// Realtime: never allocates; a pending linear band change
    /// redesigns here.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = x;
        for (state, band) in self.states.iter_mut().zip(&self.bands) {
            state.detect(band, x);
            if band.phase == Phase::Minimum {
                state.section.set_gain_db(state.target_db(band));
                y = state.section.tick(y);
            }
        }
        let Some(linear) = self.linear.as_mut() else {
            return y;
        };
        if linear.pending && !linear.swapping() {
            linear.redesign(&self.bands, self.sample_rate, false);
        }
        let mut out = linear.fixed.tick(y);
        let len = linear.delay_len();
        let pos = linear.pos;
        for lb in &mut linear.bands {
            let state = &self.states[lb.band];
            let g = 10f32.powf(state.target_db(&self.bands[lb.band]) / 20.0);
            let span = lb.to - lb.from;
            lb.delay[pos] = if span.abs() > 1e-9 { (g - lb.from) / span } else { 0.0 };
            let weight = lb.delay[(pos + 1) % len];
            out += weight * lb.convolver.tick(y);
        }
        linear.pos = (pos + 1) % len;
        out
    }

    fn latency(&self) -> u32 {
        self.linear.as_ref().map_or(0, |l| l.latency as u32)
    }

    fn tail(&self) -> Tail {
        let sections = self
            .states
            .iter()
            .zip(&self.bands)
            .filter(|(_, b)| b.phase == Phase::Minimum)
            .fold(Tail::None, |acc, (s, b)| acc.then(s.tail(b)));
        match &self.linear {
            Some(l) => sections.then(l.fixed.tail()),
            None => sections,
        }
    }
}
