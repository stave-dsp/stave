//! Write-once, atomically-published inline strings — the slot-name
//! primitive behind every telemetry registry (tap and breakpoint names).

use crate::sync::{zeroed, AtomicBool, Ordering, UnsafeCell};

/// A fixed-capacity, write-once string slot with its own publication flag.
///
/// Writing a plain `[u8; N]` through a pointer cast from `&self` would
/// be provenance-UB, and publishing the slot before its bytes would let
/// a GUI thread iterating `0..used` read a name mid-write. Here the bytes
/// live in an `UnsafeCell` (writes through `&self` are sanctioned), and
/// `as_str` returns `""` until `ready` is published with Release ordering
/// *after* the bytes are complete, so no reader ever dereferences bytes
/// that are still being written.
///
/// Write protocol: exactly one thread calls `set`, exactly once, while it
/// holds exclusive ownership of the slot (won the reservation CAS or is
/// the single init thread). Enforced by a debug assertion, relied on by
/// the SAFETY argument in `as_str`.
#[repr(C)]
pub struct InlineStr<const N: usize> {
    ready: AtomicBool,
    bytes: UnsafeCell<[u8; N]>,
}

// SAFETY: `bytes` is only mutated by the single owner before `ready` is
// published (Release); readers only dereference after observing `ready`
// (Acquire), which orders the write before the read. After publication
// the bytes are immutable for the lifetime of the bus.
unsafe impl<const N: usize> Sync for InlineStr<N> {}

impl<const N: usize> InlineStr<N> {
    zeroed! {
        pub fn zeroed() -> Self {
            Self {
                ready: AtomicBool::new(false),
                bytes: UnsafeCell::new([0; N]),
            }
        }
    }

    /// Longest string (in bytes) a slot stores; longer ones are truncated.
    pub const CAPACITY: usize = N.saturating_sub(1);

    /// The form `s` takes once stored: cut to [`CAPACITY`](Self::CAPACITY)
    /// bytes, backed off to a char boundary so the result stays UTF-8.
    /// Every lookup compares against this, so an over-long name still
    /// matches the slot it was stored in.
    pub fn truncate(s: &str) -> &str {
        let mut take = s.len().min(Self::CAPACITY);
        while !s.is_char_boundary(take) {
            take -= 1;
        }
        &s[..take]
    }

    /// Whether this slot holds `s` (compared in its truncated form).
    /// An unpublished slot matches nothing — not even `""`.
    pub fn matches(&self, s: &str) -> bool {
        self.is_set() && self.as_str() == Self::truncate(s)
    }

    /// Publish the string, truncated per [`truncate`](Self::truncate)
    /// (nul-padded). Must be called at most once, by the slot's exclusive
    /// owner (see type docs).
    pub fn set(&self, s: &str) {
        debug_assert!(
            !self.ready.load(Ordering::Relaxed),
            "InlineStr::set called twice on the same slot"
        );
        let s = Self::truncate(s);
        let take = s.len();
        // SAFETY: exclusive ownership per the write protocol — no other
        // thread writes, and no reader dereferences until `ready` below.
        unsafe {
            self.bytes.write(|dst| {
                dst[..take].copy_from_slice(&s.as_bytes()[..take]);
                dst[take] = 0;
            });
        }
        self.ready.store(true, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// Read the published string; `""` if not (yet) published. Non-UTF-8
    /// content (impossible via `set`, conceivable via ABI misuse) also
    /// reads as `""` — never a panic.
    pub fn as_str(&self) -> &str {
        if !self.ready.load(Ordering::Acquire) {
            return "";
        }
        // SAFETY: `ready` was observed with Acquire, so the Release store
        // in `set` — and every byte written before it — happens-before
        // this read, and the bytes are immutable from then on.
        let bytes = unsafe { self.bytes.read() };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        core::str::from_utf8(&bytes[..end]).unwrap_or("")
    }
}

impl<const N: usize> core::fmt::Debug for InlineStr<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Kani, over every UTF-8 string of up to six bytes and a 4-byte slot:
/// truncation keeps the longest prefix that fits and ends on a char
/// boundary, and a stored name reads back as — and matches — exactly
/// that form.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(8)]
    fn truncation_is_the_longest_fitting_prefix_and_round_trips() {
        type Slot = InlineStr<4>;
        let bytes: [u8; 6] = kani::any();
        let len: usize = kani::any_where(|l: &usize| *l <= 6);
        let Ok(s) = core::str::from_utf8(&bytes[..len]) else {
            return;
        };
        let t = Slot::truncate(s);
        assert!(s.starts_with(t));
        assert!(t.len() <= Slot::CAPACITY);
        // Longest: the next char would not fit.
        if t.len() < s.len() {
            let next = s[t.len()..].chars().next().unwrap();
            assert!(t.len() + next.len_utf8() > Slot::CAPACITY);
        }
        let slot = Slot::zeroed();
        assert!(!slot.matches(s), "unpublished matches nothing");
        slot.set(s);
        // A name with an interior nul reads back cut at the nul.
        if !t.contains('\0') {
            assert_eq!(slot.as_str(), t);
            assert!(slot.matches(s));
        }
    }
}
