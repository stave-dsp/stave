//! Capacities and layout constants of the bus (its ABI).

/// Maximum number of probe slots a single bus can hold. Plugins exceeding
/// this silently no-op extra probes.
pub const MAX_PROBES: usize = 64;

/// Maximum number of breakpoint slots.
pub const MAX_BREAKPOINTS: usize = 16;

/// Max channels we can mirror into the capture frame.
pub const CAPTURE_CHANNELS: usize = 2;

/// Per-channel frame capacity of the capture buffer.
pub const CAPTURE_LEN: usize = 1024;

/// Max length of a probe/breakpoint name in bytes (incl. nul).
pub const NAME_BYTES: usize = 32;

/// ABI version. Bump if any field layout changes.
/// 9: string slots became `InlineStr` (ready flag + bytes) — layout and
/// publication protocol change; readers must gate on the ready flag.
/// 11: preset slots removed (presets are the wrappers' program
/// parameter).
/// 12: control slots removed (parameters are the plugin's typed table).
/// 13: the message ring (`DebugBus::messages`).
pub const ABI_VERSION: u32 = 13;

// f64::INFINITY.to_bits() isn't const on our MSRV (1.80); these are the
// IEEE-754 bit patterns we'd otherwise compute.
pub(crate) const F64_INFINITY_BITS: u64 = 0x7ff0_0000_0000_0000;

pub(crate) const F64_NEG_INFINITY_BITS: u64 = 0xfff0_0000_0000_0000;

/// Log messages the bus keeps, newest last.
pub const MESSAGE_SLOTS: usize = 32;

/// Text bytes per message (a multiple of 8: stored as `AtomicU64` words).
pub const MESSAGE_BYTES: usize = 248;

/// Points in the published transfer-curve table (see `publish_curve`).
pub const CURVE_POINTS: usize = 256;

/// The input domain the curve table spans: x ∈ [−CURVE_X_SPAN, +CURVE_X_SPAN]
/// in SHAPER-INPUT units (post-drive). Fixed by convention so the editor
/// and every publisher agree without extra plumbing.
pub const CURVE_X_SPAN: f64 = 3.2;
