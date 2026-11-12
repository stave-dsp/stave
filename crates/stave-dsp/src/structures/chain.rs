//! `Chain` — runtime-addressable serial composition of [`Stage`]s.
//!
//! The container for nonlinearity ⊗ filter ⊗ nonlinearity topologies. The interstage filters are the part no single curve can
//! absorb (distorting a filtered signal ≠ filtering distortion), and
//! they're where "sounds like an amp channel" comes from: the measured
//! G(frequency, level) surface of a chain BENDS — gain compression that
//! depends on frequency — while any memoryless curve's surface is flat
//! across frequency by definition.
//!
//! Usage: build at construction / block rate; `Chain` is a `Stage`, so it
//! nests in any form — typically `Oversampled2x::front(chain)` so its
//! nonlinearities run at 2×. Fixed capacity, no audio-thread allocation.

use crate::kernel::control::Full;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};

pub struct Chain<const N: usize> {
    stages: [StageKind; N],
    /// Sense endpoints: the output at every join, index-keyed
    /// by `StageId` and enumerated as data by [`topology`]. This is
    /// the ONE surface interior signals are read from — control
    /// routes (global feedback via `set_feedback`, detector inputs to
    /// modulation and feedback compressors) and observation feeds
    /// (the topology strip) alike; nothing reaches into stages. The
    /// unconditional store IS the cheap transport — declaration-
    /// gating the publish side is the observation plane's job (taps
    /// are zero-cost unattached), not the structure's.
    ///
    /// [`topology`]: Chain::topology
    senses: [f32; N],
    used: usize,
    // Global feedback: `fb_gain × filtered(tap[fb_from])`,
    // one tick old, injected at stage `fb_to`'s input. Negative gain =
    // classic NFB (linearises low-drive THD, adds the feedback-amp edge
    // at clip); the optional loop filter shapes it (presence = HF-tilted
    // NFB). Run at 4×+ inner rate so the one-sample loop delay's phase
    // error stays small in-band; the conformance stability probe pins the
    // safe gain range — knobs must be clamped to it.
    fb_gain: f32,
    fb_from: StageId,
    fb_to: StageId,
    fb_last: f32,
    fb_filter: Option<Biquad>,
    sample_rate: f32,
}

/// Handle for block-rate parameter access to a pushed stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageId(pub usize);

impl<const N: usize> Default for Chain<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Chain<N> {
    pub fn new() -> Self {
        Self {
            stages: core::array::from_fn(|_| StageKind::Bypass),
            senses: [0.0; N],
            used: 0,
            fb_gain: 0.0,
            fb_from: StageId(N),
            fb_to: StageId(N),
            fb_last: 0.0,
            fb_filter: None,
            sample_rate: 48_000.0,
        }
    }

    /// Append a stage (build time / block rate) and return its handle;
    /// [`Full`] when all `N` slots are taken.
    pub fn push(&mut self, stage: StageKind) -> Result<StageId, Full> {
        let slot = self.stages.get_mut(self.used).ok_or(Full)?;
        *slot = stage;
        self.used += 1;
        Ok(StageId(self.used - 1))
    }

    /// The rate the chain was last prepared at.
    pub const fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Block-rate access for retuning a stage's parameters.
    pub fn stage_mut(&mut self, id: StageId) -> Option<&mut StageKind> {
        if id.0 < self.used {
            Some(&mut self.stages[id.0])
        } else {
            None
        }
    }

    /// A join's sense endpoint: the stage's last output (one sample
    /// old at read time from a modulator's perspective — the
    /// documented loop delay).
    #[inline]
    pub fn sense(&self, id: StageId) -> f32 {
        if id.0 < self.used {
            self.senses[id.0]
        } else {
            0.0
        }
    }
    /// The final join's sense endpoint (the post-chain detector point
    /// — a feedback compressor's detector is a route from HERE).
    #[inline]
    pub fn sense_out(&self) -> f32 {
        if self.used > 0 {
            self.senses[self.used - 1]
        } else {
            0.0
        }
    }

    /// Configure global feedback: `gain × filtered(sense[from])`
    /// injected at `to`'s input, one tick delayed — a control route
    /// declared over sense endpoints. `gain = 0` disables. The safe
    /// |gain| range is a MEASURED property (stability probe) — clamp
    /// knobs to it.
    pub fn set_feedback(
        &mut self,
        gain: f32,
        from: StageId,
        to: StageId,
        filter: Option<FilterSpec>,
    ) {
        self.fb_gain = gain;
        self.fb_from = from;
        self.fb_to = to;
        self.fb_filter = filter.map(|spec| {
            let mut f = Biquad::new(spec);
            f.prepare(self.sample_rate);
            f
        });
    }

    /// Global feedback is on: the chain is a loop.
    fn feedback_on(&self) -> bool {
        self.fb_gain != 0.0 && self.fb_to.0 < self.used
    }

    pub fn len(&self) -> usize {
        self.used
    }
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Reflection sidecar v1: the topology as data, read from the
    /// LIVE structure — `(id, kind)` in signal order. Emitted, never
    /// hand-maintained, so it cannot drift from what runs.
    pub fn topology(&self) -> impl Iterator<Item = (StageId, &'static str)> + '_ {
        self.stages[..self.used]
            .iter()
            .enumerate()
            .map(|(i, s)| (StageId(i), s.kind_name()))
    }
}

/// One structural swap point: a chain slot's inactive alternate,
/// parked HERE. The bench holds EXACTLY ONE
/// stage at all times and swaps are moves, so an instance lives in
/// exactly one place (chain XOR bench), a park can never overwrite
/// (no dealloc under the audio guard), and an unpark can never miss.
/// The INSTANCE survives the round trip — its built tables come back
/// without a rebuild — while `engage` prepares the incoming stage for
/// a clean re-entry (kind switches start fresh; IN-KIND re-voice
/// continuity is the separate invariant, pinned in binding_continuity).
pub struct SwapPoint {
    bench: StageKind,
}

impl SwapPoint {
    /// Park the alternate the chain slot does NOT start with.
    pub fn parking(bench: StageKind) -> Self {
        Self { bench }
    }

    /// Ensure the stage at `id` satisfies `wanted` — if it does not,
    /// swap the benched stage in (it must satisfy it; debug-asserted)
    /// and prepare it at the chain's rate. Returns true when a swap
    /// happened. Move-only: no allocation on either path.
    pub fn engage<const N: usize>(
        &mut self,
        chain: &mut Chain<N>,
        id: StageId,
        wanted: fn(&StageKind) -> bool,
    ) -> bool {
        let sample_rate = chain.sample_rate;
        let Some(slot) = chain.stage_mut(id) else {
            return false;
        };
        if wanted(slot) {
            return false;
        }
        debug_assert!(wanted(&self.bench), "benched stage is not the wanted kind");
        core::mem::swap(slot, &mut self.bench);
        slot.prepare(sample_rate);
        true
    }
}

impl<const N: usize> Stage for Chain<N> {
    /// Serial invariant: latencies sum.
    fn latency(&self) -> u32 {
        self.stages[..self.used].iter().map(Stage::latency).sum()
    }
    /// Serial invariant: tails sum. With global feedback on the chain is
    /// a loop whose stability depends on its gain — infinite.
    fn tail(&self) -> Tail {
        if self.feedback_on() {
            return Tail::Infinite;
        }
        self.stages[..self.used]
            .iter()
            .fold(Tail::None, |t, s| t.then(s.tail()))
    }
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for s in self.stages[..self.used].iter_mut() {
            s.prepare(sample_rate);
        }
        if let Some(f) = self.fb_filter.as_mut() {
            f.prepare(sample_rate);
        }
        self.fb_last = 0.0;
    }
    fn reset(&mut self) {
        for s in self.stages[..self.used].iter_mut() {
            s.reset();
        }
        if let Some(f) = self.fb_filter.as_mut() {
            f.reset();
        }
        self.fb_last = 0.0;
        self.senses = [0.0; N];
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = x;
        let fb_on = self.feedback_on();
        for (i, s) in self.stages[..self.used].iter_mut().enumerate() {
            if fb_on && i == self.fb_to.0 {
                y += self.fb_gain * self.fb_last;
            }
            y = s.tick(y);
            self.senses[i] = y;
        }
        if fb_on {
            let raw = if self.fb_from.0 < self.used {
                self.senses[self.fb_from.0]
            } else {
                y
            };
            let fb = match self.fb_filter.as_mut() {
                Some(f) => f.tick(raw),
                None => raw,
            };
            // A non-finite sample must not recirculate (finite bit-exact).
            self.fb_last = crate::kernel::denormal::finite_or_zero(fb);
        }
        y
    }
    /// STAGE-major when the chain is a pure cascade: each stage runs the
    /// whole block through its monomorphic kernel (one variant dispatch
    /// per stage per block instead of per sample). Bit-identical to the
    /// tick loop — a cascade applies the same per-stage sample sequence
    /// either way — including the sense endpoints (last-sample values,
    /// the block-rate detector contract). Global feedback is a
    /// one-sample cross-stage loop, so it keeps the per-sample path.
    fn process(&mut self, block: &mut [f32]) {
        if self.feedback_on() {
            for s in block.iter_mut() {
                *s = self.tick(*s);
            }
            return;
        }
        if block.is_empty() {
            return;
        }
        for (i, s) in self.stages[..self.used].iter_mut().enumerate() {
            s.process(block);
            self.senses[i] = block[block.len() - 1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavioral::curve::CurveStage;
    use crate::behavioral::curve::Frame;
    use crate::behavioral::shapes;
    use crate::primitives::filters::biquad::FilterSpec;

    fn goertzel(buf: &[f32], f: f64, fs: f64) -> f64 {
        let w = 2.0 * core::f64::consts::PI * f / fs;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in buf {
            let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        ((s1 - s2 * w.cos()).powi(2) + (s2 * w.sin()).powi(2)).sqrt() / (buf.len() as f64 * 0.5)
    }

    /// A small "amp channel": drive-clip → mid-scoop → harder clip.
    fn amp_chain() -> Chain<4> {
        let mut ch = Chain::<4>::new();
        let mut in_stage = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(2.4));
        in_stage.set_pending(Frame {
            bias: 0.1,
            drive: 3.0,
            gain: 1.0,
        });
        ch.push(StageKind::Curve(in_stage)).unwrap();
        ch.push(StageKind::Filter(Biquad::new(FilterSpec::HighShelf {
            hz: 900.0,
            db: -9.0,
        })))
        .unwrap();
        let mut out_stage = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(8.0));
        out_stage.set_pending(Frame {
            bias: 0.0,
            drive: 1.6,
            gain: 1.0,
        });
        ch.push(StageKind::Curve(out_stage)).unwrap();
        ch
    }

    #[test]
    fn chain_runs_in_series_and_is_stable() {
        let fs = 96_000.0f32;
        let mut ch = amp_chain();
        ch.prepare(fs);
        let mut mx = 0.0f32;
        for i in 0..96_000 {
            let x = 0.9 * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / fs as f64).sin() as f32;
            let y = ch.tick(x);
            assert!(y.is_finite());
            mx = mx.max(y.abs());
        }
        assert!(mx < 4.0, "chain runaway: {mx}");
    }

    #[test]
    fn interstage_filter_makes_compression_frequency_dependent() {
        // THE Chain claim, in one number. A memoryless curve's fundamental
        // gain at a given input level is identical at every frequency. A
        // chain with an interstage filter is not: the second clipper sees
        // different levels per frequency, so gain compression varies with
        // frequency — the amp-like G(f, level) bend.
        let fs = 96_000.0;
        let n = 16_384usize;
        let gain_at = |make: &dyn Fn() -> Chain<4>, f0: f64, amp: f64| -> f64 {
            let mut ch = make();
            ch.prepare(fs as f32);
            let m = (f0 * n as f64 / fs).round();
            let sig = |i: usize| {
                (amp * (2.0 * core::f64::consts::PI * m * i as f64 / n as f64).sin()) as f32
            };
            for i in 0..n {
                let _ = ch.tick(sig(i));
            }
            let out: Vec<f32> = (n..2 * n).map(|i| ch.tick(sig(i))).collect();
            20.0 * (goertzel(&out, m * fs / n as f64, fs) / amp).log10()
        };
        // Compression knee depth per frequency: quiet gain − loud gain.
        let comp = |make: &dyn Fn() -> Chain<4>, f0: f64| -> f64 {
            gain_at(make, f0, 0.02) - gain_at(make, f0, 0.8)
        };
        // With the interstage filter:
        let with_filter: &dyn Fn() -> Chain<4> = &amp_chain;
        let c_low = comp(with_filter, 200.0);
        let c_high = comp(with_filter, 5_000.0);
        // Same two curves, NO filter between (collapses to memoryless):
        let no_filter: &dyn Fn() -> Chain<4> = &|| {
            let mut ch = Chain::<4>::new();
            let mut a = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(2.4));
            a.set_pending(Frame {
                bias: 0.1,
                drive: 3.0,
                gain: 1.0,
            });
            ch.push(StageKind::Curve(a)).unwrap();
            let mut b = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(8.0));
            b.set_pending(Frame {
                bias: 0.0,
                drive: 1.6,
                gain: 1.0,
            });
            ch.push(StageKind::Curve(b)).unwrap();
            ch
        };
        let m_low = comp(no_filter, 200.0);
        let m_high = comp(no_filter, 5_000.0);
        println!(
            "compression depth (quiet−loud): chain {c_low:.2}/{c_high:.2} dB @200/5k | memoryless {m_low:.2}/{m_high:.2} dB"
        );
        assert!(
            (m_low - m_high).abs() < 0.4,
            "memoryless compression should be ~frequency-independent: {m_low} vs {m_high}"
        );
        assert!(
            (c_low - c_high).abs() > 2.0,
            "interstage filter should bend compression across frequency: {c_low} vs {c_high}"
        );
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::behavioral::curve::CurveStage;

    #[test]
    fn push_reports_full_instead_of_a_sentinel() {
        let mut ch = Chain::<1>::new();
        assert_eq!(ch.push(StageKind::Bypass), Ok(StageId(0)));
        assert_eq!(ch.push(StageKind::Bypass), Err(Full));
        assert_eq!(ch.len(), 1);
    }

    #[test]
    fn tail_and_latency_forward_through_every_slot() {
        let lp = |hz| StageKind::Filter(Biquad::new(FilterSpec::Lowpass { hz, q: 0.7 }));
        let mut ch = Chain::<3>::new();
        ch.push(lp(100.0)).unwrap();
        ch.push(StageKind::Curve(CurveStage::from_fn(-1.0, 1.0, 0.0, |x| x)))
            .unwrap();
        ch.push(lp(50.0)).unwrap();
        ch.prepare(48_000.0);
        let mut a = Biquad::new(FilterSpec::Lowpass { hz: 100.0, q: 0.7 });
        let mut b = Biquad::new(FilterSpec::Lowpass { hz: 50.0, q: 0.7 });
        a.prepare(48_000.0);
        b.prepare(48_000.0);
        assert_eq!(ch.tail(), a.tail().then(b.tail()), "serial tails sum");
        assert!(matches!(ch.tail(), Tail::Samples(_)));
        assert_eq!(ch.latency(), 0);
        // Global feedback makes the chain a loop: no finite bound.
        ch.set_feedback(-0.3, StageId(2), StageId(0), None);
        assert_eq!(ch.tail(), Tail::Infinite);
        ch.set_feedback(0.0, StageId(2), StageId(0), None);
        assert_eq!(ch.tail(), a.tail().then(b.tail()));
    }
}

#[cfg(test)]
mod swap_tests {
    use super::*;
    use crate::behavioral::curve::CurveStage;
    use crate::kernel::node::Stage;

    fn tiny_chain() -> (Chain<2>, StageId) {
        let mut ch = Chain::<2>::new();
        let id = ch
            .push(StageKind::Curve(CurveStage::from_fn(-1.0, 1.0, 0.0, |x| x)))
            .unwrap();
        ch.prepare(48_000.0);
        (ch, id)
    }

    #[test]
    fn engage_round_trips_the_instance() {
        // A 2x curve parked and unparked comes back WITH its table —
        // the same instance moved (a fresh CurveStage would be x→0).
        let mut ch = Chain::<2>::new();
        let id = ch
            .push(StageKind::Curve(CurveStage::from_fn(-2.0, 2.0, 0.0, |x| {
                2.0 * x
            })))
            .unwrap();
        ch.prepare(48_000.0);
        let mut sp = SwapPoint::parking(StageKind::Bypass);
        // Settle ADAA (the first quotient averages from x=0).
        let mut y0 = 0.0;
        for _ in 0..4 {
            y0 = ch.tick(0.25);
        }
        assert!((y0 - 0.5).abs() < 1e-3, "table sanity: {y0}");
        assert!(sp.engage(&mut ch, id, |s| matches!(s, StageKind::Bypass)));
        assert!(
            (ch.tick(0.25) - 0.25).abs() < 1e-6,
            "parked slot is a passthrough"
        );
        assert!(sp.engage(&mut ch, id, |s| matches!(s, StageKind::Curve(_))));
        let mut y1 = 0.0;
        for _ in 0..4 {
            y1 = ch.tick(0.25);
        }
        assert!(
            (y1 - 0.5).abs() < 1e-3,
            "unparked curve lost its table ({y1}) — instance was not moved"
        );
        // The bench can never miss: hammer the swap both ways.
        for i in 0..64 {
            let want: fn(&StageKind) -> bool = if i % 2 == 0 {
                |s| matches!(s, StageKind::Bypass)
            } else {
                |s| matches!(s, StageKind::Curve(_))
            };
            assert!(sp.engage(&mut ch, id, want));
        }
    }

    #[test]
    fn engage_is_idempotent_when_already_wanted() {
        let (mut ch, id) = tiny_chain();
        let mut sp = SwapPoint::parking(StageKind::Bypass);
        assert!(!sp.engage(&mut ch, id, |s| matches!(s, StageKind::Curve(_))));
        assert!(sp.engage(&mut ch, id, |s| matches!(s, StageKind::Bypass)));
        assert!(!sp.engage(&mut ch, id, |s| matches!(s, StageKind::Bypass)));
    }

    #[test]
    fn topology_reads_the_live_structure() {
        let mut ch = Chain::<3>::new();
        let a = ch
            .push(StageKind::Curve(CurveStage::from_fn(-1.0, 1.0, 0.0, |x| x)))
            .unwrap();
        ch.push(StageKind::Filter(Biquad::new(FilterSpec::Lowpass {
            hz: 1_000.0,
            q: 0.7,
        })))
        .unwrap();
        fn snapshot<const N: usize>(ch: &Chain<N>) -> Vec<(usize, &'static str)> {
            ch.topology().map(|(id, k)| (id.0, k)).collect()
        }
        assert_eq!(snapshot(&ch), vec![(0, "curve"), (1, "filter")]);
        // A swap changes the emitted data because it changes the
        // structure — same source of truth.
        let mut sp = SwapPoint::parking(StageKind::Bypass);
        sp.engage(&mut ch, a, |s| matches!(s, StageKind::Bypass));
        assert_eq!(snapshot(&ch)[0], (0, "bypass"));
    }
}

// StageKind — Chain's own fixed-dispatch storage (the closed set is a
// chain detail, never a gate on what can be a Stage).
use crate::behavioral::curve::CurveStage;
use crate::behavioral::hysteresis::Hysteresis;
use crate::behavioral::rc::NonlinearRc;
use crate::behavioral::slew::Slew;
use crate::circuit::topologies::coupler::Coupler;

/// Enum dispatch so a [`Chain`] is a flat array —
/// no boxing, no vtables on the audio path.
pub enum StageKind {
    Bypass,
    Curve(CurveStage),
    Filter(Biquad),
    Slew(Slew),
    Hysteresis(Hysteresis),
    Rc(NonlinearRc),
    Coupler(Coupler),
}

impl StageKind {
    /// Reflection sidecar v1: the kind as declared data, so a
    /// topology description can be emitted from the LIVE structure —
    /// it cannot drift from what actually runs.
    pub const fn kind_name(&self) -> &'static str {
        match self {
            StageKind::Bypass => "bypass",
            StageKind::Curve(_) => "curve",
            StageKind::Filter(_) => "filter",
            StageKind::Slew(_) => "slew",
            StageKind::Hysteresis(_) => "hysteresis",
            StageKind::Rc(_) => "rc",
            StageKind::Coupler(_) => "coupler",
        }
    }
}

/// Forward one `Stage` method to whichever variant is live; `Bypass` is
/// the identity.
macro_rules! each_kind {
    ($self:expr, $s:ident => $e:expr, bypass => $b:expr) => {
        match $self {
            StageKind::Bypass => $b,
            StageKind::Curve($s) => $e,
            StageKind::Filter($s) => $e,
            StageKind::Slew($s) => $e,
            StageKind::Hysteresis($s) => $e,
            StageKind::Rc($s) => $e,
            StageKind::Coupler($s) => $e,
        }
    };
}

impl Stage for StageKind {
    fn prepare(&mut self, sample_rate: f32) {
        each_kind!(self, s => s.prepare(sample_rate), bypass => {})
    }
    fn reset(&mut self) {
        each_kind!(self, s => s.reset(), bypass => {})
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        each_kind!(self, s => s.tick(x), bypass => x)
    }
    /// One variant dispatch per block, then the leaf's monomorphic tick
    /// loop — the block form's whole win at the container level.
    fn process(&mut self, block: &mut [f32]) {
        each_kind!(self, s => s.process(block), bypass => {})
    }
    fn latency(&self) -> u32 {
        each_kind!(self, s => s.latency(), bypass => 0)
    }
    fn tail(&self) -> Tail {
        each_kind!(self, s => s.tail(), bypass => Tail::None)
    }
}

#[cfg(test)]
mod stage_kind_tests {
    use super::*;
    use crate::behavioral::curve::Frame;
    use crate::behavioral::shapes;
    use crate::RcClipperConfig;

    #[test]
    fn curve_stage_matches_source_and_commits_frames() {
        let mut cs = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(4.0));
        cs.prepare(96_000.0);
        // DC settles to f(x) through ADAA.
        for _ in 0..4 {
            let _ = cs.tick(0.4);
        }
        let y = cs.tick(0.4) as f64;
        let want = shapes::knee(4.0)(0.4);
        assert!((y - want).abs() < 1e-3, "{y} vs {want}");
        // Pending frame applies (gain doubles output).
        cs.set_pending(Frame {
            bias: 0.0,
            drive: 1.0,
            gain: 2.0,
        });
        for _ in 0..4 {
            let _ = cs.tick(0.4);
        }
        let y2 = cs.tick(0.4) as f64;
        assert!((y2 - 2.0 * want).abs() < 2e-3, "{y2} vs {}", 2.0 * want);
    }

    #[test]
    fn peak_filter_is_a_placed_bell_and_rate_invariant() {
        // Gain at the center hits the label; an octave+ away it returns
        // toward unity; and the response in Hz survives the rate contract
        // (same numbers prepared at 48k and at 4×192k).
        let amp_at = |rate: f32, f0: f64| -> f64 {
            let mut fs = Biquad::new(FilterSpec::Peak {
                hz: 800.0,
                db: -7.0,
                q: 1.4,
            });
            fs.prepare(rate);
            let n = (rate as usize).min(192_000);
            let mut acc = 0.0f64;
            let mut count = 0usize;
            for i in 0..2 * n {
                let x = (2.0 * core::f64::consts::PI * f0 * i as f64 / rate as f64).sin() as f32;
                let y = fs.tick(x);
                if i >= n {
                    acc += (y as f64) * (y as f64);
                    count += 1;
                }
            }
            20.0 * ((acc / count as f64).sqrt() * core::f64::consts::SQRT_2).log10()
        };
        for rate in [48_000.0f32, 192_000.0] {
            let center = amp_at(rate, 800.0);
            let low = amp_at(rate, 100.0);
            let high = amp_at(rate, 6_000.0);
            assert!(
                (center + 7.0).abs() < 0.5,
                "center gain {center} dB at {rate}"
            );
            assert!(
                low.abs() < 1.0 && high.abs() < 1.0,
                "shoulders {low}/{high} dB at {rate}"
            );
        }
    }

    #[test]
    fn slew_distorts_by_frequency_not_amplitude() {
        // Equal-amplitude tones: the fast one is limited, the slow one
        // passes — memoryless curves cannot do this.
        let fs = 48_000.0;
        let thd = |f0: f64| -> f64 {
            // 1.0 FS/ms: a full-scale 100 Hz sine (max slope ≈ 0.63 FS/ms)
            // passes clean; 2 kHz (≈ 12.6 FS/ms) is heavily limited.
            let mut s = Slew::new(1.0, 1.0);
            s.prepare(fs as f32);
            let n = 4800;
            let mut out = vec![0.0f32; n];
            for i in 0..2 * n {
                let x = (2.0 * core::f64::consts::PI * f0 * i as f64 / fs).sin() as f32;
                let y = s.tick(x);
                if i >= n {
                    out[i - n] = y;
                }
            }
            let g = |f: f64| {
                let w = 2.0 * core::f64::consts::PI * f / fs;
                let (mut s1, mut s2) = (0.0f64, 0.0f64);
                for &v in &out {
                    let s0 = v as f64 + 2.0 * w.cos() * s1 - s2;
                    s2 = s1;
                    s1 = s0;
                }
                ((s1 - s2 * w.cos()).powi(2) + (s2 * w.sin()).powi(2)).sqrt()
            };
            g(3.0 * f0) / g(f0).max(1e-12)
        };
        let lo = thd(100.0);
        let hi = thd(2_000.0);
        assert!(
            hi > lo * 5.0,
            "slew should distort high freqs more: {lo} vs {hi}"
        );
    }

    #[test]
    fn nonlinear_rc_converges_is_stable_and_has_memory() {
        let fs = 48_000.0f32;
        let mut rc = NonlinearRc::diode_clipper(&RcClipperConfig {
            tau_ms: 4.0,
            threshold: 0.4,
            hardness: 6.0,
        });
        rc.prepare(fs);
        // Bounded + finite under hot drive.
        let mut mx = 0.0f32;
        for i in 0..48_000 {
            let x = 1.5 * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / fs as f64).sin() as f32;
            let y = rc.tick(x);
            assert!(y.is_finite());
            mx = mx.max(y.abs());
        }
        assert!(mx < 4.0, "runaway {mx}");
        // Memory: the same input value maps to different outputs depending
        // on history (state ≠ 0 after a loud passage biases the clip).
        rc.reset();
        let y_cold = rc.tick(0.5);
        rc.reset();
        for _ in 0..2000 {
            let _ = rc.tick(1.2);
        }
        let y_hot = rc.tick(0.5);
        assert!(
            (y_cold - y_hot).abs() > 1e-3,
            "the RC stage shows no history dependence: {y_cold} vs {y_hot}"
        );
        // Silence discharges the state through the bleed leak
        // (τ_discharge ≈ τ/LEAK = 80 ms here).
        for _ in 0..(fs as usize) {
            let _ = rc.tick(0.0);
        }
        assert!(rc.tick(0.0).abs() < 1e-3, "state latched: {}", rc.tick(0.0));
    }

    #[test]
    fn ts_shunt_clipper_boosts_bounds_and_remembers() {
        let fs = 48_000.0f32;
        let mut ts = NonlinearRc::ts_feedback_clipper(
            &RcClipperConfig {
                tau_ms: 0.5,
                threshold: 0.4,
                hardness: 6.0,
            },
            4.0,
        );
        ts.prepare(fs);
        // Character: quiet signals get boosted (op-amp gain), loud
        // signals get x + (bounded diode voltage) — never below dry.
        let probe = |rc: &mut NonlinearRc, amp: f64| -> (f64, f64) {
            rc.reset();
            let mut peak_in = 0.0f64;
            let mut peak_out = 0.0f64;
            for i in 0..9_600 {
                let x = amp * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / fs as f64).sin();
                let y = rc.tick(x as f32) as f64;
                if i > 4_800 {
                    peak_in = peak_in.max(x.abs());
                    peak_out = peak_out.max(y.abs());
                }
            }
            (peak_in, peak_out)
        };
        let (qi, qo) = probe(&mut ts, 0.05);
        assert!(qo / qi > 1.5, "quiet should be boosted: {}", qo / qi);
        let (li, lo) = probe(&mut ts, 1.0);
        assert!(lo >= li - 1e-3, "output never below dry: {lo} vs {li}");
        assert!(lo < li + 1.0, "diode bound on the feedback voltage: {lo}");
        // Memory: same instant input, different output after loud history.
        ts.reset();
        let cold = ts.tick(0.3);
        ts.reset();
        for _ in 0..2_000 {
            let _ = ts.tick(1.0);
        }
        let hot = ts.tick(0.3);
        assert!((cold - hot).abs() > 1e-3, "no history: {cold} vs {hot}");
    }

    #[test]
    fn hysteresis_stage_loops() {
        // Real loop (nonzero area), roughly frequency-independent — the
        // Memory::Tape invariant, preserved through the port.
        let fs = 48_000.0;
        let area = |f0: f64| {
            let mut h = Hysteresis::new(1.5, 0.8);
            h.prepare(fs as f32);
            let period = (fs / f0) as usize;
            for i in 0..period * 8 {
                let x = 0.8 * (2.0 * core::f64::consts::PI * i as f64 / period as f64).sin();
                let _ = h.tick(x as f32);
            }
            let mut a = 0.0f64;
            let mut xs = vec![];
            let mut ys = vec![];
            for i in 0..period {
                let x = 0.8 * (2.0 * core::f64::consts::PI * i as f64 / period as f64).sin();
                xs.push(x);
                ys.push(h.tick(x as f32) as f64);
            }
            for i in 0..period {
                let j = (i + 1) % period;
                a += xs[i] * ys[j] - xs[j] * ys[i];
            }
            (a * 0.5).abs()
        };
        let lo = area(50.0);
        let hi = area(500.0);
        assert!(lo > 0.01, "no loop: {lo}");
        let ratio = (lo / hi).max(hi / lo);
        assert!(
            ratio < 2.5,
            "loop area frequency-dependent (lag, not hysteresis): {lo} vs {hi}"
        );
    }
}
