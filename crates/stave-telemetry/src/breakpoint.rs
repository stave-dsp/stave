//! Breakpoints (tooling): named arm/condition pairs that can pause (mute)
//! output. Evaluated by `brk_if!`; armed and read through [`Breakpoint`].

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use crate::inline_str::InlineStr;
use crate::slot::reserve_index;
use crate::sync::zeroed;
use crate::{Telemetry, MAX_BREAKPOINTS, NAME_BYTES};

#[repr(C)]
pub(crate) struct BreakpointSlot {
    name: InlineStr<NAME_BYTES>,
    armed: AtomicBool,
    mute_on_hit: AtomicBool,
    hit_count: AtomicU64,
    last_hit_block: AtomicU64,
    /// Last block where the *condition* was true (armed or not).
    last_condition_block: AtomicU64,
}

impl BreakpointSlot {
    zeroed! {
        pub(crate) fn zeroed() -> Self {
            Self {
                name: InlineStr::zeroed(),
                armed: AtomicBool::new(false),
                mute_on_hit: AtomicBool::new(true),
                hit_count: AtomicU64::new(0),
                last_hit_block: AtomicU64::new(0),
                last_condition_block: AtomicU64::new(0),
            }
        }
    }
}

/// One registered breakpoint, as a tool sees and drives it.
#[derive(Copy, Clone)]
pub struct Breakpoint<'a>(&'a BreakpointSlot);

impl<'a> Breakpoint<'a> {
    pub fn name(self) -> &'a str {
        self.0.name.as_str()
    }
    pub fn armed(self) -> bool {
        self.0.armed.load(Ordering::Acquire)
    }
    pub fn set_armed(self, armed: bool) {
        self.0.armed.store(armed, Ordering::Release);
    }
    /// Whether a hit pauses (mutes) the output.
    pub fn mute_on_hit(self) -> bool {
        self.0.mute_on_hit.load(Ordering::Acquire)
    }
    pub fn set_mute_on_hit(self, mute: bool) {
        self.0.mute_on_hit.store(mute, Ordering::Release);
    }
    pub fn hits(self) -> u64 {
        self.0.hit_count.load(Ordering::Relaxed)
    }
    pub fn last_hit_block(self) -> u64 {
        self.0.last_hit_block.load(Ordering::Relaxed)
    }
    pub fn last_condition_block(self) -> u64 {
        self.0.last_condition_block.load(Ordering::Relaxed)
    }
}

impl Telemetry {
    /// Every registered breakpoint, in registration order.
    pub fn breakpoints(&self) -> impl Iterator<Item = Breakpoint<'_>> {
        let used = self.breakpoints_used.load(Ordering::Acquire) as usize;
        self.breakpoints[..used.min(MAX_BREAKPOINTS)].iter().map(Breakpoint)
    }

    /// Zero every breakpoint's hit count (tools).
    pub fn reset_breakpoint_hits(&self) {
        for bp in self.breakpoints() {
            bp.0.hit_count.store(0, Ordering::Relaxed);
        }
    }

    fn resolve_breakpoint(&self, name: &str) -> u16 {
        let used = self.breakpoints_used.load(Ordering::Acquire) as usize;
        if let Some(i) = self.breakpoints[..used.min(MAX_BREAKPOINTS)]
            .iter()
            .position(|b| b.name.matches(name))
        {
            return i as u16;
        }
        match reserve_index(&self.breakpoints_used, MAX_BREAKPOINTS) {
            Some(i) => {
                self.breakpoints[i as usize].name.set(name);
                i
            }
            None => u16::MAX,
        }
    }

    /// The `brk_if!` macro's entry: evaluate a breakpoint. Counts the
    /// condition whenever `cond` is true; if the breakpoint is armed,
    /// counts a hit and (if it mutes on hit) raises `paused`.
    ///
    /// Returns true if the audio thread should mute the rest of this block.
    #[doc(hidden)]
    pub fn breakpoint_check(&self, slot_cache: &AtomicU16, name: &str, cond: bool) -> bool {
        let mut idx = slot_cache.load(Ordering::Relaxed);
        // Cache-as-hint; see `probe_f64` for the multi-instance rationale.
        if (idx as usize) >= MAX_BREAKPOINTS || !self.breakpoints[idx as usize].name.matches(name) {
            idx = self.resolve_breakpoint(name);
            slot_cache.store(idx, Ordering::Relaxed);
        }
        if (idx as usize) >= MAX_BREAKPOINTS || !cond {
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
