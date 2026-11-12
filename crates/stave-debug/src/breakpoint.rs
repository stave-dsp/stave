//! Breakpoints: named arm/condition pairs that can pause (mute) output.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use crate::{DebugBus, InlineStr, MAX_BREAKPOINTS, NAME_BYTES};

#[repr(C)]
pub struct Breakpoint {
    pub name: InlineStr<NAME_BYTES>,
    pub armed: AtomicBool,
    pub mute_on_hit: AtomicBool,
    pub hit_count: AtomicU64,
    pub last_hit_block: AtomicU64,
    /// Last block index where the *condition* was true (whether armed or not).
    pub last_condition_block: AtomicU64,
}

impl Breakpoint {
    pub(crate) const fn zeroed() -> Self {
        Self {
            name: InlineStr::zeroed(),
            armed: AtomicBool::new(false),
            mute_on_hit: AtomicBool::new(true),
            hit_count: AtomicU64::new(0),
            last_hit_block: AtomicU64::new(0),
            last_condition_block: AtomicU64::new(0),
        }
    }

    pub fn name_str(&self) -> &str {
        self.name.as_str()
    }
}

impl DebugBus {
    fn resolve_breakpoint(&self, name: &str) -> u16 {
        let used = self.breakpoints_used.load(Ordering::Acquire) as usize;
        for i in 0..used {
            if self.breakpoints[i].name_str() == name {
                return i as u16;
            }
        }
        loop {
            let cur = self.breakpoints_used.load(Ordering::Relaxed);
            if cur as usize >= MAX_BREAKPOINTS {
                return u16::MAX;
            }
            let next = cur + 1;
            if self
                .breakpoints_used
                .compare_exchange(cur, next, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                self.breakpoints[cur as usize].name.set(name);
                return cur;
            }
        }
    }

    /// Audio-thread entry: evaluate a breakpoint. Increments the
    /// condition-hit counter unconditionally when `cond` is true; if the
    /// breakpoint is armed, increments the hit counter and (if the
    /// breakpoint requests mute) raises the bus's `paused` flag.
    ///
    /// Returns true if the audio thread should mute the rest of this block.
    pub fn breakpoint_check(&self, slot_cache: &AtomicU16, name: &str, cond: bool) -> bool {
        let mut idx = slot_cache.load(Ordering::Relaxed);
        // Cache-as-hint; see probe_f64 for the multi-instance rationale.
        if (idx as usize) >= MAX_BREAKPOINTS || self.breakpoints[idx as usize].name_str() != name {
            idx = self.resolve_breakpoint(name);
            slot_cache.store(idx, Ordering::Relaxed);
        }
        if (idx as usize) >= MAX_BREAKPOINTS {
            return false;
        }
        if !cond {
            return false;
        }
        let bp = &self.breakpoints[idx as usize];
        let block = self.block_counter.load(Ordering::Relaxed);
        bp.last_condition_block.store(block, Ordering::Relaxed);
        if !bp.armed.load(Ordering::Acquire) {
            return false;
        }
        bp.hit_count.fetch_add(1, Ordering::Relaxed);
        bp.last_hit_block.store(block, Ordering::Relaxed);
        if bp.mute_on_hit.load(Ordering::Acquire) {
            self.paused.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
}
