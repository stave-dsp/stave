//! Shared machinery for format wrappers (VST3, CLAP, future AU).
//!
//! Host ABIs hand every entry point the same raw instance pointer from
//! whatever thread the host likes. The types here turn that into a sound
//! Rust protocol instead of fabricated `&mut`:
//!
//! - [`SharedState`] — the fields both the main thread and the audio
//!   thread legitimately touch (sample rate, block size, activation,
//!   latency), as plain atomics. Interface objects hold `&SharedState`
//!   and never need exclusive access.
//! - [`PluginCell`] / [`ProcessorGuard`] — single-owner access to the
//!   processor-side state. Exclusive access is a *host protocol*
//!   invariant (a compliant host never overlaps `process` with
//!   `setActive`); the cell enforces it with an atomic claim token, so a
//!   protocol violation degrades to a refused call instead of a data
//!   race.
//! - [`ProcessorState`] — the plugin plus preallocated scratch for
//!   in-place hosts (input and output pointing at the same memory, which
//!   Live and others do routinely and which `AudioBuffer`'s no-alias
//!   contract forbids passing through directly).
//! - [`ranges_alias`] — the overlap detector wrappers use to decide when
//!   the scratch copy is needed.

mod panic_guard;
pub mod state;

pub use panic_guard::PanicGuard;

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

extern crate alloc;
use alloc::vec::Vec;

use crate::plugin::Plugin;

/// Cross-thread instance state. Every field is an atomic: interface
/// objects read and write through `&self` from any thread the host picks.
#[derive(Debug)]
pub struct SharedState {
    /// f32 bit pattern (atomics have no native f32 on all targets).
    sample_rate_bits: AtomicU32,
    max_frames: AtomicU32,
    activated: AtomicBool,
    /// Mirror of `Plugin::latency_samples`, refreshed on activate. Hosts
    /// query latency from the main thread while audio runs; reading the
    /// plugin directly there would alias the audio thread's exclusive
    /// borrow.
    latency: AtomicU32,
}

impl SharedState {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate_bits: AtomicU32::new(sample_rate.to_bits()),
            max_frames: AtomicU32::new(0),
            activated: AtomicBool::new(false),
            latency: AtomicU32::new(0),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate_bits.load(Ordering::Acquire))
    }

    pub fn set_sample_rate(&self, rate: f32) {
        self.sample_rate_bits
            .store(rate.to_bits(), Ordering::Release);
    }

    pub fn max_frames(&self) -> u32 {
        self.max_frames.load(Ordering::Acquire)
    }

    pub fn set_max_frames(&self, frames: u32) {
        self.max_frames.store(frames, Ordering::Release);
    }

    pub fn activated(&self) -> bool {
        self.activated.load(Ordering::Acquire)
    }

    pub fn set_activated(&self, on: bool) {
        self.activated.store(on, Ordering::Release);
    }

    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Acquire)
    }

    pub fn set_latency(&self, samples: u32) {
        self.latency.store(samples, Ordering::Release);
    }
}

/// The processor-side half of an instance: the plugin itself plus the
/// preallocated buffers the wrapper needs on the audio thread.
pub struct ProcessorState<P: Plugin> {
    pub plugin: P,
    /// Framework-owned click-free bypass.
    pub bypass: BypassGate,
    /// In-place scratch: one buffer per input channel, sized at activate.
    scratch: Vec<Vec<f32>>,
    scratch_ptrs: Vec<*mut f32>,
}

impl<P: Plugin> ProcessorState<P> {
    pub fn new(plugin: P) -> Self {
        Self {
            plugin,
            bypass: BypassGate::new(),
            scratch: Vec::new(),
            scratch_ptrs: Vec::new(),
        }
    }

    /// Size the in-place scratch. Call from a non-realtime context
    /// (activate / setupProcessing) — this is the only allocation site.
    pub fn prepare_scratch(&mut self, channels: usize, max_frames: usize) {
        self.scratch.resize_with(channels, Vec::new);
        for buf in &mut self.scratch {
            if buf.len() < max_frames {
                buf.resize(max_frames, 0.0);
            }
        }
        self.scratch_ptrs.clear();
        self.scratch_ptrs.reserve(channels);
    }

    /// Split every audio-thread borrow the wrapper needs at once:
    /// plugin, bypass gate, and (if `stage` is set) the inputs copied into
    /// scratch — stable, non-aliasing pointers for a host that processes
    /// in place. Allocation-free while the host keeps to the channel count
    /// and `max_frames` it declared; past them scratch grows rather than
    /// overruns.
    ///
    /// # Safety
    /// Each non-null pointer in `stage`'s inputs is valid for its `frames`
    /// f32 reads.
    pub unsafe fn split_for_block<'a>(
        &'a mut self,
        stage: Option<(&[*mut f32], usize)>,
    ) -> (&'a mut P, &'a mut BypassGate, Option<&'a [*mut f32]>) {
        let staged = stage.map(|(inputs, frames)| {
            if self.scratch.len() < inputs.len() {
                self.scratch.resize_with(inputs.len(), Vec::new);
            }
            self.scratch_ptrs.clear();
            for (buf, &src) in self.scratch.iter_mut().zip(inputs) {
                if buf.len() < frames {
                    buf.resize(frames, 0.0);
                }
                if src.is_null() {
                    buf[..frames].fill(0.0);
                } else {
                    // SAFETY: `src` is readable for `frames` (this
                    // function's contract) and `buf` is our own
                    // allocation of at least `frames`, so they cannot
                    // overlap.
                    unsafe { core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), frames) };
                }
                self.scratch_ptrs.push(buf.as_mut_ptr());
            }
            &self.scratch_ptrs[..]
        });
        (&mut self.plugin, &mut self.bypass, staged)
    }
}

/// Single-owner cell for [`ProcessorState`]. See module docs for the
/// protocol; `claim` is the only way in.
pub struct PluginCell<P: Plugin> {
    state: UnsafeCell<ProcessorState<P>>,
    claimed: AtomicBool,
}

// SAFETY: access to `state` is serialized by the `claimed` token —
// `claim` hands out at most one `ProcessorGuard` at a time, and the
// Acquire/Release pair on the token orders one owner's writes before the
// next owner's reads.
unsafe impl<P: Plugin> Sync for PluginCell<P> {}
// SAFETY: `P: Send`; the only non-`Send` field is `scratch_ptrs`, whose
// pointers address `scratch`'s heap buffers, owned by the same state and
// moved with it.
unsafe impl<P: Plugin> Send for PluginCell<P> {}

impl<P: Plugin> PluginCell<P> {
    pub fn new(plugin: P) -> Self {
        Self {
            state: UnsafeCell::new(ProcessorState::new(plugin)),
            claimed: AtomicBool::new(false),
        }
    }

    /// Claim exclusive access. `None` means another thread currently
    /// holds the processor — a host protocol violation (e.g. `process`
    /// overlapping `setActive`). Callers refuse the call (error return)
    /// instead of racing; a compliant host never sees `None`.
    pub fn claim(&self) -> Option<ProcessorGuard<'_, P>> {
        let got = self.try_claim_quiet();
        debug_assert!(
            got.is_some(),
            "processor claimed concurrently: host protocol violation"
        );
        got
    }

    /// Claim with bounded retry, for LEGITIMATE cross-thread callers:
    /// state save/load runs on the UI thread while audio may be
    /// mid-block (hosts save projects during playback). The audio
    /// thread holds the token only for one block, so a claim lands in
    /// an inter-block gap; `None` after `max_wait` means the audio
    /// thread is wedged — callers degrade (e.g. save params without
    /// the extra blob) rather than block or race.
    pub fn claim_with_retry(
        &self,
        max_wait: core::time::Duration,
    ) -> Option<ProcessorGuard<'_, P>> {
        let deadline = std::time::Instant::now() + max_wait;
        loop {
            if let Some(g) = self.try_claim_quiet() {
                return Some(g);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(core::time::Duration::from_micros(200));
        }
    }

    fn try_claim_quiet(&self) -> Option<ProcessorGuard<'_, P>> {
        self.claimed
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| ProcessorGuard { cell: self })
    }
}

/// Exclusive access to the processor state; released on drop.
pub struct ProcessorGuard<'a, P: Plugin> {
    cell: &'a PluginCell<P>,
}

impl<P: Plugin> Deref for ProcessorGuard<'_, P> {
    type Target = ProcessorState<P>;
    fn deref(&self) -> &ProcessorState<P> {
        // SAFETY: the claim token guarantees this guard is the sole
        // accessor for its lifetime.
        unsafe { &*self.cell.state.get() }
    }
}

impl<P: Plugin> DerefMut for ProcessorGuard<'_, P> {
    fn deref_mut(&mut self) -> &mut ProcessorState<P> {
        // SAFETY: as above.
        unsafe { &mut *self.cell.state.get() }
    }
}

impl<P: Plugin> Drop for ProcessorGuard<'_, P> {
    fn drop(&mut self) {
        self.cell.claimed.store(false, Ordering::Release);
    }
}

/// Framework-owned click-free bypass: a
/// latency-matched dry path with a 10 ms equal-power crossfade,
/// executed by the WRAPPER around the plugin — authors get correct
/// bypass without writing any of it. Keyed on the param flagged
/// `ParamFlags::bypass` (plain ≥ 0.5 = bypassed, the VST3 convention).
pub struct BypassGate {
    /// Delay ring per channel, sized latency + max_frames at prepare.
    dry: Vec<Vec<f32>>,
    ring_len: usize,
    write: usize,
    latency: usize,
    fade_len: u32,
    /// 0 = fully active (wet), fade_len = fully bypassed (dry).
    fade_pos: u32,
    engaged: bool,
}

impl Default for BypassGate {
    fn default() -> Self {
        Self::new()
    }
}

impl BypassGate {
    pub fn new() -> Self {
        Self {
            dry: Vec::new(),
            ring_len: 0,
            write: 0,
            latency: 0,
            fade_len: 1,
            fade_pos: 0,
            engaged: false,
        }
    }

    /// Non-realtime: size the dry ring and derive the fade length.
    pub fn prepare(
        &mut self,
        channels: usize,
        latency: usize,
        max_frames: usize,
        sample_rate: f64,
    ) {
        self.latency = latency;
        self.ring_len = (latency + max_frames).max(1);
        self.dry.resize_with(channels, Vec::new);
        for ch in &mut self.dry {
            ch.clear();
            ch.resize(self.ring_len, 0.0);
        }
        self.write = 0;
        self.fade_len = ((sample_rate * 0.010) as u32).max(1); // 10 ms
                                                               // Preserve engaged/fade state across re-prepare: a bypassed
                                                               // plugin must stay bypassed through a rate change.
        self.fade_pos = if self.engaged { self.fade_len } else { 0 };
    }

    /// Audio thread, BEFORE the plugin runs: capture this block's dry
    /// input (post-staging, so in-place hosts are already disaliased).
    ///
    /// # Safety
    /// Each non-null pointer in `inputs` must be readable for `frames`.
    pub unsafe fn feed(&mut self, inputs: &[*mut f32], frames: usize) {
        if self.dry.is_empty() || self.ring_len == 0 {
            return;
        }
        for (ci, ring) in self.dry.iter_mut().enumerate() {
            let Some(&src) = inputs.get(ci) else { break };
            if src.is_null() {
                continue;
            }
            let mut w = self.write;
            for f in 0..frames.min(self.ring_len) {
                // SAFETY: caller contract.
                ring[w] = unsafe { *src.add(f) };
                w = (w + 1) % self.ring_len;
            }
        }
        // Single write head for all channels; advance once.
        self.write = (self.write + frames.min(self.ring_len)) % self.ring_len;
    }

    /// Audio thread, AFTER the plugin runs: crossfade the plugin's
    /// output with the latency-delayed dry signal according to the
    /// bypass state. Fully-active blocks cost one branch.
    ///
    /// # Safety
    /// Each non-null pointer in `outputs` must be writable for `frames`.
    pub unsafe fn mix(&mut self, bypassed: bool, outputs: &[*mut f32], frames: usize) {
        self.engaged = bypassed;
        // ring_len == 0 means unprepared — dry.is_empty() does NOT:
        // a GENERATOR (0-input instrument) has no dry channels, and
        // its bypassed output is SILENCE, not a no-op (the chime
        // consumer found bypass never engaging for any 0-input
        // plugin — the fifth instrument lock).
        if self.ring_len == 0 {
            return;
        }
        // Fast path: fully active, no fade in motion.
        if !bypassed && self.fade_pos == 0 {
            return;
        }
        let read_base =
            (self.write + self.ring_len - self.latency % self.ring_len - frames.min(self.ring_len))
                % self.ring_len;
        let mut fade = self.fade_pos;
        for f in 0..frames.min(self.ring_len) {
            // Advance the fade toward its target once per frame.
            if bypassed && fade < self.fade_len {
                fade += 1;
            } else if !bypassed && fade > 0 {
                fade -= 1;
            }
            let t = fade as f32 / self.fade_len as f32; // 0..1
                                                        // Equal-power: wet·cos(t·π/2) + dry·sin(t·π/2).
            let (wet_g, dry_g) = if t <= 0.0 {
                (1.0, 0.0)
            } else if t >= 1.0 {
                (0.0, 1.0)
            } else {
                let a = t * core::f32::consts::FRAC_PI_2;
                (a.cos(), a.sin())
            };
            let ri = (read_base + f) % self.ring_len;
            // Iterate OUTPUTS: a channel with no dry ring behind it
            // (generators; the upmixed tail of an out>in effect)
            // crossfades against silence.
            for (ci, &dst) in outputs.iter().enumerate() {
                if dst.is_null() {
                    continue;
                }
                let dry_s = self.dry.get(ci).map_or(0.0, |ring| ring[ri]);
                // SAFETY: caller contract.
                unsafe {
                    let wet = *dst.add(f);
                    *dst.add(f) = wet * wet_g + dry_s * dry_g;
                }
            }
        }
        self.fade_pos = fade;
    }
}

/// RAII flush-to-zero guard ("belt AND
/// braces"): engaged at wrapper process entry so any recursive state a
/// future cell forgets to flush manually can't become an x86/arm
/// denormal perf cliff. Manual flushes in stave-dsp remain the primary
/// mechanism — this is the backstop. Engage/restore is a single
/// control-register write (~ns per block).
pub struct ScopedFtz {
    #[cfg(target_arch = "aarch64")]
    saved: u64,
    #[cfg(target_arch = "x86_64")]
    saved: u32,
}

impl ScopedFtz {
    #[inline]
    pub fn engage() -> Self {
        // Miri interprets floats deterministically and cannot execute
        // the FP-control-register asm; FTZ is hardware state with no
        // meaning there. No-op under the interpreter so every wrapper
        // process path stays Miri-checkable.
        // SAFETY: reads FPCR and sets only its FZ bit — no memory, no
        // stack; `Drop` restores the saved value on this thread.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        unsafe {
            let saved: u64;
            core::arch::asm!("mrs {}, fpcr", out(reg) saved, options(nomem, nostack));
            // FZ (bit 24): flush denormal outputs/inputs to zero.
            core::arch::asm!("msr fpcr, {}", in(reg) saved | (1 << 24), options(nomem, nostack));
            Self { saved }
        }
        #[cfg(all(target_arch = "aarch64", miri))]
        {
            Self { saved: 0 }
        }
        // SAFETY: sets only MXCSR's FZ and DAZ bits; `Drop` restores the
        // saved value on this thread.
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        unsafe {
            let saved = core::arch::x86_64::_mm_getcsr();
            // FZ (bit 15) + DAZ (bit 6).
            core::arch::x86_64::_mm_setcsr(saved | 0x8040);
            Self { saved }
        }
        #[cfg(all(target_arch = "x86_64", miri))]
        {
            Self { saved: 0 }
        }
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        Self {}
    }
}

impl Drop for ScopedFtz {
    fn drop(&mut self) {
        // SAFETY: restores the control register `engage` read.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        unsafe {
            core::arch::asm!("msr fpcr, {}", in(reg) self.saved, options(nomem, nostack));
        }
        // SAFETY: as above.
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        unsafe {
            core::arch::x86_64::_mm_setcsr(self.saved);
        }
    }
}

/// A GUI-side parameter gesture event, en route to the host.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GestureEvent {
    Begin { id: u32 },
    Value { id: u32, plain: f64 },
    End { id: u32 },
}

const GESTURE_CAP: usize = 64;

/// Bounded SPSC ring for UI→audio gesture delivery (CLAP emits param
/// changes through `out_events` on the audio thread; VST3 calls its
/// handler directly from the UI thread and doesn't need this).
///
/// Producer: UI thread (`push`). Consumer: audio thread (`pop`).
/// Overflow drops the NEWEST event and counts it — never silent;
/// a 64-deep ring only overflows if audio is wedged.
pub struct GestureQueue {
    kinds: [core::sync::atomic::AtomicU8; GESTURE_CAP],
    ids: [AtomicU32; GESTURE_CAP],
    values: [core::sync::atomic::AtomicU64; GESTURE_CAP],
    head: AtomicU32, // consumer position
    tail: AtomicU32, // producer position
    dropped: AtomicU32,
}

impl Default for GestureQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureQueue {
    pub fn new() -> Self {
        Self {
            kinds: [const { core::sync::atomic::AtomicU8::new(0) }; GESTURE_CAP],
            ids: [const { AtomicU32::new(0) }; GESTURE_CAP],
            values: [const { core::sync::atomic::AtomicU64::new(0) }; GESTURE_CAP],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// UI-thread producer. Returns false (and counts) on overflow.
    pub fn push(&self, ev: GestureEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) as usize >= GESTURE_CAP {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = tail as usize % GESTURE_CAP;
        let (kind, id, bits) = match ev {
            GestureEvent::Begin { id } => (1u8, id, 0u64),
            GestureEvent::Value { id, plain } => (2, id, plain.to_bits()),
            GestureEvent::End { id } => (3, id, 0),
        };
        self.kinds[slot].store(kind, Ordering::Relaxed);
        self.ids[slot].store(id, Ordering::Relaxed);
        self.values[slot].store(bits, Ordering::Relaxed);
        // Publish AFTER the payload: the consumer gates on tail.
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Audio-thread consumer.
    pub fn pop(&self) -> Option<GestureEvent> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = head as usize % GESTURE_CAP;
        let kind = self.kinds[slot].load(Ordering::Relaxed);
        let id = self.ids[slot].load(Ordering::Relaxed);
        let bits = self.values[slot].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(match kind {
            1 => GestureEvent::Begin { id },
            3 => GestureEvent::End { id },
            _ => GestureEvent::Value {
                id,
                plain: f64::from_bits(bits),
            },
        })
    }

    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// True if any pointer range `[p, p+frames)` in `a` overlaps any in `b`.
/// Null pointers are skipped. Used to detect in-place processing, where
/// the input must be staged through scratch before building the
/// no-alias `AudioBuffer` pair.
pub fn ranges_alias(a: &[*mut f32], b: &[*mut f32], frames: usize) -> bool {
    if frames == 0 {
        return false;
    }
    for &pa in a {
        if pa.is_null() {
            continue;
        }
        let (a_lo, a_hi) = (
            pa as usize,
            pa as usize + frames * core::mem::size_of::<f32>(),
        );
        for &pb in b {
            if pb.is_null() {
                continue;
            }
            let (b_lo, b_hi) = (
                pb as usize,
                pb as usize + frames * core::mem::size_of::<f32>(),
            );
            if a_lo < b_hi && b_lo < a_hi {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginDescriptor;
    use crate::process::ProcessCtx;

    struct NullPlugin;
    impl Plugin for NullPlugin {
        const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
            id: c"test.null",
            name: c"Null",
            vendor: c"test",
            version: c"0.0.0",
            url: c"",
            description: c"",
            features: &[],
        };
        fn new() -> Self {
            NullPlugin
        }
        fn process(&mut self, _ctx: &mut ProcessCtx<'_>) {}
    }

    #[test]
    fn claim_is_exclusive_and_released_on_drop() {
        let cell = PluginCell::new(NullPlugin::new());
        let g = cell.claim().expect("first claim");
        assert!(
            cell.claimed.load(Ordering::Relaxed),
            "token held while guard lives"
        );
        drop(g);
        assert!(cell.claim().is_some(), "claim available again after drop");
    }

    #[test]
    fn ranges_alias_detects_identity_offset_and_disjoint() {
        let mut a = [0.0f32; 64];
        let mut b = [0.0f32; 64];
        let pa = a.as_mut_ptr();
        let pb = b.as_mut_ptr();
        assert!(ranges_alias(&[pa], &[pa], 64), "identical pointers alias");
        // Partial overlap: second range starts inside the first.
        // SAFETY: 32 is inside `a`'s 64 elements.
        let pa_off = unsafe { pa.add(32) };
        assert!(ranges_alias(&[pa], &[pa_off], 64), "offset ranges alias");
        assert!(!ranges_alias(&[pa], &[pb], 64), "separate buffers don't");
        assert!(!ranges_alias(&[pa], &[pa], 0), "zero frames never alias");
        assert!(
            !ranges_alias(&[core::ptr::null_mut()], &[pb], 64),
            "null skipped"
        );
    }

    /// SPSC contract under real threads (Miri checks the ordering
    /// claims): UI produces, audio consumes, order preserved, overflow
    /// counted.
    #[test]
    fn gesture_queue_is_ordered_and_counts_drops() {
        use std::sync::Arc;
        let q = Arc::new(GestureQueue::new());
        let producer = {
            let q = Arc::clone(&q);
            std::thread::spawn(move || {
                for i in 0..40u32 {
                    q.push(GestureEvent::Value {
                        id: i,
                        plain: f64::from(i),
                    });
                }
            })
        };
        let mut got = Vec::new();
        while got.len() < 40 {
            if let Some(GestureEvent::Value { id, plain }) = q.pop() {
                assert_eq!(plain, f64::from(id), "payload matches publication order");
                got.push(id);
            } else {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert!(got.windows(2).all(|w| w[0] < w[1]), "FIFO order");
        assert_eq!(q.dropped(), 0);

        // Overflow: 65th unconsumed push drops and counts.
        for i in 0..65u32 {
            q.push(GestureEvent::Begin { id: i });
        }
        assert_eq!(q.dropped(), 1, "overflow counted, never silent");
    }

    /// FTZ guard: subnormal products flush to zero while engaged, and
    /// the register is restored on drop.
    #[test]
    #[cfg_attr(miri, ignore = "inline asm is not interpretable")]
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    fn ftz_guard_flushes_denormals_and_restores() {
        use core::hint::black_box;
        let a = black_box(1e-20f32);
        let b = black_box(1e-20f32);
        // 1e-40 is subnormal (min normal ≈ 1.18e-38): nonzero without FZ.
        assert!(black_box(a) * black_box(b) != 0.0, "precondition: FZ off");
        {
            let _g = ScopedFtz::engage();
            assert_eq!(black_box(a) * black_box(b), 0.0, "flushed under guard");
        }
        assert!(black_box(a) * black_box(b) != 0.0, "register restored");
    }

    #[test]
    fn staging_copies_and_zeroes_null_channels() {
        let mut state = ProcessorState::new(NullPlugin::new());
        state.prepare_scratch(2, 8);
        let mut src = [1.0f32, 2.0, 3.0, 4.0];
        let ptrs = [src.as_mut_ptr(), core::ptr::null_mut()];
        // SAFETY: `src` holds the 4 frames staged.
        let (_, _, staged) = unsafe { state.split_for_block(Some((&ptrs, 4))) };
        let staged = staged.unwrap();
        assert_eq!(staged.len(), 2);
        // SAFETY: each staged pointer is a scratch buffer of ≥ 4 frames.
        let c0 = unsafe { core::slice::from_raw_parts(staged[0], 4) };
        // SAFETY: as above.
        let c1 = unsafe { core::slice::from_raw_parts(staged[1], 4) };
        assert_eq!(c0, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(c1, &[0.0; 4]);
        assert!(!ranges_alias(staged, &ptrs, 4), "staged pointers are fresh");
    }
}
