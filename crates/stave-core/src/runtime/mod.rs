//! The plugin runtime the format wrappers (VST3, CLAP) drive: the
//! erased plugin contract ([`Erased`]) and the machinery every wrapper
//! shares. Not author API — an author implements `stave::Plugin`, and
//! `stave::export!` adapts it to [`Erased`].
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
//!   Live and others do routinely and which the buffers' no-alias
//!   contract forbids passing through directly).
//! - [`ranges_alias`] — the overlap detector wrappers use to decide when
//!   the scratch copy is needed.
//! - [`WrapperTaps`] — the I/O peak and CPU taps every wrapper publishes,
//!   and [`peaks`], the block scan behind them.

mod erased;
mod ftz;
mod gesture;
mod metrics;
mod panic_guard;
#[cfg(kani)]
mod proofs;
pub mod state;
mod taps;

pub use erased::{tail_wire, Erased};
pub use ftz::ScopedFtz;
pub use gesture::{GestureEvent, GestureQueue};
pub use metrics::peaks;
pub use panic_guard::PanicGuard;
pub use taps::WrapperTaps;

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use std::boxed::Box;
use std::vec::Vec;



/// Cross-thread instance state. Every field is an atomic: interface
/// objects read and write through `&self` from any thread the host picks.
#[derive(Debug)]
pub struct SharedState {
    /// f32 bit pattern (atomics have no native f32 on all targets).
    sample_rate_bits: AtomicU32,
    max_frames: AtomicU32,
    activated: AtomicBool,
    /// Mirror of `Erased::latency`, refreshed on activate. Hosts
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

/// Most aux input ports a block carries (declared ports past this are
/// served as absent).
pub const MAX_AUX_PORTS: usize = 4;

/// The processor-side half of an instance: the plugin itself plus the
/// preallocated buffers the wrapper needs on the audio thread.
///
/// The lifecycle is a state: the plugin is processed only between
/// [`prepare`](Self::prepare) and [`release`](Self::release) —
/// [`split_for_block`](Self::split_for_block), the only way to its
/// audio-thread borrow, answers `None` otherwise.
pub struct ProcessorState<P: Erased> {
    pub plugin: P,
    /// Between `prepare` and `release`.
    prepared: bool,
    /// Framework-owned click-free bypass.
    pub bypass: BypassGate,
    /// Scratch channels, `scratch_frames` samples each, back to back:
    /// staged in-place inputs and stand-ins for null host channels.
    /// Sized at prepare; the audio thread never resizes it.
    scratch: Box<[f32]>,
    scratch_frames: usize,
    /// Pointer lists the block's buffers read when the host's lists
    /// cannot be used as-is. One slot per scratch channel.
    ptrs: Box<[*mut f32]>,
}

/// One block's channel pointer lists, ready for
/// [`InputBuffer::from_raw`](crate::InputBuffer::from_raw) and
/// [`OutputBuffer::from_raw`](crate::OutputBuffer::from_raw): no nulls, and
/// staged inputs never alias the outputs.
pub struct BlockIo<'a> {
    pub input: &'a [*mut f32],
    pub output: &'a [*mut f32],
    pub aux: [&'a [*mut f32]; MAX_AUX_PORTS],
    /// The frames the plugin may run: the host's count, or the prepared
    /// scratch length when the block needed scratch and was longer.
    pub frames: usize,
    /// Frames or channels were cut to fit the prepared scratch: record
    /// [`ProcessFault::BlockTruncated`](crate::diagnostics::ProcessFault::BlockTruncated).
    pub truncated: bool,
}

/// Every audio-thread borrow a wrapper needs for one block.
pub struct BlockBorrows<'a, P> {
    pub plugin: &'a mut P,
    pub bypass: &'a mut BypassGate,
    pub io: BlockIo<'a>,
}

/// Scratch channels for the widest declared configuration: main in +
/// main out + every aux input.
fn declared_channels<P: Erased>() -> usize {
    P::LAYOUTS
        .iter()
        .map(|c| {
            c.input_channels()
                + c.output_channels()
                + c.aux
                    .iter()
                    .take(MAX_AUX_PORTS)
                    .map(|a| a.channels.count())
                    .sum::<usize>()
        })
        .max()
        .unwrap_or(0)
}

impl<P: Erased> ProcessorState<P> {
    pub fn new(plugin: P) -> Self {
        Self {
            plugin,
            prepared: false,
            bypass: BypassGate::new(),
            scratch: Box::default(),
            scratch_frames: 0,
            ptrs: Box::default(),
        }
    }

    /// Size the scratch for `max_frames`: every channel of the widest
    /// declared configuration, and at least `in_channels`. Call from a
    /// non-realtime context (activate / setupProcessing) — this is the
    /// only allocation site.
    pub fn prepare_scratch(&mut self, in_channels: usize, max_frames: usize) {
        let channels = declared_channels::<P>().max(in_channels);
        if self.ptrs.len() != channels || self.scratch_frames != max_frames {
            self.scratch = std::vec![0.0; channels * max_frames].into_boxed_slice();
            self.ptrs = std::vec![core::ptr::null_mut(); channels].into_boxed_slice();
            self.scratch_frames = max_frames;
        }
    }

    /// Prepare the plugin for `env` (and reset it: a fresh start must not
    /// ring in stale state), size the scratch and the bypass gate's dry
    /// path, and allow processing. Non-realtime. Returns the plugin's
    /// latency at these conditions.
    pub fn prepare(&mut self, env: &crate::Environment, in_channels: usize) -> u32 {
        let max_frames = env.max_block.max(1);
        self.plugin.prepare(env);
        self.plugin.reset();
        self.prepare_scratch(in_channels, max_frames);
        let latency = self.plugin.latency();
        self.bypass
            .prepare(in_channels, latency as usize, max_frames, env.sample_rate);
        self.prepared = true;
        latency
    }

    /// No more processing until the next [`prepare`](Self::prepare)
    /// (the host deactivated the plugin).
    pub fn release(&mut self) {
        self.prepared = false;
    }

    /// Between [`prepare`](Self::prepare) and [`release`](Self::release).
    pub fn is_prepared(&self) -> bool {
        self.prepared
    }

    /// Split every audio-thread borrow the wrapper needs at once: the
    /// plugin, the bypass gate, and the block's channel lists — the
    /// host's own when they can be used as-is, otherwise lists through
    /// preallocated scratch: inputs staged when they alias the outputs
    /// (in-place hosts), zeroed stand-ins for null channels.
    ///
    /// Never allocates. A block that needs scratch beyond what was
    /// prepared (more frames, or more channels than declared) is cut to
    /// fit — the plugin runs the frames that fit, surplus channels are
    /// left out (surplus outputs zeroed) — and `io.truncated` is set.
    /// The caller zeroes outputs past `io.frames`
    /// ([`zero_tail`]). `None` while the plugin is not prepared: the
    /// caller silences the block and records
    /// [`ProcessFault::Unprepared`](crate::diagnostics::ProcessFault::Unprepared).
    ///
    /// # Safety
    /// Each non-null pointer in `input`, `output` and `aux` is valid for
    /// `frames` f32 reads (inputs) or writes (outputs) for `'a`, and no
    /// two output channels overlap.
    pub unsafe fn split_for_block<'a>(
        &'a mut self,
        input: &'a [*mut f32],
        output: &'a [*mut f32],
        aux: &[&'a [*mut f32]],
        frames: usize,
    ) -> Option<BlockBorrows<'a, P>> {
        if !self.prepared {
            return None;
        }
        let stage = ranges_alias(input, output, frames);
        let aux = &aux[..aux.len().min(MAX_AUX_PORTS)];
        let has_null = |ptrs: &[*mut f32]| ptrs.iter().any(|p| p.is_null());
        let needs_scratch =
            stage || has_null(input) || has_null(output) || aux.iter().any(|a| has_null(a));
        let mut truncated = false;
        let run = if needs_scratch && frames > self.scratch_frames {
            truncated = true;
            self.scratch_frames
        } else {
            frames
        };

        let mut cursor = Cursor {
            // One reborrow of the scratch per block: every channel
            // pointer below derives from this base.
            base: self.scratch.as_mut_ptr(),
            stride: self.scratch_frames,
            ptrs: &mut self.ptrs,
            next: 0,
            run,
            truncated,
        };
        // SAFETY: this function's contract: non-null inputs are readable
        // for `frames >= run`.
        let input_at = unsafe { cursor.list(input, stage, false) };
        // SAFETY: as above; non-null outputs are writable too.
        let output_at = unsafe { cursor.list(output, false, true) };
        let mut aux_at = [ListAt::Host; MAX_AUX_PORTS];
        for (at, &list) in aux_at.iter_mut().zip(aux) {
            // SAFETY: as for the inputs.
            *at = unsafe { cursor.list(list, false, false) };
        }
        let truncated = cursor.truncated;

        let ptrs: &'a [*mut f32] = &self.ptrs;
        let resolve = |at: ListAt, host: &'a [*mut f32]| match at {
            ListAt::Host => host,
            ListAt::Scratch(from, to) => &ptrs[from..to],
        };
        let mut aux_lists: [&'a [*mut f32]; MAX_AUX_PORTS] = [&[]; MAX_AUX_PORTS];
        for (i, &list) in aux.iter().enumerate() {
            aux_lists[i] = resolve(aux_at[i], list);
        }
        Some(BlockBorrows {
            plugin: &mut self.plugin,
            bypass: &mut self.bypass,
            io: BlockIo {
                input: resolve(input_at, input),
                output: resolve(output_at, output),
                aux: aux_lists,
                frames: run,
                truncated,
            },
        })
    }
}

/// Where a block's channel list lives: the host's own, or a range of
/// [`ProcessorState`]'s pointer slots.
#[derive(Copy, Clone)]
enum ListAt {
    Host,
    Scratch(usize, usize),
}

/// Hands out scratch channels and pointer slots for one block.
struct Cursor<'s> {
    base: *mut f32,
    stride: usize,
    ptrs: &'s mut [*mut f32],
    next: usize,
    run: usize,
    truncated: bool,
}

impl Cursor<'_> {
    /// `host` as a usable list: itself when it has no nulls and need not
    /// be staged; otherwise a scratch-backed copy where null channels (and
    /// every channel, when `stage`) read scratch holding zeros (or the
    /// staged input). Channels past the scratch are left out — zeroed
    /// first when `zero_dropped` (outputs the plugin will not write).
    ///
    /// # Safety
    /// Each non-null pointer in `host` is valid for `run` f32 reads, and
    /// writes too when `zero_dropped`.
    unsafe fn list(&mut self, host: &[*mut f32], stage: bool, zero_dropped: bool) -> ListAt {
        if !stage && !host.iter().any(|p| p.is_null()) {
            return ListAt::Host;
        }
        let from = self.next;
        for &src in host {
            if self.next == self.ptrs.len() {
                self.truncated = true;
                if zero_dropped && !src.is_null() {
                    // SAFETY: a non-null output is writable for `run`.
                    unsafe { core::slice::from_raw_parts_mut(src, self.run) }.fill(0.0);
                }
                continue;
            }
            if !src.is_null() && !stage {
                self.ptrs[self.next] = src;
                self.next += 1;
                continue;
            }
            // SAFETY: slot `next` < ptrs.len() = the scratch's channel
            // count, each `stride` long: the offset stays inside the
            // allocation `base` points into.
            let dst = unsafe { self.base.add(self.next * self.stride) };
            // SAFETY: `dst` is `run <= stride` samples of our own scratch,
            // reached only through `base` this block.
            let chan = unsafe { core::slice::from_raw_parts_mut(dst, self.run) };
            if src.is_null() {
                chan.fill(0.0);
            } else {
                // SAFETY: `src` is readable for `run` (the contract), and
                // the scratch is our allocation, so the two cannot overlap.
                unsafe { core::ptr::copy_nonoverlapping(src, dst, self.run) };
            }
            self.ptrs[self.next] = dst;
            self.next += 1;
        }
        ListAt::Scratch(from, self.next)
    }
}

/// Zero `outputs` from frame `from` to `frames`: what a truncated block
/// plays past the frames the plugin ran.
///
/// # Safety
/// Each non-null pointer in `outputs` is writable for `frames`.
pub unsafe fn zero_tail(outputs: &[*mut f32], from: usize, frames: usize) {
    if from >= frames {
        return;
    }
    for &out in outputs.iter().filter(|p| !p.is_null()) {
        // SAFETY: writable for `frames` (the contract); `from < frames`.
        unsafe { core::slice::from_raw_parts_mut(out.add(from), frames - from) }.fill(0.0);
    }
}

/// Order a block's parameter changes by frame, arrival order breaking
/// ties — the (frame, sequence) order hosts mean. An in-place insertion
/// sort: hosts deliver nearly sorted points, so this is near linear, and
/// unlike the standard library's stable sort it never allocates.
pub fn sort_param_changes(changes: &mut [crate::process::ParamChange]) {
    for i in 1..changes.len() {
        let mut j = i;
        // Strictly greater: equal frames keep arrival order.
        while j > 0 && changes[j - 1].frame > changes[j].frame {
            changes.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Single-owner cell for [`ProcessorState`]. See module docs for the
/// protocol; `claim` is the only way in.
pub struct PluginCell<P: Erased> {
    state: UnsafeCell<ProcessorState<P>>,
    claimed: AtomicBool,
}

// SAFETY: access to `state` is serialized by the `claimed` token —
// `claim` hands out at most one `ProcessorGuard` at a time, and the
// Acquire/Release pair on the token orders one owner's writes before the
// next owner's reads.
unsafe impl<P: Erased> Sync for PluginCell<P> {}
// SAFETY: `P: Send`; the only non-`Send` field is `ptrs`, whose pointers
// are rewritten every block before use (host channels for that call, or
// `scratch`, owned by the same state and moved with it) and never read
// across a move.
unsafe impl<P: Erased> Send for PluginCell<P> {}

impl<P: Erased> PluginCell<P> {
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
pub struct ProcessorGuard<'a, P: Erased> {
    cell: &'a PluginCell<P>,
}

impl<P: Erased> Deref for ProcessorGuard<'_, P> {
    type Target = ProcessorState<P>;
    fn deref(&self) -> &ProcessorState<P> {
        // SAFETY: the claim token guarantees this guard is the sole
        // accessor for its lifetime.
        unsafe { &*self.cell.state.get() }
    }
}

impl<P: Erased> DerefMut for ProcessorGuard<'_, P> {
    fn deref_mut(&mut self) -> &mut ProcessorState<P> {
        // SAFETY: as above.
        unsafe { &mut *self.cell.state.get() }
    }
}

impl<P: Erased> Drop for ProcessorGuard<'_, P> {
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
        sample_rate: f32,
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
        // its bypassed output is SILENCE, not a no-op.
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

/// True if any pointer range `[p, p+frames)` in `a` overlaps any in `b`.
/// Null pointers are skipped. Used to detect in-place processing, where
/// the input must be staged through scratch before building the
/// no-alias input/output buffer pair.
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
    use crate::params::ParamId;
    use crate::info::{Info, Layout};
    use crate::params::Params;
    use crate::process::Block;
    use std::sync::Arc;
    use stave_telemetry::Telemetry;

    struct NullPlugin;
    impl Erased for NullPlugin {
        const INFO: Info = Info {
            id: "test.null",
            name: "Null",
            vendor: "test",
            ..Info::EFFECT
        };
        const LAYOUTS: &'static [Layout] = &[Layout::STEREO];
        fn new(_telemetry: &Telemetry) -> Self {
            NullPlugin
        }
        fn params(&self) -> Arc<dyn Params> {
            Arc::new(crate::params::NoParams)
        }
        fn process(&mut self, _block: &mut Block<'_>) {}
    }

    #[test]
    fn claim_is_exclusive_and_released_on_drop() {
        let cell = PluginCell::new(NullPlugin::new(&Telemetry::new()));
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
                        id: ParamId(i),
                        value: f64::from(i),
                    });
                }
            })
        };
        let mut got = Vec::new();
        while got.len() < 40 {
            if let Some(GestureEvent::Value { id, value }) = q.pop() {
                assert_eq!(value, f64::from(id.0), "payload matches publication order");
                got.push(id.0);
            } else {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert!(got.windows(2).all(|w| w[0] < w[1]), "FIFO order");
        assert_eq!(q.dropped(), 0);

        // Overflow: the 65th unconsumed Value drops and counts.
        for i in 0..65u32 {
            q.push(GestureEvent::Value {
                id: ParamId(i),
                value: 0.0,
            });
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

    /// A stereo effect with one sidechain: 2 in + 2 out + 2 aux = 6
    /// scratch channels.
    struct SideFx;
    impl Erased for SideFx {
        const INFO: Info = NullPlugin::INFO;
        const LAYOUTS: &'static [Layout] = &[Layout {
            aux: &[crate::info::Aux {
                name: "side",
                channels: crate::info::Channels::Stereo,
            }],
            ..Layout::STEREO
        }];
        fn new(_telemetry: &Telemetry) -> Self {
            SideFx
        }
        fn params(&self) -> Arc<dyn Params> {
            Arc::new(crate::params::NoParams)
        }
        fn process(&mut self, _block: &mut Block<'_>) {}
    }

    fn channel<'a>(ptr: *mut f32, frames: usize) -> &'a [f32] {
        assert!(!ptr.is_null(), "a null channel reached the buffers");
        // SAFETY: every pointer these tests read is a test array or
        // scratch of at least `frames` samples.
        unsafe { core::slice::from_raw_parts(ptr, frames) }
    }

    /// The lifecycle as a state: no audio-thread borrow before `prepare`
    /// or after `release`.
    #[test]
    fn only_a_prepared_plugin_is_processed() {
        let mut state = ProcessorState::new(SideFx);
        let mut out = [0.0f32; 4];
        let outs = [out.as_mut_ptr()];
        // SAFETY: `out` holds the 4 frames.
        assert!(unsafe { state.split_for_block(&[], &outs, &[], 4) }.is_none(), "unprepared");
        state.prepare(&crate::Environment::new(48_000.0, 8), 2);
        assert!(state.is_prepared());
        // SAFETY: as above.
        assert!(unsafe { state.split_for_block(&[], &outs, &[], 4) }.is_some(), "prepared");
        state.release();
        // SAFETY: as above.
        assert!(unsafe { state.split_for_block(&[], &outs, &[], 4) }.is_none(), "released");
    }

    #[test]
    fn staging_copies_and_zeroes_null_channels() {
        let mut state = ProcessorState::new(SideFx);
        state.prepare(&crate::Environment::new(48_000.0, 8), 2);
        let mut buf = [1.0f32, 2.0, 3.0, 4.0];
        let mut out_r = [9.0f32; 4];
        // One raw pointer to `buf`, shared by the input and the output, the
        // way a host passes an in-place buffer.
        let shared = buf.as_mut_ptr();
        let ins = [shared, core::ptr::null_mut()];
        let outs = [shared, out_r.as_mut_ptr()];
        // SAFETY: `buf` and `out_r` hold the 4 frames.
        let block = unsafe { state.split_for_block(&ins, &outs, &[], 4) }.expect("prepared");
        let io = block.io;
        assert!(!io.truncated);
        assert_eq!(io.input.len(), 2);
        assert_eq!(channel(io.input[0], 4), &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(channel(io.input[1], 4), &[0.0; 4]);
        assert!(
            !ranges_alias(io.input, &outs, 4),
            "staged pointers are fresh"
        );
        assert_eq!(io.output, &outs, "outputs without nulls pass through");
    }

    /// Null channels in any list become zeroed scratch, each its own
    /// (outputs must not alias one another).
    #[test]
    fn null_channels_become_distinct_zeroed_scratch() {
        let mut state = ProcessorState::new(SideFx);
        state.prepare(&crate::Environment::new(48_000.0, 8), 2);
        let mut a = [1.0f32; 8];
        let mut b = [2.0f32; 8];
        let null = core::ptr::null_mut();
        let ins = [a.as_mut_ptr(), null];
        let outs = [null, null];
        let side = [null, b.as_mut_ptr()];
        // SAFETY: `a` and `b` hold the 8 frames.
        let block = unsafe { state.split_for_block(&ins, &outs, &[&side], 8) }.expect("prepared");
        let io = block.io;
        assert!(!io.truncated);
        assert_eq!(io.input[0], ins[0], "a real channel is not copied");
        assert_eq!(channel(io.input[1], 8), &[0.0; 8]);
        assert_eq!(channel(io.output[0], 8), &[0.0; 8]);
        assert_eq!(channel(io.output[1], 8), &[0.0; 8]);
        assert_ne!(io.output[0], io.output[1]);
        assert!(!ranges_alias(&io.output[..1], &io.output[1..], 8));
        assert_eq!(channel(io.aux[0][0], 8), &[0.0; 8]);
        assert_eq!(io.aux[0][1], side[1]);
    }

    /// A block longer than the prepared scratch, or wider than declared,
    /// is cut to fit — never grown — and says so.
    #[test]
    fn a_block_past_the_prepared_scratch_is_truncated_not_grown() {
        let mut state = ProcessorState::new(SideFx);
        state.prepare(&crate::Environment::new(48_000.0, 4), 2);
        let (scratch, slots) = (state.scratch.as_ptr(), state.ptrs.len());
        let mut buf = [5.0f32; 16];
        let null = core::ptr::null_mut();

        // Too long: in place (needs staging) with 16 > 4 frames.
        let ins = [buf.as_mut_ptr()];
        // SAFETY: `buf` holds 16 frames.
        let block = unsafe { state.split_for_block(&ins, &ins, &[], 16) }.expect("prepared");
        assert!(block.io.truncated);
        assert_eq!(block.io.frames, 4);
        assert_eq!(channel(block.io.input[0], 4), &[5.0; 4]);

        // Too wide: 8 null outputs against 6 scratch channels; the real
        // output past the scratch is zeroed and left out.
        let mut tail = [7.0f32; 4];
        let mut outs = [null; 9];
        outs[8] = tail.as_mut_ptr();
        // SAFETY: `tail` holds 4 frames.
        let block = unsafe { state.split_for_block(&[], &outs, &[], 4) }.expect("prepared");
        assert!(block.io.truncated);
        assert_eq!(block.io.output.len(), 6);
        assert_eq!(tail, [0.0; 4]);

        // A long block that needs no scratch runs whole.
        let mut out = [0.0f32; 16];
        let outs = [out.as_mut_ptr()];
        // SAFETY: `buf`, `out` hold 16 frames and do not overlap.
        let block = unsafe { state.split_for_block(&ins, &outs, &[], 16) }.expect("prepared");
        assert!(!block.io.truncated);
        assert_eq!(block.io.frames, 16);

        assert_eq!(state.scratch.as_ptr(), scratch, "scratch never reallocated");
        assert_eq!(state.ptrs.len(), slots);
    }

    #[test]
    fn zero_tail_clears_past_the_frames_run() {
        let mut out = [1.0f32; 6];
        let outs = [out.as_mut_ptr(), core::ptr::null_mut()];
        // SAFETY: `out` holds 6 frames.
        unsafe { zero_tail(&outs, 4, 6) };
        assert_eq!(out, [1.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
    }

    /// (frame, sequence) order: by frame, arrival order breaking ties.
    #[test]
    fn param_changes_sort_by_frame_then_arrival() {
        use crate::process::{ParamChange, ParamChangeKind};
        let change = |offset: usize, seq: u32| ParamChange {
            id: crate::ParamId(seq),
            frame: offset,
            value: 0.0,
            kind: ParamChangeKind::Value,
        };
        let mut changes = [
            change(5, 0),
            change(0, 1),
            change(5, 2),
            change(3, 3),
            change(0, 4),
            change(5, 5),
        ];
        sort_param_changes(&mut changes);
        let order: Vec<(usize, u32)> = changes.iter().map(|c| (c.frame, c.id.0)).collect();
        assert_eq!(order, [(0, 1), (0, 4), (3, 3), (5, 0), (5, 2), (5, 5)]);
    }
}
