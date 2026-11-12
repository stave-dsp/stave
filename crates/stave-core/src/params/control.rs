//! The processor's side of parameters: a [`Smoother`] runs a
//! [`FloatParam`]'s declared glide, [`Steps`] follows a stepped param
//! (bool, enum) at its exact frames. Parameters hold the shared target
//! (hosts, faces, state); these hold the per-plugin render state.

use super::{BoolParam, EnumParam, FloatParam, ParamEnum, ParamId, Range, Smooth};
use crate::environment::Environment;
use crate::process::{Block, ParamBlock, ParamChange, ParamChangeKind};

/// A [`FloatParam`]'s glide, owned by the plugin that runs it.
///
/// Build it from the param in `Plugin::new`, [`prepare`](Self::prepare)
/// it in `Plugin::prepare`, and take one [`Ramp`] per block:
///
/// ```ignore
/// let mut tilt = self.tilt.ramp(block, &self.params.tilt);
/// // per frame:
/// let t = tilt.next();
/// ```
#[derive(Clone, Debug)]
pub struct Smoother {
    id: ParamId,
    /// The parameter's restore count when last read: a new count means a
    /// restored value, which is jumped to, not glided to.
    restores: u32,
    range: Range,
    smooth: Smooth,
    current: f64,
    target: f64,
    exp_coeff: f64,
    linear_len: u32,
    linear_remaining: u32,
    /// Set by `prepare`: the next ramp starts at the param's value.
    snap_next: bool,
}

impl Smoother {
    /// The glide `param` declares, starting at its current value.
    pub fn new(param: &FloatParam) -> Self {
        let initial = param.plain();
        Self {
            id: param.id(),
            restores: param.restores(),
            range: param.range(),
            smooth: param.smoothing(),
            current: initial,
            target: initial,
            exp_coeff: 1.0,
            linear_len: 1,
            linear_remaining: 0,
            snap_next: true,
        }
    }

    /// Derive the rate-dependent state; the next ramp starts at the
    /// param's value as of its block's start, without a glide.
    pub fn prepare(&mut self, env: &Environment) {
        let sample_rate = f64::from(env.sample_rate).max(1.0);
        match self.smooth {
            Smooth::Off => {}
            Smooth::Exp { ms } => {
                let tau = (ms / 1000.0).max(1e-6);
                self.exp_coeff = 1.0 - (-1.0 / (tau * sample_rate)).exp();
            }
            Smooth::Linear { ms } => {
                self.linear_len = ((ms / 1000.0) * sample_rate).round().max(1.0) as u32;
            }
        }
        self.snap_next = true;
    }

    /// Jump to `value` without a glide.
    pub fn snap(&mut self, value: f64) {
        let value = self.clamp(value);
        self.current = value;
        self.target = value;
        self.linear_remaining = 0;
    }

    /// The value the last ramp reached.
    pub fn current(&self) -> f32 {
        self.current as f32
    }

    /// This block's per-frame values of `param` (the one this smoother
    /// was built from), honouring the block's timed changes.
    pub fn ramp<'s>(&'s mut self, block: &Block<'s>, param: &FloatParam) -> Ramp<'s> {
        self.ramp_over(block.params(), param)
    }

    /// [`ramp`](Self::ramp) over a bare change list (tests, tools).
    pub fn ramp_over<'s>(&'s mut self, changes: ParamBlock<'s>, param: &FloatParam) -> Ramp<'s> {
        debug_assert_eq!(param.id(), self.id, "a smoother read against another parameter");
        // The restore protocol (see `FloatParam::restore`): count,
        // value, count. A count that moved, or an odd (in-progress) count,
        // means the value read belongs to a restore — snap to it.
        let first = param.restores();
        let value = param.plain();
        let second = param.restores();
        self.ramp_read(changes, first, value, second)
    }

    /// [`ramp_over`](Self::ramp_over) after its three loads.
    fn ramp_read<'s>(
        &'s mut self,
        changes: ParamBlock<'s>,
        first: u32,
        value: f64,
        second: u32,
    ) -> Ramp<'s> {
        let has_points = changes
            .changes()
            .iter()
            .any(|change| change.id == self.id && change.kind == ParamChangeKind::Value);
        if core::mem::take(&mut self.snap_next) {
            // The first block after prepare starts at the param's value
            // as of the block's start: the table's, unless this block
            // moves it — then the value before its first point, the last
            // this smoother saw (the table already holds the block's end).
            self.restores = first;
            let start = if has_points { self.target } else { value };
            self.snap(start);
        }
        self.observe_restore(first, value, second);
        let final_target = self.clamp(value);
        if !has_points {
            self.retarget(final_target);
        }
        Ramp {
            smoother: self,
            changes: changes.changes(),
            next_change: 0,
            frame: 0,
            final_target,
        }
    }

    /// One read of the store under the restore protocol: the count
    /// before (`first`) and after (`second`) reading `value`. Any sign of
    /// a restore — a count other than the last seen, a count that moved
    /// during the read, or an odd count (the writer is mid-restore) —
    /// snaps. Remembering `first`, not `second`, keeps an unsettled read
    /// a restore for the next block too, which snaps again to whatever
    /// value the restore left.
    fn observe_restore(&mut self, first: u32, value: f64, second: u32) {
        if first != self.restores || second != first || first & 1 == 1 {
            self.restores = first;
            self.snap(value);
        }
    }

    fn clamp(&self, value: f64) -> f64 {
        if value.is_nan() {
            self.target
        } else {
            value.clamp(self.range.min(), self.range.max())
        }
    }

    fn retarget(&mut self, target: f64) {
        let target = self.clamp(target);
        if self.target == target {
            return;
        }
        self.target = target;
        if matches!(self.smooth, Smooth::Linear { .. }) {
            self.linear_remaining = self.linear_len;
        }
    }

    fn next_value(&mut self) -> f64 {
        match self.smooth {
            Smooth::Off => {
                self.current = self.target;
            }
            Smooth::Exp { .. } => {
                self.current += self.exp_coeff * (self.target - self.current);
                if (self.current - self.target).abs() <= 1e-10 {
                    self.current = self.target;
                }
            }
            Smooth::Linear { .. } => {
                if self.linear_remaining == 0 {
                    self.current = self.target;
                } else {
                    self.current += (self.target - self.current) / f64::from(self.linear_remaining);
                    self.linear_remaining -= 1;
                    if self.linear_remaining == 0 {
                        self.current = self.target;
                    }
                }
            }
        }
        self.current
    }
}

/// One block of a [`Smoother`]'s values: call [`next`](Self::next) once
/// per frame. Allocation free.
pub struct Ramp<'s> {
    smoother: &'s mut Smoother,
    changes: &'s [ParamChange],
    next_change: usize,
    frame: usize,
    final_target: f64,
}

impl Ramp<'_> {
    /// The next frame's value.
    #[inline]
    #[expect(
        clippy::should_implement_trait,
        reason = "an endless per-frame stream; Option would force unwrap in every DSP loop"
    )]
    pub fn next(&mut self) -> f32 {
        self.next_f64() as f32
    }

    /// [`next`](Self::next) at full precision.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        while let Some(change) = self.changes.get(self.next_change) {
            if change.frame > self.frame {
                break;
            }
            self.next_change += 1;
            if change.id == self.smoother.id && change.kind == ParamChangeKind::Value {
                self.smoother.retarget(change.value);
            }
        }
        self.frame += 1;
        self.smoother.next_value()
    }
}

impl Drop for Ramp<'_> {
    fn drop(&mut self) {
        // The table already holds the last host point: if fewer frames
        // were taken than the block has, the next block still aims at it.
        self.smoother.retarget(self.final_target);
    }
}

/// A parameter with discrete steps: [`BoolParam`] and [`EnumParam`].
pub trait Stepped {
    /// The value in its natural type.
    type Value: Copy;
    fn id(&self) -> ParamId;
    /// The current value.
    fn value(&self) -> Self::Value;
    /// A plain (wire) value as this param's value.
    fn decode(&self, plain: f64) -> Self::Value;
}

impl Stepped for BoolParam {
    type Value = bool;
    fn id(&self) -> ParamId {
        BoolParam::id(self)
    }
    fn value(&self) -> bool {
        BoolParam::value(self)
    }
    fn decode(&self, plain: f64) -> bool {
        plain >= 0.5
    }
}

impl<T: ParamEnum> Stepped for EnumParam<T> {
    type Value = T;
    fn id(&self) -> ParamId {
        EnumParam::id(self)
    }
    fn value(&self) -> T {
        EnumParam::value(self)
    }
    fn decode(&self, plain: f64) -> T {
        T::from_index(if plain.is_nan() { 0 } else { plain.round().max(0.0) as u32 })
    }
}

/// One change of a stepped param, at its frame.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Step<T> {
    pub frame: usize,
    pub value: T,
}

/// A stepped param as the plugin follows it: the value in force, and
/// each block's changes at their exact frames.
#[derive(Clone, Debug)]
pub struct Steps<T: Copy> {
    id: ParamId,
    value: T,
}

impl<T: Copy> Steps<T> {
    /// Follow `param`, starting at its current value.
    pub fn new<P: Stepped<Value = T>>(param: &P) -> Self {
        Self {
            id: param.id(),
            value: param.value(),
        }
    }

    /// The value in force (after the last block, the param's value).
    pub fn value(&self) -> T {
        self.value
    }

    /// This block's changes of `param`, in frame order. When the
    /// iterator is dropped the value in force is the param's latest.
    pub fn changes<'s, P: Stepped<Value = T>>(
        &'s mut self,
        block: &Block<'s>,
        param: &'s P,
    ) -> StepChanges<'s, P> {
        self.changes_over(block.params(), param)
    }

    /// [`changes`](Self::changes) over a bare change list (tests, tools).
    pub fn changes_over<'s, P: Stepped<Value = T>>(
        &'s mut self,
        changes: ParamBlock<'s>,
        param: &'s P,
    ) -> StepChanges<'s, P> {
        debug_assert_eq!(param.id(), self.id, "steps read against another parameter");
        StepChanges {
            steps: self,
            param,
            changes: changes.changes(),
            next: 0,
        }
    }
}

/// The iterator [`Steps::changes`] returns.
pub struct StepChanges<'s, P: Stepped> {
    steps: &'s mut Steps<P::Value>,
    param: &'s P,
    changes: &'s [ParamChange],
    next: usize,
}

impl<P: Stepped> Iterator for StepChanges<'_, P> {
    type Item = Step<P::Value>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(change) = self.changes.get(self.next) {
            self.next += 1;
            if change.id == self.steps.id && change.kind == ParamChangeKind::Value {
                let value = self.param.decode(change.value);
                self.steps.value = value;
                return Some(Step {
                    frame: change.frame,
                    value,
                });
            }
        }
        None
    }
}

impl<P: Stepped> Drop for StepChanges<'_, P> {
    fn drop(&mut self) {
        self.steps.value = self.param.value();
    }
}

/// A value whose active form may only change at a safe point (the
/// plugin's `prepare` or `reset`): set it any time, apply it there.
#[derive(Clone, Debug)]
pub struct Deferred<T> {
    pending: T,
    active: T,
}

impl<T: Clone> Deferred<T> {
    pub fn new(initial: T) -> Self {
        Self {
            pending: initial.clone(),
            active: initial,
        }
    }

    pub fn set(&mut self, value: T) {
        self.pending = value;
    }

    pub fn pending(&self) -> &T {
        &self.pending
    }

    pub fn active(&self) -> &T {
        &self.active
    }

    /// Make the pending value the active one (at the safe point).
    pub fn apply(&mut self) -> &T {
        self.active.clone_from(&self.pending);
        &self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::AnyParam;
    use crate::process::{ParamChange, ParamChangeKind};

    fn param(smooth: Smooth) -> FloatParam {
        FloatParam::new(ParamId(7), "drive", Range::linear(0.0, 1.0)).smooth(smooth)
    }

    #[test]
    fn continuous_steps_at_the_declared_offset() {
        let p = param(Smooth::Off);
        let mut control = Smoother::new(&p);
        settle(&mut control, &p, 48_000.0);
        p.set_value(1.0);
        let points = [ParamChange {
            id: ParamId(7),
            frame: 3,
            value: 1.0,
            kind: ParamChangeKind::Value,
        }];
        let mut block = control.ramp_over(ParamBlock::new(&points), &p);
        assert_eq!(
            (0..6).map(|_| block.next()).collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
        );
    }

    #[test]
    fn continuous_state_is_owned_by_each_processor() {
        let p = param(Smooth::linear_ms(10.0));
        let mut a = Smoother::new(&p);
        let mut b = Smoother::new(&p);
        settle(&mut a, &p, 1_000.0);
        settle(&mut b, &p, 1_000.0);
        p.set_value(1.0);
        let mut block = a.ramp_over(ParamBlock::empty(), &p);
        for _ in 0..5 {
            block.next();
        }
        drop(block);
        assert!((a.current() - 0.5).abs() < 1e-12);
        assert_eq!(
            b.current(),
            0.0,
            "another processor has independent history"
        );
    }

    /// Prepare at `rate` and take the first (snapping) ramp.
    fn settle(control: &mut Smoother, p: &FloatParam, rate: f32) {
        control.prepare(&Environment::new(rate, 64));
        drop(control.ramp_over(ParamBlock::empty(), p));
    }

    fn prepared(smooth: Smooth, rate: f32, initial: f64) -> (FloatParam, Smoother) {
        let p = FloatParam::new(ParamId(7), "drive", Range::linear(0.0, 1.0))
            .default(initial)
            .smooth(smooth);
        let mut control = Smoother::new(&p);
        settle(&mut control, &p, rate);
        (p, control)
    }

    fn render(control: &mut Smoother, p: &FloatParam, target: f64, n: usize) -> Vec<f64> {
        p.set_value(target);
        let mut block = control.ramp_over(ParamBlock::empty(), p);
        (0..n).map(|_| block.next_f64()).collect()
    }

    fn point(offset: usize, value: f64) -> ParamChange {
        ParamChange {
            id: ParamId(7),
            frame: offset,
            value,
            kind: ParamChangeKind::Value,
        }
    }

    #[test]
    fn off_yields_the_target_immediately() {
        let (p, mut c) = prepared(Smooth::Off, 48_000.0, 0.0);
        assert_eq!(render(&mut c, &p, 1.0, 4), [1.0; 4]);
    }

    #[test]
    fn exp_time_constant_is_rate_derived() {
        let (p, mut c) = prepared(Smooth::ms(15.0), 48_000.0, 0.0);
        let tau = (0.015 * 48_000.0) as usize;
        let after_tau = render(&mut c, &p, 1.0, tau)[tau - 1];
        assert!(
            (after_tau - (1.0 - (-1.0f64).exp())).abs() < 0.01,
            "{after_tau}"
        );
    }

    #[test]
    fn linear_arrives_exactly_and_holds() {
        let (p, mut c) = prepared(Smooth::linear_ms(10.0), 1_000.0, 0.0);
        let out = render(&mut c, &p, 1.0, 12);
        assert!((out[4] - 0.5).abs() < 1e-12, "midpoint {}", out[4]);
        assert_eq!(out[9..], [1.0; 3], "exact arrival, then hold");
    }

    #[test]
    fn a_glide_continues_across_blocks() {
        let (p, mut c) = prepared(Smooth::linear_ms(10.0), 1_000.0, 0.0);
        let a = render(&mut c, &p, 1.0, 4);
        let b = render(&mut c, &p, 1.0, 6);
        assert!((a[3] - 0.4).abs() < 1e-12);
        assert!((b[0] - 0.5).abs() < 1e-12, "block 2 continues: {}", b[0]);
        assert_eq!(b[5], 1.0);
    }

    #[test]
    fn a_linear_retarget_restarts_from_the_current_value() {
        let (p, mut c) = prepared(Smooth::linear_ms(10.0), 1_000.0, 0.0);
        render(&mut c, &p, 1.0, 5);
        let out = render(&mut c, &p, 0.0, 10);
        assert!(
            (out[0] - 0.45).abs() < 1e-12,
            "restarts from 0.5: {}",
            out[0]
        );
        assert_eq!(out[9], 0.0, "a fresh full ramp");
    }

    #[test]
    fn a_restored_value_is_jumped_to_not_glided_to() {
        let (p, mut c) = prepared(Smooth::ms(15.0), 48_000.0, 0.0);
        (&p as &dyn AnyParam).restore(0.7);
        let mut block = c.ramp_over(ParamBlock::empty(), &p);
        assert_eq!(
            [block.next_f64(), block.next_f64()],
            [0.7, 0.7],
            "no fade-in"
        );
    }

    #[test]
    fn a_restore_snaps_through_restore() {
        let (p, mut c) = prepared(Smooth::ms(15.0), 48_000.0, 0.0);
        (&p as &dyn AnyParam).restore(0.7);
        let mut block = c.ramp_over(ParamBlock::empty(), &p);
        assert_eq!([block.next_f64(), block.next_f64()], [0.7, 0.7]);
    }

    /// One step of a restore racing a block's read of the store.
    #[derive(Copy, Clone, Debug)]
    enum Step {
        Writer(usize),
        Reader(usize),
    }

    /// Every interleaving of `writers` writer steps with `readers` reader
    /// steps (each side keeps its program order).
    fn interleavings(writers: usize, readers: usize) -> Vec<Vec<Step>> {
        if writers == 0 && readers == 0 {
            return vec![Vec::new()];
        }
        let mut out = Vec::new();
        if writers > 0 {
            for mut rest in interleavings(writers - 1, readers) {
                rest.insert(0, Step::Writer(0));
                out.push(rest);
            }
        }
        if readers > 0 {
            for mut rest in interleavings(writers, readers - 1) {
                rest.insert(0, Step::Reader(0));
                out.push(rest);
            }
        }
        // Number each side's steps in order.
        for steps in &mut out {
            let (mut w, mut r) = (0, 0);
            for step in steps.iter_mut() {
                match step {
                    Step::Writer(i) => {
                        *i = w;
                        w += 1;
                    }
                    Step::Reader(i) => {
                        *i = r;
                        r += 1;
                    }
                }
            }
        }
        out
    }

    /// A restore racing the first block's read, every interleaving of
    /// the writer's three steps (count+1, value, count+1) with the
    /// reader's three loads (count, value, count): the block and the one
    /// after it hold the old value or the restored value exactly — never
    /// a sample of glide between them.
    #[test]
    fn a_restore_racing_a_block_never_glides() {
        const OLD: f64 = 0.0;
        const NEW: f64 = 0.7;
        for steps in interleavings(3, 3) {
            let (p, mut c) = prepared(Smooth::ms(15.0), 48_000.0, OLD);
            let start = p.restores();
            let (mut count, mut value) = (start, OLD);
            let mut reads = (0u32, 0.0f64, 0u32);
            for &step in &steps {
                match step {
                    Step::Writer(0) | Step::Writer(2) => count += 1,
                    Step::Writer(_) => value = NEW,
                    Step::Reader(0) => reads.0 = count,
                    Step::Reader(1) => reads.1 = value,
                    Step::Reader(_) => reads.2 = count,
                }
            }
            let first: Vec<f64> = {
                let mut block = c.ramp_read(ParamBlock::empty(), reads.0, reads.1, reads.2);
                (0..64).map(|_| block.next_f64()).collect()
            };
            // The writer has finished: the real store holds the restore.
            (&p as &dyn AnyParam).restore(NEW);
            assert_eq!(p.restores(), start + 2);
            let second: Vec<f64> = {
                let mut block = c.ramp_over(ParamBlock::empty(), &p);
                (0..64).map(|_| block.next_f64()).collect()
            };
            assert!(
                first.iter().all(|&v| v == OLD) || first.iter().all(|&v| v == NEW),
                "{steps:?}: the racing block glided: {:?}",
                &first[..4]
            );
            assert!(
                second.iter().all(|&v| v == NEW),
                "{steps:?}: the block after the restore glided: {:?}",
                &second[..4]
            );
        }
    }

    /// The model above catches the old protocol: value stored then the
    /// count bumped once, read value-then-count, glides in one
    /// interleaving (the reader sees the new value before the bump).
    #[test]
    fn the_model_catches_the_old_value_then_count_protocol() {
        let glided = interleavings(2, 2).into_iter().any(|steps| {
            let (p, mut c) = prepared(Smooth::ms(15.0), 48_000.0, 0.0);
            let seen = p.restores();
            let (mut count, mut value) = (seen, 0.0);
            let (mut read_value, mut read_count) = (0.0, 0);
            for step in steps {
                match step {
                    Step::Writer(0) => value = 0.7,
                    Step::Writer(_) => count += 1,
                    Step::Reader(0) => read_value = value,
                    Step::Reader(_) => read_count = count,
                }
            }
            // The old reader: snap only on a moved count.
            if read_count != seen {
                c.snap(read_value);
            }
            let mut block = c.ramp_read(ParamBlock::empty(), seen, read_value, seen);
            let v = block.next_f64();
            v != 0.0 && v != 0.7
        });
        assert!(glided, "the old protocol has a gliding interleaving");
    }

    #[test]
    fn a_scheduled_point_retargets_an_exp_glide_at_its_offset() {
        let (p, mut c) = prepared(Smooth::ms(1.0), 48_000.0, 0.0);
        p.set_value(1.0);
        let points = [point(100, 1.0)];
        let mut block = c.ramp_over(ParamBlock::new(&points), &p);
        let mut before = 0.0;
        for i in 0..300 {
            let v = block.next_f64();
            match i {
                0..=99 => assert_eq!(v, 0.0, "holds the previous target until the offset"),
                100 => assert!(v > 0.0 && v < 1.0, "the glide begins at the offset: {v}"),
                _ => assert!(v >= before, "monotone toward the new target"),
            }
            before = v;
        }
    }

    #[test]
    fn values_honor_the_declared_range() {
        let (p, mut c) = prepared(Smooth::Off, 48_000.0, 0.5);
        let points = [point(3, 9.0)];
        let mut block = c.ramp_over(ParamBlock::new(&points), &p);
        for i in 0..8 {
            let v = block.next_f64();
            assert!((0.0..=1.0).contains(&v), "sample {i} escaped: {v}");
        }
    }

    #[test]
    fn exp_settles_onto_the_target() {
        let (p, mut c) = prepared(Smooth::ms(1.0), 48_000.0, 0.0);
        render(&mut c, &p, 1.0, 4_800);
        assert_eq!(
            c.current(),
            1.0,
            "100 τ later: exactly the target, not an asymptote"
        );
    }

    #[test]
    fn steps_keep_exact_frames() {
        let points = [
            ParamChange {
                id: ParamId(9),
                frame: 2,
                value: 1.0,
                kind: ParamChangeKind::Value,
            },
            ParamChange {
                id: ParamId(9),
                frame: 5,
                value: 0.0,
                kind: ParamChangeKind::Value,
            },
        ];
        let hold = BoolParam::new(ParamId(9), "Hold", false);
        let mut steps = Steps::new(&hold);
        let changes: Vec<_> = steps.changes_over(ParamBlock::new(&points), &hold).collect();
        assert_eq!(
            changes,
            [
                super::Step {
                    frame: 2,
                    value: true
                },
                super::Step {
                    frame: 5,
                    value: false
                },
            ]
        );
        assert!(!steps.value());
    }

    #[test]
    fn deferred_applies_only_at_the_safe_point() {
        let mut d = Deferred::new(2u8);
        d.set(4);
        assert_eq!(*d.active(), 2);
        assert_eq!(*d.apply(), 4);
    }
}
