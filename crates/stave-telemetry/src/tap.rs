//! Taps: typed, declared observation points. The engine writes through a
//! [`Tap`]; editors and tools read by [`TapId`].

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::slot::{TapSlot, NO_SLOT};
use crate::{Telemetry, NAME_BYTES};

/// A tap's identity: a name checked at compile time and the 64-bit hash
/// readers match on. `#[derive(Taps)]` declares one constant per field
/// (`HeirloomTaps::SPIN`); readers pass it, never a string.
///
/// Two ids are equal when their hashes are (the name rides along for
/// registration and for tools).
#[derive(Copy, Clone, Debug)]
pub struct TapId {
    hash: u64,
    name: &'static str,
}

impl TapId {
    /// The id named `name`. In a `const` (as the derive declares them) an
    /// empty name, or one longer than a slot holds (`NAME_BYTES - 1`
    /// bytes), is a compile error:
    ///
    /// ```
    /// const OK: stave_telemetry::TapId = stave_telemetry::TapId::new("exactly_thirty_one_bytes_long__");
    /// assert_eq!(OK.name().len(), 31);
    /// ```
    ///
    /// ```compile_fail
    /// const LONG: stave_telemetry::TapId = stave_telemetry::TapId::new("exactly_thirty_two_bytes_long___");
    /// let _ = LONG;
    /// ```
    pub const fn new(name: &'static str) -> Self {
        assert!(!name.is_empty(), "a tap name is not empty");
        assert!(
            name.len() < NAME_BYTES,
            "tap name longer than a telemetry slot holds (NAME_BYTES - 1 bytes)"
        );
        TapId {
            hash: Self::hash_of(name),
            name,
        }
    }

    /// [`new`](Self::new) without the panic: `None` for an empty or
    /// over-long name. For shims that still receive names at run time.
    pub const fn try_new(name: &'static str) -> Option<Self> {
        if name.is_empty() || name.len() >= NAME_BYTES {
            return None;
        }
        Some(TapId {
            hash: Self::hash_of(name),
            name,
        })
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub(crate) const fn hash(self) -> u64 {
        self.hash
    }

    /// FNV-1a over the name's bytes: the id a slot named `name` carries.
    pub(crate) const fn hash_of(name: &str) -> u64 {
        let bytes = name.as_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < bytes.len() {
            h ^= bytes[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
}

impl PartialEq for TapId {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for TapId {}

impl core::hash::Hash for TapId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

/// One registered tap: the engine's write handle (and a reader's cached
/// one). Registration happens once, off the audio thread; [`set`] is then
/// a bounds check and a few relaxed atomic stores — realtime safe, lock
/// free, no lookup.
///
/// A detached tap ([`Tap::detached`], or one registered on a full block)
/// accepts writes and drops them, so an engine never branches on whether
/// telemetry is attached.
///
/// [`set`]: Tap::set
pub struct Tap {
    telemetry: Option<Arc<Telemetry>>,
    slot: u16,
}

impl Tap {
    /// Register `id` on `telemetry` (find its slot, or reserve one that
    /// reads 0 until the first `set`). Allocation-free, but not for the
    /// audio thread's hot path: call it when the plugin is created.
    pub fn register(telemetry: &Arc<Telemetry>, id: TapId) -> Self {
        let slot = telemetry.register_tap(id);
        Tap {
            telemetry: (slot != NO_SLOT).then(|| Arc::clone(telemetry)),
            slot,
        }
    }

    /// A tap bound to nothing: `set` is a no-op, `get` reads 0.
    pub const fn detached() -> Self {
        Tap {
            telemetry: None,
            slot: NO_SLOT,
        }
    }

    pub fn is_attached(&self) -> bool {
        self.telemetry.is_some()
    }

    fn slot(&self) -> Option<&TapSlot> {
        self.telemetry.as_deref()?.tap_slot(self.slot)
    }

    /// Publish one value (audio thread).
    #[inline]
    pub fn set(&self, value: f64) {
        if let Some(slot) = self.slot() {
            slot.record(value);
        }
    }

    /// The latest published value (0 until the first `set`, or detached).
    pub fn get(&self) -> f64 {
        self.slot()
            .map_or(0.0, |s| f64::from_bits(s.latest.load(Ordering::Relaxed)))
    }
}

impl Default for Tap {
    fn default() -> Self {
        Self::detached()
    }
}

/// One tap slot as a tool sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct TapReading {
    pub name: String,
    pub latest: f64,
    /// `+∞` until the first value.
    pub min: f64,
    /// `−∞` until the first value.
    pub max: f64,
    pub hits: u64,
}

impl Telemetry {
    /// The latest value of tap `id`; `None` until it has published.
    pub fn value(&self, id: TapId) -> Option<f64> {
        let slot = self.tap_slot(self.find_tap(id)?)?;
        (slot.hits.load(Ordering::Relaxed) > 0)
            .then(|| f64::from_bits(slot.latest.load(Ordering::Relaxed)))
    }

    /// Every published slot, in registration order (tools: the debugger,
    /// the bench). Allocates.
    pub fn taps(&self) -> Vec<TapReading> {
        let f = |a: &core::sync::atomic::AtomicU64| f64::from_bits(a.load(Ordering::Relaxed));
        self.tap_slots()
            .iter()
            .map(|s| TapReading {
                name: s.name.as_str().to_string(),
                latest: f(&s.latest),
                min: f(&s.min),
                max: f(&s.max),
                hits: s.hits.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Forget every slot's min, max and hit count (tools).
    pub fn reset_taps(&self) {
        self.tap_slots().iter().for_each(TapSlot::reset);
    }
}
