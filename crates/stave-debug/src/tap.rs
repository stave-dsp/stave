//! `Tap` — the typed observation handle.

use core::sync::atomic::{AtomicU16, Ordering};

use crate::{DebugBus, MAX_PROBES, NAME_BYTES};

/// A DECLARED observation point:
/// the wire name is data on the declaration — engines write values
/// through the handle and never spell name strings at publish sites;
/// instruments read through the same handle. The bus stays pure
/// transport. Zero-cost unattached BY CONSTRUCTION: `set` requires a
/// `&DebugBus`, so a processor with no bus cannot even reach the
/// publish path.
pub struct Tap {
    name: &'static str,
    slot: AtomicU16,
}

impl Tap {
    /// Panics if `name` is longer than a bus slot holds — at compile
    /// time when the tap is a `static`/`const`, as declared taps are.
    pub const fn new(name: &'static str) -> Self {
        assert!(
            name.len() < NAME_BYTES,
            "Tap name longer than a DebugBus slot holds (NAME_BYTES - 1 bytes)"
        );
        Self {
            name,
            slot: AtomicU16::new(u16::MAX),
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Publish one value (audio thread; slot resolved once, cached).
    #[inline]
    pub fn set(&self, bus: &DebugBus, value: f64) {
        bus.probe_f64(&self.slot, self.name, value);
    }

    /// Instrument-side read of the latest published value (None until
    /// the tap has published on this bus).
    pub fn get(&self, bus: &DebugBus) -> Option<f64> {
        let used = bus.probes_used.load(Ordering::Acquire) as usize;
        (0..used.min(MAX_PROBES))
            .find(|&i| bus.probes[i].name.matches(self.name))
            .filter(|&i| bus.probes[i].hits.load(Ordering::Relaxed) > 0)
            .map(|i| f64::from_bits(bus.probes[i].latest.load(Ordering::Relaxed)))
    }
}
