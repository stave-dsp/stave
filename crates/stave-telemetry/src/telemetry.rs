//! The telemetry block itself: fixed capacity, lock-free, shared between
//! a plugin's audio thread and its editor or host.

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::breakpoint::BreakpointSlot;
use crate::capture::CaptureFrame;
use crate::slot::TapSlot;
use crate::{Messages, ABI_VERSION, CURVE_POINTS, MAX_BREAKPOINTS, MAX_TAPS};

const PLUGIN_CAPTURE: u8 = 1;

/// One plugin instance's telemetry. Allocated by the format wrapper
/// ([`Telemetry::new`]) and handed to the plugin; every field is private
/// and every method takes `&self`.
///
/// `abi_version` must stay the first field: a host reads it before
/// anything else to refuse a block from another layout.
#[repr(C)]
pub struct Telemetry {
    abi_version: u32,
    pub(crate) taps_used: AtomicU16,
    pub(crate) breakpoints_used: AtomicU16,
    pub(crate) paused: AtomicBool,
    pub(crate) block_counter: AtomicU64,
    pub(crate) taps: [TapSlot; MAX_TAPS],
    pub(crate) breakpoints: [BreakpointSlot; MAX_BREAKPOINTS],
    /// Live transfer-curve table: `curve_used` (0 = none) + f32-bit y
    /// values over x ∈ ±CURVE_X_SPAN.
    curve_used: AtomicU16,
    curve: [AtomicU32; CURVE_POINTS],
    pub(crate) capture: CaptureFrame,
    /// Who fills `capture` each block: the format wrapper (its output
    /// L/R, the default) or the plugin (`claim_capture`).
    capture_owner: AtomicU8,
    messages: Messages,
    /// This block's own `Arc`, so a [`Tap`](crate::Tap) registered
    /// through `&Telemetry` keeps the block alive. Last: the layout a
    /// host reads ends before it.
    me: Weak<Telemetry>,
}

impl Telemetry {
    /// A fresh block, on the heap (it is kilobytes) and shared: the
    /// wrapper, the plugin's taps and the editor each hold the `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            abi_version: ABI_VERSION,
            taps_used: AtomicU16::new(0),
            breakpoints_used: AtomicU16::new(0),
            paused: AtomicBool::new(false),
            block_counter: AtomicU64::new(0),
            taps: [const { TapSlot::zeroed() }; MAX_TAPS],
            breakpoints: [const { BreakpointSlot::zeroed() }; MAX_BREAKPOINTS],
            curve_used: AtomicU16::new(0),
            curve: [const { AtomicU32::new(0) }; CURVE_POINTS],
            capture: CaptureFrame::zeroed(),
            capture_owner: AtomicU8::new(0),
            messages: Messages::zeroed(),
            me: me.clone(),
        })
    }

    /// The shared handle to this block.
    pub fn shared(&self) -> Arc<Telemetry> {
        // Every `Telemetry` is built by `new`, inside its `Arc`, and
        // `&self` proves that `Arc` is alive.
        self.me.upgrade().expect("a Telemetry lives in the Arc `new` made")
    }

    /// The layout version this block was built with ([`ABI_VERSION`]).
    pub fn abi_version(&self) -> u32 {
        self.abi_version
    }

    /// The plugin writes its own capture pairs (e.g. shaper input on ch0,
    /// chain output on ch1, so the scope plots transfer trajectories) and
    /// the wrapper stops copying output. Call when attaching telemetry.
    pub fn claim_capture(&self) {
        self.capture_owner.store(PLUGIN_CAPTURE, Ordering::Relaxed);
    }

    pub fn plugin_owns_capture(&self) -> bool {
        self.capture_owner.load(Ordering::Relaxed) == PLUGIN_CAPTURE
    }

    /// Count one processed block; returns the new count. The wrapper
    /// calls it once per `process`.
    pub fn tick_block(&self) -> u64 {
        self.block_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Blocks processed so far.
    pub fn block(&self) -> u64 {
        self.block_counter.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::Release);
    }

    /// Publish the plugin's live transfer curve: `ys` are output values
    /// sampled uniformly over x ∈ [−CURVE_X_SPAN, +CURVE_X_SPAN] in
    /// shaper-input units. Atomic stores only — safe from the audio
    /// thread; call when the shape changes (block rate), not per sample.
    pub fn publish_curve(&self, ys: &[f32]) {
        let n = ys.len().min(CURVE_POINTS);
        for (slot, &y) in self.curve.iter().zip(ys.iter().take(n)) {
            slot.store(y.to_bits(), Ordering::Relaxed);
        }
        self.curve_used.store(n as u16, Ordering::Release);
    }

    /// The published transfer curve (empty until one is published).
    pub fn curve(&self) -> Vec<f32> {
        let n = (self.curve_used.load(Ordering::Acquire) as usize).min(CURVE_POINTS);
        self.curve[..n]
            .iter()
            .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
            .collect()
    }

    /// The plugin's recent log messages.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }
}
