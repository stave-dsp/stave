//! Processor-owned parameter consumers.
//!
//! Parameter objects hold the shared target used by hosts, editors, and
//! state. These types hold the render state: smoothing history, exact-offset
//! transitions, and values that are only safe to apply during activation.

use super::{FloatParam, ParamId, Range, Smooth};
use crate::process::{ParamBlock, ParamChangeKind};

/// A continuous parameter as seen by one processor instance.
///
/// Construct this beside the DSP it controls, call [`prepare`](Self::prepare)
/// on activation, and create one [`ContinuousBlock`] per process call.
#[derive(Clone, Debug)]
pub struct ContinuousControl {
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
}

impl ContinuousControl {
    /// Copy the immutable declaration and initial target from a parameter.
    pub fn new(param: &FloatParam) -> Self {
        let initial = param.plain().clamp(param.range().min, param.range().max);
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
        }
    }

    /// Derive rate-dependent state and snap to `param`'s target.
    pub fn prepare(&mut self, sample_rate: f64, param: &FloatParam) {
        debug_assert_eq!(
            param.id(),
            self.id,
            "a control prepared from another parameter"
        );
        let target = param.plain();
        match self.smooth {
            Smooth::Off => {}
            Smooth::Exp { ms } => {
                let tau = (ms / 1000.0).max(1e-6);
                self.exp_coeff = 1.0 - (-1.0 / (tau * sample_rate.max(1.0))).exp();
            }
            Smooth::Linear { ms } => {
                self.linear_len = ((ms / 1000.0) * sample_rate.max(1.0)).round().max(1.0) as u32;
            }
        }
        self.snap(target);
    }

    /// Jump to a value without a glide. Use for activation and state restore.
    pub fn snap(&mut self, target: f64) {
        let target = self.clamp(target);
        self.current = target;
        self.target = target;
        self.linear_remaining = 0;
    }

    /// Consume this block's timeline and `param`'s latest target.
    pub fn begin_block<'control, 'block>(
        &'control mut self,
        params: ParamBlock<'block>,
        param: &FloatParam,
    ) -> ContinuousBlock<'control, 'block> {
        debug_assert_eq!(
            param.id(),
            self.id,
            "a control read against another parameter"
        );
        let store_target = param.plain();
        let restores = param.restores();
        if restores != self.restores {
            self.restores = restores;
            self.snap(store_target);
        }
        let has_points = params
            .changes()
            .iter()
            .any(|change| change.id == self.id && change.kind == ParamChangeKind::Value);
        let final_target = self.clamp(store_target);
        if !has_points {
            self.retarget(final_target);
        }
        ContinuousBlock {
            control: self,
            changes: params.changes(),
            next_change: 0,
            sample_offset: 0,
            final_target,
        }
    }

    pub fn current(&self) -> f64 {
        self.current
    }

    fn clamp(&self, value: f64) -> f64 {
        if value.is_nan() {
            self.target
        } else {
            value.clamp(self.range.min, self.range.max)
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

/// The allocation-free value stream for one process call.
pub struct ContinuousBlock<'control, 'block> {
    control: &'control mut ContinuousControl,
    changes: &'block [crate::process::ParamChange],
    next_change: usize,
    sample_offset: u32,
    final_target: f64,
}

impl ContinuousBlock<'_, '_> {
    #[inline]
    #[expect(
        clippy::should_implement_trait,
        reason = "an endless per-sample stream; Option would force unwrap in every DSP loop"
    )]
    pub fn next(&mut self) -> f32 {
        self.next_f64() as f32
    }

    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        while let Some(change) = self.changes.get(self.next_change) {
            if change.sample_offset > self.sample_offset {
                break;
            }
            self.next_change += 1;
            if change.id == self.control.id && change.kind == ParamChangeKind::Value {
                self.control.retarget(change.plain_value);
            }
        }
        self.sample_offset += 1;
        self.control.next_value()
    }
}

impl Drop for ContinuousBlock<'_, '_> {
    fn drop(&mut self) {
        // The store already contains the last host point. If a caller renders
        // fewer frames than the host supplied, the next block still aims at
        // the authoritative target.
        self.control.retarget(self.final_target);
    }
}

/// One exact-offset transition from a [`DiscreteControl`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DiscreteChange<T> {
    pub sample_offset: u32,
    pub value: T,
}

/// Processor-owned state for bools, enums, and other stepped controls.
///
/// The decode function is a plain function pointer so the audio path remains
/// allocation-free and statically dispatched.
pub struct DiscreteControl<T: Copy> {
    id: ParamId,
    value: T,
    decode: fn(f64) -> T,
}

impl<T: Copy> DiscreteControl<T> {
    pub const fn new(id: ParamId, initial: T, decode: fn(f64) -> T) -> Self {
        Self {
            id,
            value: initial,
            decode,
        }
    }

    pub fn value(&self) -> T {
        self.value
    }

    pub fn begin_block<'control, 'block>(
        &'control mut self,
        params: ParamBlock<'block>,
        store_target: f64,
    ) -> DiscreteBlock<'control, 'block, T> {
        let final_value = (self.decode)(store_target);
        DiscreteBlock {
            control: self,
            changes: params.changes(),
            next_change: 0,
            final_value,
        }
    }

    pub fn snap(&mut self, plain: f64) {
        self.value = (self.decode)(plain);
    }
}

/// Exact-offset iterator for one discrete control and process call.
pub struct DiscreteBlock<'control, 'block, T: Copy> {
    control: &'control mut DiscreteControl<T>,
    changes: &'block [crate::process::ParamChange],
    next_change: usize,
    final_value: T,
}

impl<T: Copy> Iterator for DiscreteBlock<'_, '_, T> {
    type Item = DiscreteChange<T>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(change) = self.changes.get(self.next_change) {
            self.next_change += 1;
            if change.id == self.control.id && change.kind == ParamChangeKind::Value {
                let value = (self.control.decode)(change.plain_value);
                self.control.value = value;
                return Some(DiscreteChange {
                    sample_offset: change.sample_offset,
                    value,
                });
            }
        }
        None
    }
}

impl<T: Copy> Drop for DiscreteBlock<'_, '_, T> {
    fn drop(&mut self) {
        self.control.value = self.final_value;
    }
}

/// A value whose active form may only change at activation/restart.
#[derive(Clone, Debug)]
pub struct ReconfigureControl<T> {
    pending: T,
    active: T,
}

impl<T: Clone> ReconfigureControl<T> {
    pub fn new(initial: T) -> Self {
        Self {
            pending: initial.clone(),
            active: initial,
        }
    }

    pub fn set_pending(&mut self, value: T) {
        self.pending = value;
    }

    pub fn pending(&self) -> &T {
        &self.pending
    }

    pub fn active(&self) -> &T {
        &self.active
    }

    /// Apply the latest pending value at the processor's safe point.
    pub fn activate(&mut self) -> &T {
        self.active.clone_from(&self.pending);
        &self.active
    }
}

/// Declares a processor's continuous controls over named [`FloatParam`]
/// fields of a params struct: `$controls` holds one
/// [`ContinuousControl`] per field, `$ramps` one block's
/// [`ContinuousBlock`]s, and `$values` one sample's values.
///
/// ```ignore
/// stave_core::continuous_controls! {
///     pub struct Controls<MyParams> => Ramps, Values { drive, mix }
/// }
/// let mut ramps = self.controls.begin_block(ctx.params, &params);
/// // per sample:
/// let Values { drive, mix } = ramps.next();
/// ```
#[macro_export]
macro_rules! continuous_controls {
    (
        $(#[$meta:meta])*
        $vis:vis struct $controls:ident<$params:ty> => $ramps:ident, $values:ident {
            $($field:ident),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $controls {
            $($field: $crate::params::ContinuousControl),+
        }

        $vis struct $ramps<'control, 'block> {
            $($field: $crate::params::ContinuousBlock<'control, 'block>),+
        }

        #[derive(Copy, Clone, Debug)]
        $vis struct $values {
            $(pub $field: f32),+
        }

        impl $controls {
            $vis fn new(params: &$params) -> Self {
                Self { $($field: $crate::params::ContinuousControl::new(&params.$field)),+ }
            }

            $vis fn prepare(&mut self, sample_rate: f64, params: &$params) {
                $(self.$field.prepare(sample_rate, &params.$field);)+
            }

            $vis fn begin_block<'control, 'block>(
                &'control mut self,
                block: $crate::ParamBlock<'block>,
                params: &$params,
            ) -> $ramps<'control, 'block> {
                $ramps { $($field: self.$field.begin_block(block, &params.$field)),+ }
            }
        }

        impl $ramps<'_, '_> {
            #[expect(
                clippy::should_implement_trait,
                reason = "an endless per-sample stream, like ContinuousBlock::next"
            )]
            $vis fn next(&mut self) -> $values {
                $values { $($field: self.$field.next()),+ }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{AnyParam, Taper};
    use crate::process::{ParamChange, ParamChangeKind};

    fn param(smooth: Smooth) -> FloatParam {
        FloatParam::new(7, "drive", 0.0, Range::new(0.0, 1.0, Taper::Linear)).with_smooth(smooth)
    }

    #[test]
    fn continuous_steps_at_the_declared_offset() {
        let p = param(Smooth::Off);
        let mut control = ContinuousControl::new(&p);
        p.set_plain(1.0);
        let points = [ParamChange {
            id: ParamId(7),
            sample_offset: 3,
            plain_value: 1.0,
            kind: ParamChangeKind::Value,
        }];
        let mut block = control.begin_block(ParamBlock::new(&points), &p);
        assert_eq!(
            (0..6).map(|_| block.next()).collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
        );
    }

    #[test]
    fn continuous_state_is_owned_by_each_processor() {
        let p = param(Smooth::Linear { ms: 10.0 });
        let mut a = ContinuousControl::new(&p);
        let mut b = ContinuousControl::new(&p);
        a.prepare(1_000.0, &p);
        b.prepare(1_000.0, &p);
        p.set_plain(1.0);
        let mut block = a.begin_block(ParamBlock::empty(), &p);
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

    fn prepared(smooth: Smooth, rate: f64, initial: f64) -> (FloatParam, ContinuousControl) {
        let p = FloatParam::new(7, "drive", initial, Range::new(0.0, 1.0, Taper::Linear))
            .with_smooth(smooth);
        let mut control = ContinuousControl::new(&p);
        control.prepare(rate, &p);
        (p, control)
    }

    fn render(control: &mut ContinuousControl, p: &FloatParam, target: f64, n: usize) -> Vec<f64> {
        p.set_plain(target);
        let mut block = control.begin_block(ParamBlock::empty(), p);
        (0..n).map(|_| block.next_f64()).collect()
    }

    fn point(offset: u32, value: f64) -> ParamChange {
        ParamChange {
            id: ParamId(7),
            sample_offset: offset,
            plain_value: value,
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
        let (p, mut c) = prepared(Smooth::Exp { ms: 15.0 }, 48_000.0, 0.0);
        let tau = (0.015 * 48_000.0) as usize;
        let after_tau = render(&mut c, &p, 1.0, tau)[tau - 1];
        assert!(
            (after_tau - (1.0 - (-1.0f64).exp())).abs() < 0.01,
            "{after_tau}"
        );
    }

    #[test]
    fn linear_arrives_exactly_and_holds() {
        let (p, mut c) = prepared(Smooth::Linear { ms: 10.0 }, 1_000.0, 0.0);
        let out = render(&mut c, &p, 1.0, 12);
        assert!((out[4] - 0.5).abs() < 1e-12, "midpoint {}", out[4]);
        assert_eq!(out[9..], [1.0; 3], "exact arrival, then hold");
    }

    #[test]
    fn a_glide_continues_across_blocks() {
        let (p, mut c) = prepared(Smooth::Linear { ms: 10.0 }, 1_000.0, 0.0);
        let a = render(&mut c, &p, 1.0, 4);
        let b = render(&mut c, &p, 1.0, 6);
        assert!((a[3] - 0.4).abs() < 1e-12);
        assert!((b[0] - 0.5).abs() < 1e-12, "block 2 continues: {}", b[0]);
        assert_eq!(b[5], 1.0);
    }

    #[test]
    fn a_linear_retarget_restarts_from_the_current_value() {
        let (p, mut c) = prepared(Smooth::Linear { ms: 10.0 }, 1_000.0, 0.0);
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
        let (p, mut c) = prepared(Smooth::Exp { ms: 15.0 }, 48_000.0, 0.0);
        p.set_plain(0.7);
        (&p as &dyn AnyParam).mark_restored();
        let mut block = c.begin_block(ParamBlock::empty(), &p);
        assert_eq!(
            [block.next_f64(), block.next_f64()],
            [0.7, 0.7],
            "no fade-in"
        );
    }

    #[test]
    fn a_scheduled_point_retargets_an_exp_glide_at_its_offset() {
        let (p, mut c) = prepared(Smooth::Exp { ms: 1.0 }, 48_000.0, 0.0);
        p.set_plain(1.0);
        let points = [point(100, 1.0)];
        let mut block = c.begin_block(ParamBlock::new(&points), &p);
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
        let mut block = c.begin_block(ParamBlock::new(&points), &p);
        for i in 0..8 {
            let v = block.next_f64();
            assert!((0.0..=1.0).contains(&v), "sample {i} escaped: {v}");
        }
    }

    #[test]
    fn exp_settles_onto_the_target() {
        let (p, mut c) = prepared(Smooth::Exp { ms: 1.0 }, 48_000.0, 0.0);
        render(&mut c, &p, 1.0, 4_800);
        assert_eq!(
            c.current(),
            1.0,
            "100 τ later: exactly the target, not an asymptote"
        );
    }

    #[test]
    fn discrete_changes_keep_exact_offsets() {
        let points = [
            ParamChange {
                id: ParamId(9),
                sample_offset: 2,
                plain_value: 1.0,
                kind: ParamChangeKind::Value,
            },
            ParamChange {
                id: ParamId(9),
                sample_offset: 5,
                plain_value: 0.0,
                kind: ParamChangeKind::Value,
            },
        ];
        let mut control = DiscreteControl::new(ParamId(9), false, |v| v >= 0.5);
        let changes: Vec<_> = control.begin_block(ParamBlock::new(&points), 0.0).collect();
        assert_eq!(
            changes,
            [
                DiscreteChange {
                    sample_offset: 2,
                    value: true
                },
                DiscreteChange {
                    sample_offset: 5,
                    value: false
                },
            ]
        );
        assert!(!control.value());
    }

    #[test]
    fn reconfigure_applies_only_at_the_safe_point() {
        let mut control = ReconfigureControl::new(2u8);
        control.set_pending(4);
        assert_eq!(*control.active(), 2);
        assert_eq!(*control.activate(), 4);
    }
}
