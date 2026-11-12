//! The typed parameter kinds: [`FloatParam`], [`BoolParam`],
//! [`EnumParam`]. A plugin declares these as struct fields (`#[derive(Params)]`
//! generates the table glue); the audio thread reads them as
//! fields — a typo is a compile error, a read is an atomic load.
//!
//! Identity: every param carries an author-chosen [`ParamId`] that
//! NEVER derives from declaration order. Reordering, adding, or
//! removing fields must not shift existing ids — saved automation
//! lanes and presets key on them.
//!
//! Value model: the atomically-stored PLAIN value is the single
//! host/state-visible truth. Normalized exists only at the wrapper
//! boundary (via [`Range`]); smoothing is render state, owned by the
//! processor that renders it ([`ContinuousControl`](super::ContinuousControl)).

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::params::ParamId;

extern crate alloc;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::display::{Unit, ValueText};
use super::smooth::Smooth;
use super::taper::Range;

/// Behavioral flags, additive. Defaults: automatable, visible.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamFlags {
    /// The host bypass param (VST3 `kIsBypass`, CLAP `is_bypass`).
    pub bypass: bool,
    /// Excluded from host automation lists (editor-internal knobs).
    pub non_automatable: bool,
    /// Hidden from generic editors; still automatable unless flagged.
    pub hidden: bool,
}

/// Declaration shared by every param kind.
#[derive(Clone, Debug)]
pub struct ParamMeta {
    pub id: ParamId,
    pub name: &'static str,
    pub group: &'static str,
    pub unit: Unit,
    pub flags: ParamFlags,
}

// ===========================================================================
// FloatParam
// ===========================================================================

/// A continuous parameter: plain f64 target + declared taper, unit, and
/// framework-owned smoothing.
#[derive(Debug)]
pub struct FloatParam {
    meta: ParamMeta,
    range: Range,
    default: f64,
    smooth: Smooth,
    /// Plain-value target (f64 bits) — the host/state-visible truth.
    value: AtomicU64,
    /// Bumped when the value is restored (session load), not moved:
    /// processor-owned controls jump to a restored value instead of
    /// gliding to it.
    restores: AtomicU32,
}

impl FloatParam {
    /// Declare a param. `id` is FOREVER: pick it once, never reuse or
    /// renumber (see module docs).
    pub fn new(id: u32, name: &'static str, default: f64, range: Range) -> Self {
        let default = default.clamp(range.min, range.max);
        Self {
            meta: ParamMeta {
                id: ParamId(id),
                name,
                group: "",
                unit: Unit::Plain { decimals: 2 },
                flags: ParamFlags::default(),
            },
            range,
            default,
            smooth: Smooth::DEFAULT,
            value: AtomicU64::new(default.to_bits()),
            restores: AtomicU32::new(0),
        }
    }

    pub fn with_unit(mut self, unit: Unit) -> Self {
        self.meta.unit = unit;
        self
    }

    pub fn with_group(mut self, group: &'static str) -> Self {
        self.meta.group = group;
        self
    }

    pub fn with_smooth(mut self, smooth: Smooth) -> Self {
        self.smooth = smooth;
        self
    }

    pub fn with_flags(mut self, flags: ParamFlags) -> Self {
        self.meta.flags = flags;
        self
    }

    /// Host/state-visible target, plain units.
    pub fn plain(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Acquire))
    }

    /// The engine read: the current value, TYPED and BOUNDED BY
    /// THE DECLARATION — already clamped to the declared range, as
    /// f32. Engines never restate their ranges at a read site (a
    /// clamp beside a read is a drift hazard: nothing keeps it equal
    /// to the declaration). `value()` is the block-rate snapshot; a
    /// per-sample glide is a processor-owned
    /// [`ContinuousControl`](super::ContinuousControl).
    #[inline]
    pub fn value(&self) -> f32 {
        self.plain().clamp(self.range.min, self.range.max) as f32
    }

    pub fn range(&self) -> Range {
        self.range
    }

    /// Stable identity copied into processor-owned controls.
    pub fn id(&self) -> ParamId {
        self.meta.id
    }

    /// Declared smoothing copied into processor-owned controls.
    pub fn smoothing(&self) -> Smooth {
        self.smooth
    }

    /// The restore count (see [`AnyParam::restore_plain`]): it moves on
    /// every restore and is odd while one is being written. Readers load
    /// it, then the value, then it again (all `Acquire`) and snap when it
    /// moved, is odd, or differs from the count they last saw.
    pub fn restores(&self) -> u32 {
        self.restores.load(Ordering::Acquire)
    }
}

// ===========================================================================
// BoolParam
// ===========================================================================

/// A two-state parameter. Unsmoothed by nature; DSP that needs a
/// click-free toggle crossfades on its own terms (or uses the
/// framework bypass, Step 4).
#[derive(Debug)]
pub struct BoolParam {
    meta: ParamMeta,
    default: bool,
    value: AtomicBool,
    names: [&'static str; 2], // [off, on]
}

impl BoolParam {
    pub fn new(id: u32, name: &'static str, default: bool) -> Self {
        Self {
            meta: ParamMeta {
                id: ParamId(id),
                name,
                group: "",
                unit: Unit::Plain { decimals: 0 },
                flags: ParamFlags::default(),
            },
            default,
            value: AtomicBool::new(default),
            names: ["off", "on"],
        }
    }

    pub fn with_group(mut self, group: &'static str) -> Self {
        self.meta.group = group;
        self
    }

    pub fn with_flags(mut self, flags: ParamFlags) -> Self {
        self.meta.flags = flags;
        self
    }

    /// Display names for the two states (default "off"/"on").
    pub fn with_names(mut self, off: &'static str, on: &'static str) -> Self {
        self.names = [off, on];
        self
    }

    pub fn get(&self) -> bool {
        self.value.load(Ordering::Acquire)
    }

    /// The engine read: the current state, as the bool it is.
    #[inline]
    pub fn value(&self) -> bool {
        self.get()
    }
}

// ===========================================================================
// EnumParam
// ===========================================================================

/// Implemented by selector enums (`param_enum!` writes the impl).
pub trait ParamEnum: Copy + Send + Sync + 'static {
    const COUNT: u32;
    fn from_index(i: u32) -> Self;
    fn to_index(self) -> u32;
    fn name(i: u32) -> &'static str;
}

/// Declare a selector in one breath: the label slice (the single
/// source of truth editors and hosts both read) and its index
/// newtype with a clamped [`ParamEnum`] impl. Doc comments attach to
/// the LABELS — that's where the meaning lives.
///
/// ```
/// stave_core::param_enum! {
///     /// The oversampling menu.
///     pub const MODES: Mode = ["1x", "2x", "4x"];
/// }
/// assert_eq!(<Mode as stave_core::params::ParamEnum>::COUNT, 3);
/// ```
#[macro_export]
macro_rules! param_enum {
    ($(#[$doc:meta])* $vis:vis const $labels:ident : $name:ident = [$($label:expr),+ $(,)?];) => {
        $(#[$doc])*
        $vis const $labels: &[&str] = &[$($label),+];

        #[doc = concat!("Selector index over [`", stringify!($labels), "`] — see there for what the positions mean.")]
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        $vis struct $name(pub u32);

        impl $crate::params::ParamEnum for $name {
            const COUNT: u32 = $labels.len() as u32;
            fn from_index(i: u32) -> Self {
                Self(i.min(Self::COUNT - 1))
            }
            fn to_index(self) -> u32 {
                self.0
            }
            fn name(i: u32) -> &'static str {
                $labels[(i as usize).min($labels.len() - 1)]
            }
        }
    };
}

/// A discrete selector over a [`ParamEnum`]. Hosts see a stepped
/// parameter with named positions.
#[derive(Debug)]
pub struct EnumParam<T: ParamEnum> {
    meta: ParamMeta,
    default: u32,
    index: AtomicU32,
    _marker: core::marker::PhantomData<T>,
}

impl<T: ParamEnum> EnumParam<T> {
    pub fn new(id: u32, name: &'static str, default: T) -> Self {
        assert!(T::COUNT >= 2, "EnumParam needs at least 2 positions");
        Self {
            meta: ParamMeta {
                id: ParamId(id),
                name,
                group: "",
                unit: Unit::Plain { decimals: 0 },
                flags: ParamFlags::default(),
            },
            default: default.to_index(),
            index: AtomicU32::new(default.to_index()),
            _marker: core::marker::PhantomData,
        }
    }

    pub fn with_group(mut self, group: &'static str) -> Self {
        self.meta.group = group;
        self
    }

    pub fn with_flags(mut self, flags: ParamFlags) -> Self {
        self.meta.flags = flags;
        self
    }

    pub fn get(&self) -> T {
        T::from_index(self.index.load(Ordering::Acquire).min(T::COUNT - 1))
    }

    /// The engine read: the current selection as a plain index,
    /// bounded by the declared variant count — what a `match` over
    /// selector positions wants, without `.get().0 as usize`.
    #[inline]
    pub fn index(&self) -> usize {
        self.get().to_index() as usize
    }
}

// ===========================================================================
// AnyParam — the object-safe host/wrapper view
// ===========================================================================

/// What wrappers, editors, and state consume. Everything here
/// is `&self` + atomics: callable from any host thread. NOT the audio
/// read path — that's the typed field access above.
pub trait AnyParam: Send + Sync {
    fn meta(&self) -> &ParamMeta;

    /// Plain-value truth.
    fn plain(&self) -> f64;
    /// Host/state write. Clamps into range; a restore also calls
    /// [`mark_restored`](Self::mark_restored).
    fn set_plain(&self, plain: f64);

    /// Normalized 0..=1 view for host ABIs.
    fn normalized(&self) -> f64;
    fn set_normalized(&self, n: f64);
    fn default_normalized(&self) -> f64;
    fn plain_to_normalized(&self, plain: f64) -> f64;
    fn normalized_to_plain(&self, n: f64) -> f64;

    /// Display of an arbitrary plain value (hosts format automation
    /// lane positions, not just the current value).
    fn format_plain(&self, plain: f64) -> ValueText;
    /// Typed entry → plain value.
    fn parse(&self, text: &str) -> Option<f64>;

    /// 0 = continuous; N ≥ 2 = stepped with [`step_name`](Self::step_name).
    fn step_count(&self) -> u32 {
        0
    }
    fn step_name(&self, _index: u32) -> Option<&'static str> {
        None
    }

    /// The value was restored (session load), not moved: renderers jump
    /// to it rather than glide. Restored state must not fade in.
    /// Prefer [`restore_plain`](Self::restore_plain), which writes the
    /// value and the mark as one protocol.
    fn mark_restored(&self) {}
    /// Restore `plain` (session load): the value is jumped to, never
    /// glided to, by every renderer reading it concurrently.
    fn restore_plain(&self, plain: f64) {
        self.set_plain(plain);
        self.mark_restored();
    }
    /// Display hint for generic editors: logarithmic knob travel.
    fn display_log(&self) -> bool {
        false
    }
}

impl AnyParam for FloatParam {
    fn meta(&self) -> &ParamMeta {
        &self.meta
    }
    fn plain(&self) -> f64 {
        FloatParam::plain(self)
    }
    fn set_plain(&self, plain: f64) {
        let v = if plain.is_nan() {
            self.default
        } else {
            plain.clamp(self.range.min, self.range.max)
        };
        self.value.store(v.to_bits(), Ordering::Release);
    }
    fn normalized(&self) -> f64 {
        self.range.normalize(self.plain())
    }
    fn set_normalized(&self, n: f64) {
        self.set_plain(self.range.denormalize(n));
    }
    fn default_normalized(&self) -> f64 {
        self.range.normalize(self.default)
    }
    fn plain_to_normalized(&self, plain: f64) -> f64 {
        self.range.normalize(plain)
    }
    fn normalized_to_plain(&self, n: f64) -> f64 {
        self.range.denormalize(n)
    }
    fn format_plain(&self, plain: f64) -> ValueText {
        self.meta.unit.format(plain)
    }
    fn parse(&self, text: &str) -> Option<f64> {
        self.meta
            .unit
            .parse(text)
            .map(|v| v.clamp(self.range.min, self.range.max))
    }
    fn mark_restored(&self) {
        // +2 keeps the count even: no restore is in progress.
        self.restores.fetch_add(2, Ordering::Release);
    }
    fn restore_plain(&self, plain: f64) {
        // Bracket the value store with the count: odd while writing, even
        // (and moved) after. A reader that sees this value also sees an
        // odd or moved count: the Release store below orders the first
        // increment before it, and the reader's Acquire load of the value
        // orders its second count load after it. A reader that sees the
        // old value with a moved count snaps to the old value, then snaps
        // again next block. Neither interleaving glides.
        self.restores.fetch_add(1, Ordering::Release);
        self.set_plain(plain);
        self.restores.fetch_add(1, Ordering::Release);
    }
    fn display_log(&self) -> bool {
        matches!(self.range.taper, super::taper::Taper::Log)
    }
}

impl AnyParam for BoolParam {
    fn meta(&self) -> &ParamMeta {
        &self.meta
    }
    fn plain(&self) -> f64 {
        if self.get() {
            1.0
        } else {
            0.0
        }
    }
    fn set_plain(&self, plain: f64) {
        self.value.store(plain >= 0.5, Ordering::Release);
    }
    fn normalized(&self) -> f64 {
        self.plain()
    }
    fn set_normalized(&self, n: f64) {
        self.set_plain(n);
    }
    fn default_normalized(&self) -> f64 {
        if self.default {
            1.0
        } else {
            0.0
        }
    }
    fn plain_to_normalized(&self, plain: f64) -> f64 {
        if plain >= 0.5 {
            1.0
        } else {
            0.0
        }
    }
    fn normalized_to_plain(&self, n: f64) -> f64 {
        if n >= 0.5 {
            1.0
        } else {
            0.0
        }
    }
    fn format_plain(&self, plain: f64) -> ValueText {
        let mut t = ValueText::default();
        use core::fmt::Write as _;
        let _ = t.write_str(self.names[usize::from(plain >= 0.5)]);
        t
    }
    fn parse(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        if t.eq_ignore_ascii_case(self.names[1]) || t == "1" {
            Some(1.0)
        } else if t.eq_ignore_ascii_case(self.names[0]) || t == "0" {
            Some(0.0)
        } else {
            None
        }
    }
    fn step_count(&self) -> u32 {
        2
    }
    fn step_name(&self, index: u32) -> Option<&'static str> {
        self.names.get(index as usize).copied()
    }
}

impl<T: ParamEnum> AnyParam for EnumParam<T> {
    fn meta(&self) -> &ParamMeta {
        &self.meta
    }
    fn plain(&self) -> f64 {
        self.index.load(Ordering::Acquire) as f64
    }
    fn set_plain(&self, plain: f64) {
        let i = if plain.is_nan() {
            self.default
        } else {
            plain.round().max(0.0) as u32
        };
        self.index.store(i.min(T::COUNT - 1), Ordering::Release);
    }
    fn normalized(&self) -> f64 {
        self.plain() / (T::COUNT - 1) as f64
    }
    fn set_normalized(&self, n: f64) {
        self.set_plain(n.clamp(0.0, 1.0) * (T::COUNT - 1) as f64);
    }
    fn default_normalized(&self) -> f64 {
        self.default as f64 / (T::COUNT - 1) as f64
    }
    fn plain_to_normalized(&self, plain: f64) -> f64 {
        (plain / (T::COUNT - 1) as f64).clamp(0.0, 1.0)
    }
    fn normalized_to_plain(&self, n: f64) -> f64 {
        (n.clamp(0.0, 1.0) * (T::COUNT - 1) as f64).round()
    }
    fn format_plain(&self, plain: f64) -> ValueText {
        let i = (plain.round().max(0.0) as u32).min(T::COUNT - 1);
        let mut t = ValueText::default();
        use core::fmt::Write as _;
        let _ = t.write_str(T::name(i));
        t
    }
    fn parse(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        (0..T::COUNT)
            .find(|&i| t.eq_ignore_ascii_case(T::name(i)))
            .map(|i| i as f64)
    }
    fn step_count(&self) -> u32 {
        T::COUNT
    }
    fn step_name(&self, index: u32) -> Option<&'static str> {
        (index < T::COUNT).then(|| T::name(index))
    }
}

// ===========================================================================
// Params — the per-plugin table
// ===========================================================================

/// The parameter table a plugin exposes: iteration for wrappers,
/// id lookup for events/state. Step 1.3's `#[derive(Params)]`
/// implements this; hand impls are supported (and how the derive is
/// tested).
pub trait Params: Send + Sync {
    fn count(&self) -> usize;
    /// Stable declaration order (which does NOT define ids).
    fn by_index(&self, index: usize) -> Option<&dyn AnyParam>;

    fn by_id(&self, id: ParamId) -> Option<&dyn AnyParam> {
        (0..self.count())
            .filter_map(|i| self.by_index(i))
            .find(|p| p.meta().id == id)
    }

    fn by_name(&self, name: &str) -> Option<&dyn AnyParam> {
        (0..self.count())
            .filter_map(|i| self.by_index(i))
            .find(|p| p.meta().name == name)
    }

    /// The param flagged [`ParamFlags::bypass`], if any. The default
    /// scans; [`IndexedParams`] answers from a cached index.
    fn bypass(&self) -> Option<&dyn AnyParam> {
        (0..self.count())
            .filter_map(|i| self.by_index(i))
            .find(|p| p.meta().flags.bypass)
    }

    /// Wrapper-side sanity at instance creation: id uniqueness.
    /// Duplicate ids are author bugs that corrupt automation; fail
    /// loudly, first run. (No count cap: the table is Vec-shaped
    /// end to end — a former MAX_PARAMS=256 was pure ceremony, and
    /// Serum-class instruments clear 600.)
    fn validate(&self) -> Result<(), ParamTableError> {
        let n = self.count();
        for i in 0..n {
            let a = self.by_index(i).ok_or(ParamTableError::Hole { index: i })?;
            // The top bit is the VIEW key namespace (editor-local
            // page state, crate::editor::VIEW_WHEN_BIT) — a param id
            // there would collide with view bindings.
            if a.meta().id.0 >= 0x8000_0000 || a.meta().id == ParamId::PROGRAM {
                return Err(ParamTableError::ReservedId {
                    id: a.meta().id,
                    name: a.meta().name,
                });
            }
            for j in (i + 1)..n {
                let b = self.by_index(j).ok_or(ParamTableError::Hole { index: j })?;
                if a.meta().id == b.meta().id {
                    return Err(ParamTableError::DuplicateId {
                        id: a.meta().id,
                        first: a.meta().name,
                        second: b.meta().name,
                    });
                }
            }
        }
        Ok(())
    }
}

/// A table with an id → index map and the bypass param's index, built
/// once at instance creation: [`by_id`](Params::by_id) is a binary search
/// plus one `by_index`, [`bypass`](Params::bypass) one `by_index` —
/// neither scans nor allocates, so both are audio-thread safe.
/// [`StavePlugin`](crate::StavePlugin) serves its table through this.
pub struct IndexedParams<T: ?Sized> {
    /// (id, index), sorted by id; the first declaration wins a duplicate
    /// (which [`Params::validate`] refuses anyway).
    ids: Box<[(u32, u32)]>,
    bypass: Option<usize>,
    table: Arc<T>,
}

impl<T: Params + ?Sized> IndexedParams<T> {
    /// Walk `table` once (non-realtime: allocates the index).
    pub fn new(table: Arc<T>) -> Self {
        let mut ids: Vec<(u32, u32)> = (0..table.count())
            .filter_map(|i| Some((table.by_index(i)?.meta().id.0, i as u32)))
            .collect();
        // Stable: equal ids keep declaration order, so dedup keeps the first.
        ids.sort_by_key(|&(id, _)| id);
        ids.dedup_by_key(|&mut (id, _)| id);
        let bypass =
            (0..table.count()).find(|&i| table.by_index(i).is_some_and(|p| p.meta().flags.bypass));
        Self {
            ids: ids.into_boxed_slice(),
            bypass,
            table,
        }
    }

    /// The wrapped table.
    pub fn inner(&self) -> &Arc<T> {
        &self.table
    }
}

impl<T: Params + ?Sized> Params for IndexedParams<T> {
    fn count(&self) -> usize {
        self.table.count()
    }
    fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
        self.table.by_index(index)
    }
    fn by_id(&self, id: ParamId) -> Option<&dyn AnyParam> {
        let at = self.ids.binary_search_by_key(&id.0, |&(id, _)| id).ok()?;
        self.table.by_index(self.ids[at].1 as usize)
    }
    fn by_name(&self, name: &str) -> Option<&dyn AnyParam> {
        self.table.by_name(name)
    }
    fn bypass(&self) -> Option<&dyn AnyParam> {
        self.table.by_index(self.bypass?)
    }
    fn validate(&self) -> Result<(), ParamTableError> {
        self.table.validate()
    }
}

/// An array of tables is one table, in order.
impl<P: Params, const N: usize> Params for [P; N] {
    fn count(&self) -> usize {
        self.iter().map(Params::count).sum()
    }

    fn by_index(&self, mut index: usize) -> Option<&dyn AnyParam> {
        for table in self {
            let n = table.count();
            if index < n {
                return table.by_index(index);
            }
            index -= n;
        }
        None
    }
}

/// Why a parameter table is unusable. Loud and specific — these are
/// author-time bugs, not runtime conditions.
#[derive(Debug, PartialEq, Eq)]
pub enum ParamTableError {
    Hole {
        index: usize,
    },
    /// Ids with the top bit set are the VIEW-key namespace;
    /// [`ParamId::PROGRAM`] is the wrappers' preset selector.
    ReservedId {
        id: ParamId,
        name: &'static str,
    },
    DuplicateId {
        id: ParamId,
        first: &'static str,
        second: &'static str,
    },
}

impl core::fmt::Display for ParamTableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Hole { index } => write!(f, "by_index({index}) returned None inside count()"),
            Self::ReservedId { id, name } => write!(
                f,
                "param '{name}' id {} is reserved (ids must be < 0x7FFF_FFFF)",
                id.0
            ),
            Self::DuplicateId { id, first, second } => write!(
                f,
                "params '{first}' and '{second}' share id {} — ids are forever, renumber the new one",
                id.0
            ),
        }
    }
}

/// An empty table, for plugins with no parameters.
pub struct NoParams;

impl Params for NoParams {
    fn count(&self) -> usize {
        0
    }
    fn by_index(&self, _index: usize) -> Option<&dyn AnyParam> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Taper, Unit};

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Character {
        Triode,
        Diode,
        Tape,
    }

    impl ParamEnum for Character {
        const COUNT: u32 = 3;
        fn from_index(i: u32) -> Self {
            match i {
                0 => Self::Triode,
                1 => Self::Diode,
                _ => Self::Tape,
            }
        }
        fn to_index(self) -> u32 {
            self as u32
        }
        fn name(i: u32) -> &'static str {
            ["TRIODE", "DIODE", "TAPE"][i as usize % 3]
        }
    }

    /// Hand-written table — the shape the derive will generate, and
    /// the proof plugins can implement Params without the macro.
    struct TestParams {
        drive: FloatParam,
        character: EnumParam<Character>,
        bypass: BoolParam,
    }

    impl TestParams {
        fn new() -> Self {
            Self {
                drive: FloatParam::new(1, "drive", 0.0, Range::new(-12.0, 36.0, Taper::Linear))
                    .with_unit(Unit::Db { floor_db: None })
                    .with_group("Main"),
                character: EnumParam::new(2, "character", Character::Triode).with_group("Main"),
                bypass: BoolParam::new(3, "bypass", false).with_flags(ParamFlags {
                    bypass: true,
                    ..Default::default()
                }),
            }
        }
    }

    impl Params for TestParams {
        fn count(&self) -> usize {
            3
        }
        fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
            match index {
                0 => Some(&self.drive),
                1 => Some(&self.character),
                2 => Some(&self.bypass),
                _ => None,
            }
        }
    }

    /// A wide table that counts `by_index` calls: the cost of a lookup.
    struct Wide {
        params: Vec<FloatParam>,
        calls: core::sync::atomic::AtomicUsize,
    }

    impl Wide {
        fn new(n: u32) -> Self {
            // Ids out of declaration order, as authors pick them.
            let params = (0..n)
                .map(|i| {
                    let p = FloatParam::new(
                        (i * 7919) % 100_003,
                        "p",
                        0.0,
                        Range::new(0.0, 1.0, Taper::Linear),
                    );
                    if i == n - 1 {
                        p.with_flags(ParamFlags {
                            bypass: true,
                            ..Default::default()
                        })
                    } else {
                        p
                    }
                })
                .collect();
            Self {
                params,
                calls: Default::default(),
            }
        }
        fn calls(&self) -> usize {
            self.calls.swap(0, Ordering::Relaxed)
        }
    }

    impl Params for Wide {
        fn count(&self) -> usize {
            self.params.len()
        }
        fn by_index(&self, index: usize) -> Option<&dyn AnyParam> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.params.get(index).map(|p| p as &dyn AnyParam)
        }
    }

    /// The index answers every id the scan does, at one `by_index` per
    /// lookup (the scan walks to the param), and the bypass from cache.
    #[test]
    fn indexed_lookups_match_the_scan_without_scanning() {
        let wide = std::sync::Arc::new(Wide::new(600));
        let indexed = IndexedParams::new(wide.clone());
        indexed.validate().expect("unique ids");
        wide.calls();
        for p in &wide.params {
            let id = p.id();
            let found = indexed.by_id(id).expect("every declared id");
            assert_eq!(found.meta().id, id);
            assert!(wide.calls() <= 1, "one by_index per lookup");
        }
        assert!(indexed.by_id(ParamId(100_004)).is_none());
        assert_eq!(wide.calls(), 0, "a miss touches no param");
        let bypass = indexed.bypass().expect("the flagged param");
        assert!(bypass.meta().flags.bypass);
        assert_eq!(wide.calls(), 1);
        // The scan, for contrast: the last param costs the whole table.
        assert!(wide.bypass().is_some());
        assert_eq!(wide.calls(), 600);
        // A table with no bypass has none, from the cache too.
        let plain = IndexedParams::new(std::sync::Arc::new(NoParams));
        assert!(plain.bypass().is_none() && plain.by_id(ParamId(1)).is_none());
    }

    #[test]
    fn table_lookup_and_validation() {
        let p = TestParams::new();
        p.validate().expect("valid table");
        assert_eq!(p.by_id(ParamId(2)).unwrap().meta().name, "character");
        assert!(p.by_id(ParamId(99)).is_none());
    }

    #[test]
    fn duplicate_ids_fail_loudly() {
        struct Dup(FloatParam, FloatParam);
        impl Params for Dup {
            fn count(&self) -> usize {
                2
            }
            fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
                [&self.0 as &dyn AnyParam, &self.1].get(i).copied()
            }
        }
        let d = Dup(
            FloatParam::new(7, "a", 0.0, Range::new(0.0, 1.0, Taper::Linear)),
            FloatParam::new(7, "b", 0.0, Range::new(0.0, 1.0, Taper::Linear)),
        );
        match d.validate() {
            Err(ParamTableError::DuplicateId { id, first, second }) => {
                assert_eq!(id, ParamId(7));
                assert_eq!((first, second), ("a", "b"));
            }
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn float_param_host_view_round_trips_through_taper() {
        let p = TestParams::new();
        let drive = &p.drive as &dyn AnyParam;
        drive.set_normalized(0.5);
        assert!((drive.plain() - 12.0).abs() < 1e-12); // -12..36 midpoint
        assert_eq!(&*drive.format_plain(drive.plain()), "12.0 dB");
        drive.set_plain(999.0); // clamps
        assert_eq!(drive.plain(), 36.0);
        assert_eq!(drive.normalized(), 1.0);
        drive.set_plain(f64::NAN); // NaN → default, never stored
        assert!(drive.plain().is_finite());
        assert_eq!(drive.parse("-6 dB"), Some(-6.0));
    }

    #[test]
    fn audio_read_path_is_smoothed_and_typed() {
        let p = TestParams::new();
        let mut drive = crate::params::ContinuousControl::new(&p.drive);
        drive.prepare(48_000.0, &p.drive);
        (&p.drive as &dyn AnyParam).set_plain(6.0);
        // prepare() snapped to the pre-set value 0.0; the new target
        // glides — first sample must move toward 6 but not arrive.
        let mut ramp = drive.begin_block(crate::process::ParamBlock::empty(), &p.drive);
        let first = ramp.next_f64();
        assert!(first > 0.0 && first < 6.0, "glide, not step: {first}");
        // Typed enum access on the audio thread: a plain match, no
        // strings, no floats.
        assert_eq!(p.character.get(), Character::Triode);
        (&p.character as &dyn AnyParam).set_normalized(1.0);
        assert_eq!(p.character.get(), Character::Tape);
        assert!(!p.bypass.get());
    }

    #[test]
    fn enum_param_stepped_contract() {
        let p = TestParams::new();
        let c = &p.character as &dyn AnyParam;
        assert_eq!(c.step_count(), 3);
        assert_eq!(c.step_name(1), Some("DIODE"));
        assert_eq!(c.step_name(3), None);
        // Normalized midpoint rounds to the middle step.
        c.set_normalized(0.5);
        assert_eq!(c.plain(), 1.0);
        assert_eq!(&*c.format_plain(1.0), "DIODE");
        assert_eq!(c.parse("tape"), Some(2.0));
        assert_eq!(c.parse("fuzz"), None);
        // Out-of-range plain clamps to the last step.
        c.set_plain(99.0);
        assert_eq!(c.plain(), 2.0);
    }

    /// Host thread writes targets while the audio thread renders
    /// ramps — the exact production concurrency. Values must stay
    /// finite and in range under Miri's race detection.
    #[test]
    fn concurrent_host_writes_during_audio_ramps() {
        use std::sync::Arc;
        let p = Arc::new(TestParams::new());
        let mut drive = crate::params::ContinuousControl::new(&p.drive);
        drive.prepare(48_000.0, &p.drive);
        let host = {
            let p = Arc::clone(&p);
            std::thread::spawn(move || {
                for i in 0..50 {
                    (&p.drive as &dyn AnyParam).set_normalized(f64::from(i % 5) / 4.0);
                    (&p.character as &dyn AnyParam).set_normalized(f64::from(i % 3) / 2.0);
                }
            })
        };
        for _ in 0..50 {
            let mut ramp = drive.begin_block(crate::process::ParamBlock::empty(), &p.drive);
            for _ in 0..8 {
                let v = ramp.next_f64();
                assert!(
                    v.is_finite() && (-12.0..=36.0).contains(&v),
                    "escaped range: {v}"
                );
            }
            let _ = p.character.get();
        }
        host.join().unwrap();
    }

    #[test]
    fn bool_param_names_and_parse() {
        let b = BoolParam::new(9, "power", true).with_names("BYPASSED", "ACTIVE");
        let any = &b as &dyn AnyParam;
        assert_eq!(&*any.format_plain(1.0), "ACTIVE");
        assert_eq!(any.parse("bypassed"), Some(0.0));
        assert_eq!(any.step_count(), 2);
        assert_eq!(any.default_normalized(), 1.0);
    }
}
