//! The multiband frame: a [`Crossover`] into `N` bands, a stage per band
//! and channel, and the band controls every multiband design needs —
//! solo, mute, bypass, an external sidechain split at the same corners,
//! stereo link, and latency alignment.

use super::compressor::Compressor;
use super::detectors::Link;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::crossover::Crossover;

/// A stage that takes its level from a separate key: a compressor's
/// sidechain. [`Multiband`] keys each band's stage with that band of
/// the key, so linked channels and external sidechains reach every
/// band.
pub trait Keyed: Stage {
    /// Process `x`, deciding the gain from `key`.
    fn tick_keyed(&mut self, x: f32, key: f32) -> f32;
}

impl Keyed for Compressor {
    #[inline]
    fn tick_keyed(&mut self, x: f32, key: f32) -> f32 {
        Compressor::tick_keyed(self, x, key)
    }
}

/// How long solo, mute and bypass take to cross-fade.
const FADE_S: f32 = 0.005;

/// A fixed delay: `push` returns what was pushed `len − 1` calls ago.
#[derive(Clone, Debug, Default)]
struct Line {
    buf: Vec<f32>,
    position: usize,
}

impl Line {
    fn allocate(&mut self, delay: usize) {
        self.buf = vec![0.0; delay + 1];
        self.position = 0;
    }

    #[inline]
    fn push(&mut self, x: f32) -> f32 {
        let len = self.buf.len();
        if len <= 1 {
            return x;
        }
        self.buf[self.position] = x;
        self.position = if self.position + 1 == len { 0 } else { self.position + 1 };
        self.buf[self.position]
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.position = 0;
    }
}

/// A gain gliding linearly to its target.
#[derive(Copy, Clone, Debug)]
struct Fade {
    value: f32,
    target: f32,
}

impl Fade {
    const fn at(v: f32) -> Self {
        Self { value: v, target: v }
    }

    #[inline]
    fn next(&mut self, step: f32) -> f32 {
        let d = self.target - self.value;
        self.value = if d.abs() <= step { self.target } else { self.value + step.copysign(d) };
        self.value
    }
}

/// One band's switches and their fades.
#[derive(Copy, Clone, Debug)]
struct Switches {
    solo: bool,
    mute: bool,
    bypass: bool,
    /// 1 = the stage's output, 0 = the aligned dry band.
    wet: Fade,
    /// 1 = in the mix, 0 = silent.
    level: Fade,
}

/// A multiband frame: an N-band [`Crossover`] whose every band runs its
/// own stage (one per channel), recombined.
///
/// # Models
///
/// The structure of the multiband compressor (Linkwitz–Riley split, a
/// compressor per band, summed back): algorithmic, no hardware claim.
/// The per-band machinery is what stave's multiband-comp plugin carries
/// and generalises past three bands and past compressors: a stage per
/// band and channel (any [`Stage`]; a [`Keyed`] one hears its band of a
/// key), an external sidechain split at the same corners by its own
/// crossover, a stereo [`Link`] applied band by band, and solo, mute and
/// bypass per band.
///
/// # Derivation
///
/// The bands come from [`Crossover`] (Linkwitz–Riley, any order, or
/// linear phase). Each band's stage output is delayed so every band
/// carries the longest band latency: band `i` waits `max − latency_i`
/// samples, and a bypassed band is the dry band delayed by `max`, so
/// bypass keeps the bands aligned and the sum intact. Solo and mute
/// follow multiband-comp: a band sounds when it is not muted and either
/// it is soloed or nothing is. Switches cross-fade over 5 ms; the stages
/// always run, so a band brought back has a warm detector. Stereo link
/// keys both channels' stages of a band with [`Link::apply`] of the two
/// channels' band levels (`|band|`, or `|key band|` with a sidechain),
/// so both get one gain.
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/dynamics/multiband/tests.rs::unity_bands_reconstruct`:
/// with unity stages the output is the crossover's own recombination —
/// flat within 0.002 dB (LR4, 4 bands) and the delayed input within 2e-6
/// (linear phase) — and the reported latency is where the impulse lands,
/// exactly, with a look-ahead limiter in one band.
/// `crates/stave-dsp/src/effects/dynamics/multiband/tests.rs::a_band_compresses_alone`:
/// the output is the crossover's bands through three lone compressors
/// within 1e-6; a loud tone squashes its band by more than 15 dB and
/// leaves a quiet tone in another band within 0.01 dB of untouched.
/// `crates/stave-dsp/src/effects/dynamics/multiband/tests.rs::solo_mute_and_bypass`
/// and `crates/stave-dsp/src/effects/dynamics/multiband/tests.rs::link_and_sidechain_reach_every_band`;
/// no allocation after `prepare`
/// (`crates/stave-dsp/tests/realtime_effects.rs::effects_never_allocate_after_prepare`).
///
/// # Cost
///
/// `"Multiband (4 bands LR4, compressors)"` on the datasheet, mono; the
/// crossover's own lines for the split.
///
/// # Aliasing
///
/// The crossover is linear; the band stages alias as they do alone.
///
/// # Valid ranges
///
/// The [`Crossover`]'s. Band latencies are read at `prepare`; prepare
/// again after changing one. Feedback compressors detect their own
/// output and ignore the key, so link and sidechain need feed-forward.
///
/// ```
/// use stave_dsp::effects::dynamics::{Compressor, GainComputer, Multiband, Topology};
/// use stave_dsp::primitives::filters::crossover::{Crossover, Order};
/// use stave_dsp::units::hz;
/// use stave_dsp::Stage;
///
/// let split = Crossover::<3>::linkwitz_riley(Order::Lr4, &[hz(200.0), hz(2_000.0)]);
/// let curve = GainComputer { threshold_db: -24.0, ratio: 4.0, knee_db: 6.0 };
/// let mut mb = Multiband::new(split, |_band| Compressor::new(curve, Topology::FeedForward));
/// mb.prepare(48_000.0);
/// mb.set_solo(1, true); // hear the mids alone
/// let (l, r) = mb.tick_stereo_linked(0.3, 0.2);
/// assert!(l.is_finite() && r.is_finite());
/// ```
#[derive(Clone, Debug)]
pub struct Multiband<S, const N: usize> {
    split: [Crossover<N>; 2],
    key_split: [Crossover<N>; 2],
    keyed: bool,
    stages: [[S; 2]; N],
    aligned: [[Line; 2]; N],
    dry: [[Line; 2]; N],
    switches: [Switches; N],
    link: Link,
    step: f32,
    latency: u32,
}

impl<S: Stage, const N: usize> Multiband<S, N> {
    /// `crossover`'s bands, band `i` running `stage(i)` on each channel.
    pub fn new(crossover: Crossover<N>, mut stage: impl FnMut(usize) -> S) -> Self {
        let off = Switches {
            solo: false,
            mute: false,
            bypass: false,
            wet: Fade::at(1.0),
            level: Fade::at(1.0),
        };
        Self {
            split: [crossover.clone(), crossover.clone()],
            key_split: [crossover.clone(), crossover],
            keyed: false,
            stages: core::array::from_fn(|i| [stage(i), stage(i)]),
            aligned: core::array::from_fn(|_| Default::default()),
            dry: core::array::from_fn(|_| Default::default()),
            switches: [off; N],
            link: Link::default(),
            step: 1.0,
            latency: 0,
        }
    }

    /// Band `i`'s stages, left and right (mono runs the left).
    pub fn band(&self, i: usize) -> &[S; 2] {
        &self.stages[i]
    }

    /// Band `i`'s stages, to set (both channels, usually alike).
    pub fn band_mut(&mut self, i: usize) -> &mut [S; 2] {
        &mut self.stages[i]
    }

    /// The crossovers' corners, low to high (`N − 1`).
    pub fn corners(&self) -> [crate::units::Hz; N] {
        self.split[0].corners()
    }

    /// Move corner `k` on every crossover (the key's too).
    pub fn set_corner(&mut self, k: usize, corner: crate::units::Hz) {
        for x in self.split.iter_mut().chain(&mut self.key_split) {
            x.set_corner(k, corner);
        }
    }

    /// How a stereo pair's keys combine ([`Link::Max`] by default).
    pub fn set_link(&mut self, link: Link) {
        self.link = link;
    }

    /// Hear band `i` alone (with any other soloed bands).
    pub fn set_solo(&mut self, i: usize, solo: bool) {
        self.switches[i].solo = solo;
        self.retarget();
    }

    /// Silence band `i`.
    pub fn set_mute(&mut self, i: usize, mute: bool) {
        self.switches[i].mute = mute;
        self.retarget();
    }

    /// Pass band `i` dry (aligned with the processed bands).
    pub fn set_bypass(&mut self, i: usize, bypass: bool) {
        self.switches[i].bypass = bypass;
        self.retarget();
    }

    fn retarget(&mut self) {
        let any_solo = self.switches.iter().any(|s| s.solo);
        for s in &mut self.switches {
            s.level.target = f32::from(u8::from(!s.mute && (s.solo || !any_solo)));
            s.wet.target = f32::from(u8::from(!s.bypass));
        }
    }

    /// One frame: split `x` (and the key, when given, by its own
    /// crossovers), run band `i` of each channel through `run(stage, x,
    /// key)`, align, switch and sum.
    #[inline]
    fn frame(
        &mut self,
        x: [f32; 2],
        key: Option<[f32; 2]>,
        lanes: usize,
        run: impl Fn(&mut S, f32, f32) -> f32,
    ) -> [f32; 2] {
        let mut bands = [[0.0f32; N]; 2];
        let mut keys = [[0.0f32; N]; 2];
        if key.is_some() && !self.keyed {
            for k in &mut self.key_split {
                k.reset();
            }
        }
        self.keyed = key.is_some();
        for c in 0..lanes {
            bands[c] = self.split[c].split(x[c]);
            keys[c] = match key {
                Some(k) => self.key_split[c].split(k[c]),
                None => bands[c],
            };
        }
        let mut out = [0.0f32; 2];
        for i in 0..N {
            let (kl, kr) = if lanes == 2 {
                self.link.apply(keys[0][i].abs(), keys[1][i].abs())
            } else {
                (keys[0][i], 0.0)
            };
            let wet = self.switches[i].wet.next(self.step);
            let level = self.switches[i].level.next(self.step);
            for (c, k) in [kl, kr].into_iter().enumerate().take(lanes) {
                let band = bands[c][i];
                let y = self.aligned[i][c].push(run(&mut self.stages[i][c], band, k));
                let d = self.dry[i][c].push(band);
                out[c] += level * (d + wet * (y - d));
            }
        }
        out
    }

    /// One stereo frame, each channel's stages on their own (no link:
    /// any stage).
    #[inline]
    pub fn tick_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let [l, r] = self.frame([left, right], None, 2, |s, x, _| s.tick(x));
        (l, r)
    }
}

impl<S: Keyed, const N: usize> Multiband<S, N> {
    /// One mono sample keyed by `key` (an external sidechain), split by
    /// the key's own crossover.
    #[inline]
    pub fn tick_keyed(&mut self, x: f32, key: f32) -> f32 {
        self.frame([x, 0.0], Some([key, 0.0]), 1, |s, x, k| s.tick_keyed(x, k))[0]
    }

    /// One stereo frame, each band's two stages keyed by the
    /// [`Link`] of the band's two channels.
    #[inline]
    pub fn tick_stereo_linked(&mut self, left: f32, right: f32) -> (f32, f32) {
        let [l, r] = self.frame([left, right], None, 2, |s, x, k| s.tick_keyed(x, k));
        (l, r)
    }

    /// One stereo frame keyed by an external stereo sidechain, split at
    /// the same corners and linked band by band.
    #[inline]
    pub fn tick_stereo_keyed(
        &mut self,
        left: f32,
        right: f32,
        key_left: f32,
        key_right: f32,
    ) -> (f32, f32) {
        let [l, r] = self.frame(
            [left, right],
            Some([key_left, key_right]),
            2,
            |s, x, k| s.tick_keyed(x, k),
        );
        (l, r)
    }
}

impl<S: Stage, const N: usize> Stage for Multiband<S, N> {
    /// Prepares the crossovers and stages, reads the band latencies and
    /// sizes the alignment (this allocates).
    fn prepare(&mut self, sample_rate: f32) {
        for x in self.split.iter_mut().chain(&mut self.key_split) {
            x.prepare(sample_rate);
        }
        for s in self.stages.iter_mut().flatten() {
            s.prepare(sample_rate);
        }
        let longest = self.stages.iter().map(|b| b[0].latency()).max().unwrap_or(0);
        for i in 0..N {
            let own = self.stages[i][0].latency();
            for c in 0..2 {
                self.aligned[i][c].allocate((longest - own) as usize);
                self.dry[i][c].allocate(longest as usize);
            }
        }
        self.latency = self.split[0].latency() + longest;
        self.step = 1.0 / (FADE_S * sample_rate).max(1.0);
        self.reset();
    }

    fn reset(&mut self) {
        for x in self.split.iter_mut().chain(&mut self.key_split) {
            x.reset();
        }
        for s in self.stages.iter_mut().flatten() {
            s.reset();
        }
        for l in self.aligned.iter_mut().chain(&mut self.dry).flatten() {
            l.reset();
        }
        for s in &mut self.switches {
            s.wet.value = s.wet.target;
            s.level.value = s.level.target;
        }
        self.keyed = false;
    }

    /// One mono sample, each band's stage on its own band.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.frame([x, 0.0], None, 1, |s, x, _| s.tick(x))[0]
    }

    /// The crossover's latency plus the longest band's.
    fn latency(&self) -> u32 {
        self.latency
    }

    fn tail(&self) -> Tail {
        let bands = self
            .stages
            .iter()
            .fold(Tail::None, |t, b| t.max(b[0].tail()))
            .plus(self.latency - self.split[0].latency());
        self.split[0].tail().then(bands)
    }
}

#[cfg(test)]
mod tests;
