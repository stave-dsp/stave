//! Tap slots: one published scalar each (latest, min, max, hits), found by
//! [`TapId`] or, on the tooling path, by name.

use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use crate::inline_str::InlineStr;
use crate::{
    TapId, Telemetry, F64_INFINITY_BITS, F64_NEG_INFINITY_BITS, MAX_TAPS, NAME_BYTES,
};

/// Index of "no slot" (the block is full, or a tap is detached).
pub(crate) const NO_SLOT: u16 = u16::MAX;

#[repr(C)]
pub(crate) struct TapSlot {
    /// Slot name; an unpublished slot reads as `""`. Published (Release)
    /// after `id`, so a reader that sees the name sees the id.
    pub(crate) name: InlineStr<NAME_BYTES>,
    /// The [`TapId`] hash of `name`.
    pub(crate) id: AtomicU64,
    /// f64 bit patterns.
    pub(crate) latest: AtomicU64,
    pub(crate) min: AtomicU64,
    pub(crate) max: AtomicU64,
    pub(crate) hits: AtomicU64,
}

impl TapSlot {
    pub(crate) const fn zeroed() -> Self {
        Self {
            name: InlineStr::zeroed(),
            id: AtomicU64::new(0),
            latest: AtomicU64::new(0),
            min: AtomicU64::new(F64_INFINITY_BITS),
            max: AtomicU64::new(F64_NEG_INFINITY_BITS),
            hits: AtomicU64::new(0),
        }
    }

    /// Whether this (published) slot holds tap `id`.
    fn holds(&self, id: TapId) -> bool {
        self.name.is_set() && self.id.load(Ordering::Relaxed) == id.hash()
    }

    /// Record one value: latest, hit count, running min and max.
    #[inline]
    pub(crate) fn record(&self, value: f64) {
        self.latest.store(value.to_bits(), Ordering::Relaxed);
        self.hits.fetch_add(1, Ordering::Relaxed);
        fold(&self.min, value, |new, cur| new < cur);
        fold(&self.max, value, |new, cur| new > cur);
    }

    /// Forget min, max and hits (tooling).
    pub(crate) fn reset(&self) {
        self.min.store(F64_INFINITY_BITS, Ordering::Relaxed);
        self.max.store(F64_NEG_INFINITY_BITS, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
    }
}

/// CAS `value` into `cell` while `better(value, current)`.
#[inline]
fn fold(cell: &AtomicU64, value: f64, better: impl Fn(f64, f64) -> bool) {
    let mut cur = cell.load(Ordering::Relaxed);
    while better(value, f64::from_bits(cur)) {
        match cell.compare_exchange_weak(cur, value.to_bits(), Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(seen) => cur = seen,
        }
    }
}

impl Telemetry {
    /// The published slots.
    pub(crate) fn tap_slots(&self) -> &[TapSlot] {
        let used = self.taps_used.load(Ordering::Acquire) as usize;
        &self.taps[..used.min(MAX_TAPS)]
    }

    pub(crate) fn tap_slot(&self, index: u16) -> Option<&TapSlot> {
        self.taps.get(index as usize)
    }

    /// The slot holding `id`, if one is published.
    pub(crate) fn find_tap(&self, id: TapId) -> Option<u16> {
        self.tap_slots().iter().position(|s| s.holds(id)).map(|i| i as u16)
    }

    /// The slot holding `id`, reserving one if none does. [`NO_SLOT`]
    /// when the block is full. Not for the audio thread's hot path:
    /// taps resolve once, at registration.
    pub(crate) fn register_tap(&self, id: TapId) -> u16 {
        match self.find_tap(id) {
            Some(i) => i,
            None => self.reserve(id.name(), id.hash()),
        }
    }

    /// Claim a fresh slot for (`name`, `hash`). [`NO_SLOT`] when full.
    fn reserve(&self, name: &str, hash: u64) -> u16 {
        loop {
            let cur = self.taps_used.load(Ordering::Relaxed);
            if cur as usize >= MAX_TAPS {
                return NO_SLOT;
            }
            if self
                .taps_used
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                // The CAS handed this thread exclusive ownership of index
                // `cur`; `InlineStr::set` publishes the name (and the id
                // stored before it) with Release, so concurrent scanners
                // see "" (no match) until the slot is complete.
                let slot = &self.taps[cur as usize];
                slot.reset();
                slot.latest.store(0, Ordering::Relaxed);
                slot.id.store(hash, Ordering::Relaxed);
                slot.name.set(name);
                return cur;
            }
        }
    }

    /// Find a slot by name, or reserve a fresh one (the tooling path).
    fn resolve_named(&self, name: &str) -> u16 {
        if let Some(i) = self.tap_slots().iter().position(|s| s.name.matches(name)) {
            return i as u16;
        }
        let stored = InlineStr::<NAME_BYTES>::truncate(name);
        self.reserve(stored, TapId::hash_of(stored))
    }

    /// The `probe!` macro's entry: record `value` under `name`. Tooling
    /// only — shipping engines publish through a [`Tap`](crate::Tap).
    ///
    /// The macro's slot cache is a per-callsite static, but a process can
    /// host many instances (each with its own block) sharing that
    /// callsite. The cache is therefore only a hint: a hit is trusted only
    /// if the slot's name matches, otherwise the name is re-resolved
    /// against this block.
    #[doc(hidden)]
    pub fn probe_f64(&self, slot_cache: &AtomicU16, name: &str, value: f64) {
        let mut idx = slot_cache.load(Ordering::Relaxed);
        if (idx as usize) >= MAX_TAPS || !self.taps[idx as usize].name.matches(name) {
            idx = self.resolve_named(name);
            slot_cache.store(idx, Ordering::Relaxed);
        }
        if let Some(slot) = self.tap_slot(idx) {
            slot.record(value);
        }
    }
}
