//! The tooling macros: `probe!`, `brk_if!`. String-keyed, for harness,
//! bench and debugging code; shipping engines publish through typed
//! [`Tap`](crate::Tap)s and faces read by [`TapId`](crate::TapId).

/// Record a value under a named slot (tooling only — see the crate docs).
///
/// A literal name over `NAME_BYTES - 1` bytes is a compile error:
///
/// ```
/// let telemetry = stave_telemetry::Telemetry::new();
/// stave_telemetry::probe!(telemetry, "exactly_thirty_one_bytes_long__", 1.0);
/// ```
///
/// ```compile_fail
/// let telemetry = stave_telemetry::Telemetry::new();
/// stave_telemetry::probe!(telemetry, "exactly_thirty_two_bytes_long___", 1.0);
/// ```
#[macro_export]
macro_rules! probe {
    // A literal name is checked at compile time: one longer than a slot
    // holds would be silently truncated.
    ($telemetry:expr, $name:literal, $value:expr) => {{
        const _: () = assert!(
            $name.len() < $crate::NAME_BYTES,
            "probe! name longer than a telemetry slot holds (NAME_BYTES - 1 bytes)"
        );
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $telemetry.probe_f64(&SLOT, $name, ($value) as f64);
    }};
    ($telemetry:expr, $name:expr, $value:expr) => {{
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $telemetry.probe_f64(&SLOT, $name, ($value) as f64);
    }};
}

/// Check a breakpoint condition (tooling only). If the breakpoint is
/// armed and the condition is true, telemetry is marked paused (the
/// wrapper mutes output until cleared). Returns whether the audio thread
/// should mute the rest of the block.
///
/// ```ignore
/// if brk_if!(telemetry, "gate_rise", new_gate > 0.0 && prev_gate == 0.0) {
///     // optional: zero out remaining samples manually
/// }
/// ```
#[macro_export]
macro_rules! brk_if {
    ($telemetry:expr, $name:literal, $cond:expr) => {{
        const _: () = assert!(
            $name.len() < $crate::NAME_BYTES,
            "brk_if! name longer than a telemetry slot holds (NAME_BYTES - 1 bytes)"
        );
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $telemetry.breakpoint_check(&SLOT, $name, $cond)
    }};
    ($telemetry:expr, $name:expr, $cond:expr) => {{
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $telemetry.breakpoint_check(&SLOT, $name, $cond)
    }};
}
