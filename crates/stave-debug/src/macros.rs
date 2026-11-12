//! The audio-thread macros: `probe!`, `brk_if!`.

/// Sample a value into the named probe. Engines should prefer a
/// declared [`Tap`](crate::Tap); this macro remains for harness/tool code.
///
/// A literal name over `NAME_BYTES - 1` bytes is a compile error:
///
/// ```
/// let bus = stave_debug::DebugBus::new();
/// stave_debug::probe!(bus, "exactly_thirty_one_bytes_long__", 1.0);
/// ```
///
/// ```compile_fail
/// let bus = stave_debug::DebugBus::new();
/// stave_debug::probe!(bus, "exactly_thirty_two_bytes_long___", 1.0);
/// ```
#[macro_export]
macro_rules! probe {
    // A literal name is checked at compile time: one longer than a slot
    // holds would be silently truncated.
    ($bus:expr, $name:literal, $value:expr) => {{
        const _: () = assert!(
            $name.len() < $crate::NAME_BYTES,
            "probe! name longer than a DebugBus slot holds (NAME_BYTES - 1 bytes)"
        );
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $bus.probe_f64(&SLOT, $name, ($value) as f64);
    }};
    ($bus:expr, $name:expr, $value:expr) => {{
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $bus.probe_f64(&SLOT, $name, ($value) as f64);
    }};
}

/// Check a breakpoint condition. If the bp is armed and the condition is
/// true, the bus marks itself paused (host mutes output until cleared).
/// Returns whether the audio thread should mute the rest of the block.
///
/// ```ignore
/// if brk_if!(bus, "gate_rise", new_gate > 0.0 && prev_gate == 0.0) {
///     // optional: zero out remaining samples manually
/// }
/// ```
#[macro_export]
macro_rules! brk_if {
    ($bus:expr, $name:literal, $cond:expr) => {{
        const _: () = assert!(
            $name.len() < $crate::NAME_BYTES,
            "brk_if! name longer than a DebugBus slot holds (NAME_BYTES - 1 bytes)"
        );
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $bus.breakpoint_check(&SLOT, $name, $cond)
    }};
    ($bus:expr, $name:expr, $cond:expr) => {{
        static SLOT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(u16::MAX);
        $bus.breakpoint_check(&SLOT, $name, $cond)
    }};
}

// We pull in alloc explicitly so the `Arc::new` doesn't need full `std`.
