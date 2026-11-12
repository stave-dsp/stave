//! The bus itself: a fixed-capacity, lock-free state container shared
//! between a plugin's audio thread and the host/editor.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::{
    Breakpoint, CaptureFrame, Messages, Probe, ABI_VERSION, CURVE_POINTS, MAX_BREAKPOINTS,
    MAX_PROBES,
};

const PLUGIN_CAPTURE: u8 = 1;

#[repr(C)]
pub struct DebugBus {
    pub abi_version: u32,
    pub probes_used: AtomicU16,
    pub breakpoints_used: AtomicU16,
    pub paused: AtomicBool,
    pub block_counter: AtomicU64,
    pub probes: [Probe; MAX_PROBES],
    pub breakpoints: [Breakpoint; MAX_BREAKPOINTS],
    /// Live transfer-curve table: `curve_used` (0 = none) + f32-bit y
    /// values over x ∈ ±CURVE_X_SPAN. Written by the plugin whenever its
    /// shape changes (atomic stores, no allocation — audio-thread safe);
    /// drawn by the editor's transfer view each tick.
    pub curve_used: AtomicU16,
    pub curve: [AtomicU32; CURVE_POINTS],
    pub capture: CaptureFrame,
    /// Who fills `capture` each block: the format wrapper (its output
    /// L/R, the default) or the plugin (`claim_capture`).
    capture_owner: AtomicU8,
    /// The plugin's recent log messages.
    pub messages: Messages,
}

impl DebugBus {
    /// The plugin writes its own capture pairs (e.g. Forge: shaper input
    /// on ch0, chain output on ch1, so the scope plots transfer
    /// trajectories) and the wrapper stops copying output. Call from
    /// `debug_attach`.
    pub fn claim_capture(&self) {
        self.capture_owner.store(PLUGIN_CAPTURE, Ordering::Relaxed);
    }

    pub fn plugin_owns_capture(&self) -> bool {
        self.capture_owner.load(Ordering::Relaxed) == PLUGIN_CAPTURE
    }

    /// Construct a fresh bus. Heap-allocate via Box so it's safe to share
    /// across an Arc — the bus is large (~kilobytes).
    pub fn new() -> alloc::sync::Arc<Self> {
        let probes = [const { Probe::zeroed() }; MAX_PROBES];
        let bus = Self {
            abi_version: ABI_VERSION,
            probes_used: AtomicU16::new(0),
            breakpoints_used: AtomicU16::new(0),
            paused: AtomicBool::new(false),
            block_counter: AtomicU64::new(0),
            probes,
            breakpoints: [const { Breakpoint::zeroed() }; MAX_BREAKPOINTS],
            curve_used: AtomicU16::new(0),
            curve: [const { AtomicU32::new(0) }; CURVE_POINTS],
            capture: CaptureFrame::zeroed(),
            capture_owner: AtomicU8::new(0),
            messages: Messages::zeroed(),
        };
        alloc::sync::Arc::new(bus)
    }

    /// Tick the per-block counter. Call once per `process()` from the wrapper
    /// (or from the plugin itself if no wrapper involvement).
    pub fn tick_block(&self) -> u64 {
        self.block_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::Release);
    }

    /// Publish the plugin's live transfer curve: `ys` are output values
    /// sampled uniformly over x ∈ [−+CURVE_X_SPAN] in
    /// shaper-input units. Atomic stores only — safe from the audio
    /// thread; call when the shape CHANGES (block-rate), not per sample.
    pub fn publish_curve(&self, ys: &[f32]) {
        let n = ys.len().min(CURVE_POINTS);
        for (i, &y) in ys.iter().take(n).enumerate() {
            self.curve[i].store(y.to_bits(), Ordering::Relaxed);
        }
        self.curve_used.store(n as u16, Ordering::Release);
    }
}
