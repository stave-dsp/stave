//! Probes: named scalars sampled per block (latest, min, max, hits).

use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use crate::{
    DebugBus, InlineStr, F64_INFINITY_BITS, F64_NEG_INFINITY_BITS, MAX_PROBES, NAME_BYTES,
};

#[repr(C)]
pub struct Probe {
    /// Slot name; unpublished slot reads as `""`.
    pub name: InlineStr<NAME_BYTES>,
    /// f64 bit-pattern.
    pub latest: AtomicU64,
    pub min: AtomicU64,
    pub max: AtomicU64,
    pub hits: AtomicU64,
}

impl Probe {
    pub(crate) const fn zeroed() -> Self {
        Self {
            name: InlineStr::zeroed(),
            latest: AtomicU64::new(0),
            min: AtomicU64::new(F64_INFINITY_BITS),
            max: AtomicU64::new(F64_NEG_INFINITY_BITS),
            hits: AtomicU64::new(0),
        }
    }

    pub fn name_str(&self) -> &str {
        self.name.as_str()
    }
}

impl DebugBus {
    /// Find a probe slot by name, or reserve a fresh one. Returns its index.
    /// `u16::MAX` means "no slot available" (the bus is full).
    fn resolve_probe(&self, name: &str) -> u16 {
        let used = self.probes_used.load(Ordering::Acquire) as usize;
        for i in 0..used {
            if self.probes[i].name.matches(name) {
                return i as u16;
            }
        }
        // Reserve a new slot.
        loop {
            let cur = self.probes_used.load(Ordering::Relaxed);
            if cur as usize >= MAX_PROBES {
                return u16::MAX;
            }
            let next = cur + 1;
            if self
                .probes_used
                .compare_exchange(cur, next, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                // The CAS handed this thread exclusive ownership of index
                // `cur`; `InlineStr::set` publishes the name with Release,
                // so concurrent scanners see "" (no match) until complete.
                let slot = &self.probes[cur as usize];
                slot.min.store(F64_INFINITY_BITS, Ordering::Relaxed);
                slot.max.store(F64_NEG_INFINITY_BITS, Ordering::Relaxed);
                slot.hits.store(0, Ordering::Relaxed);
                slot.latest.store(0u64, Ordering::Relaxed);
                slot.name.set(name);
                return cur;
            }
        }
    }

    /// Audio-thread entry: record a probe value. `name` is interned once
    /// per unique string at the cost of a linear scan on first call.
    ///
    /// The macro-side slot cache is a per-CALLSITE static, but a process
    /// can host many instances (each with its own bus) sharing that
    /// callsite. The cache is therefore only a hint: a hit is trusted
    /// only if the slot's name matches, otherwise we re-resolve against
    /// THIS bus. Costs one short string compare per call; converges to a
    /// stable hit for the common single-instance case.
    pub fn probe_f64(&self, slot_cache: &AtomicU16, name: &str, value: f64) {
        let mut idx = slot_cache.load(Ordering::Relaxed);
        if (idx as usize) >= MAX_PROBES || !self.probes[idx as usize].name.matches(name) {
            idx = self.resolve_probe(name);
            slot_cache.store(idx, Ordering::Relaxed);
        }
        if (idx as usize) >= MAX_PROBES {
            return;
        }
        let p = &self.probes[idx as usize];
        let bits = value.to_bits();
        p.latest.store(bits, Ordering::Relaxed);
        p.hits.fetch_add(1, Ordering::Relaxed);
        // min / max update via load-CAS loop. Cheap when value is in range.
        let new_lo = value;
        let new_hi = value;
        loop {
            let cur_bits = p.min.load(Ordering::Relaxed);
            let cur = f64::from_bits(cur_bits);
            if new_lo >= cur {
                break;
            }
            if p.min
                .compare_exchange_weak(
                    cur_bits,
                    new_lo.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
        loop {
            let cur_bits = p.max.load(Ordering::Relaxed);
            let cur = f64::from_bits(cur_bits);
            if new_hi <= cur {
                break;
            }
            if p.max
                .compare_exchange_weak(
                    cur_bits,
                    new_hi.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }
}
