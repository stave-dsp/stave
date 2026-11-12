//! Safe views of what a CLAP host hands the plugin. Each view has one
//! `unsafe` constructor carrying clap.h's contract — a pointer the host
//! passes is null or valid, function pointers included, for the call that
//! passed it (the host itself: for the plugin's lifetime) — and safe
//! methods after it. A view keeps the host's own pointer to pass back to
//! the host's functions: a C host may recover its enclosing object from
//! it, which a pointer derived from a Rust reference to the struct alone
//! would not allow.

use super::*;
use stave_core::diagnostics::Severity;
use stave_core::ParamChangeKind;

/// The `clap_host` that created the instance. Calls pass the host's own
/// pointer back (a C host may recover its object from it), never one
/// derived from a Rust reference.
#[derive(Copy, Clone)]
pub(super) struct Host(*const sys::ClapHost);

// SAFETY: the host's `get_extension` and `request_callback` are
// [thread-safe] (clap.h); `Host` only reads the struct.
unsafe impl Send for Host {}
// SAFETY: as above.
unsafe impl Sync for Host {}

impl Host {
    /// # Safety
    /// `host` is null or the `clap_host` passed to `create_plugin`, which
    /// outlives every instance it creates.
    pub(super) unsafe fn new(host: *const sys::ClapHost) -> Option<Self> {
        (!host.is_null()).then_some(Self(host))
    }

    fn get(self) -> &'static sys::ClapHost {
        // SAFETY: non-null and valid for the plugin's lifetime (`new`).
        unsafe { &*self.0 }
    }

    /// Asks for `on_main_thread`. [thread-safe]
    pub(super) fn request_callback(self) {
        if let Some(request_callback) = self.get().request_callback {
            // SAFETY: the host's own function, called with the host.
            unsafe { request_callback(self.0) };
        }
    }

    /// Tell the host every parameter value may have changed (after a
    /// state load), so it re-reads them. [main-thread] (clap ext/params.h)
    pub(super) fn rescan_param_values(self) {
        let Some(get_extension) = self.get().get_extension else {
            return;
        };
        // SAFETY: the host's own function, called with the host.
        let ext = unsafe { get_extension(self.0, sys::CLAP_EXT_PARAMS.as_ptr()) };
        // SAFETY: for `clap.params` a host returns null or a
        // `clap_host_params` that lives as long as the host.
        let Some(rescan) = unsafe { ext.cast::<sys::ClapHostParams>().as_ref() }.and_then(|p| p.rescan) else {
            return;
        };
        // SAFETY: the host's own function, on the main thread (state.load
        // is [main-thread]).
        unsafe { rescan(self.0, sys::CLAP_PARAM_RESCAN_VALUES) };
    }

    /// Ask the host to flush parameter events (`params.flush`) when it
    /// is not processing. [thread-safe, !audio-thread] (clap ext/params.h)
    pub(super) fn request_flush(self) {
        let Some(get_extension) = self.get().get_extension else {
            return;
        };
        // SAFETY: the host's own function, called with the host.
        let ext = unsafe { get_extension(self.0, sys::CLAP_EXT_PARAMS.as_ptr()) };
        // SAFETY: for `clap.params` a host returns null or a
        // `clap_host_params` that lives as long as the host.
        let Some(flush) = unsafe { ext.cast::<sys::ClapHostParams>().as_ref() }.and_then(|p| p.request_flush) else {
            return;
        };
        // SAFETY: the host's own function; thread-safe (not called from
        // the audio thread: the face runs on the main thread).
        unsafe { flush(self.0) };
    }

    /// The host's extension `id`, as a `T`.
    ///
    /// # Safety
    /// `T` is the struct clap.h declares for `id`.
    #[cfg(all(unix, not(target_os = "macos")))]
    unsafe fn extension<T>(self, id: &CStr) -> Option<&'static T> {
        let get_extension = self.get().get_extension?;
        // SAFETY: the host's own function, called with the host.
        let ext = unsafe { get_extension(self.0, id.as_ptr()) };
        // SAFETY: a host returns null or the extension's struct, which
        // lives as long as the host (this function's contract for `T`).
        unsafe { ext.cast::<T>().as_ref() }
    }

    /// A repaint timer every `period_ms` (`clap.timer-support`). [main-thread]
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn register_timer(self, period_ms: u32) -> Option<sys::ClapId> {
        // SAFETY: `clap.timer-support` is a `clap_host_timer_support`.
        let register = unsafe { self.extension::<sys::ClapHostTimerSupport>(sys::CLAP_EXT_TIMER_SUPPORT) }?.register_timer?;
        let mut id = sys::CLAP_INVALID_ID;
        // SAFETY: the host's own function, on the main thread; `id` is ours.
        let ok = unsafe { register(self.0, period_ms, &mut id) };
        (ok && id != sys::CLAP_INVALID_ID).then_some(id)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn unregister_timer(self, id: sys::ClapId) {
        // SAFETY: as in `register_timer`.
        let ext = unsafe { self.extension::<sys::ClapHostTimerSupport>(sys::CLAP_EXT_TIMER_SUPPORT) };
        if let Some(unregister) = ext.and_then(|e| e.unregister_timer) {
            // SAFETY: the host's own function, on the main thread.
            unsafe { unregister(self.0, id) };
        }
    }

    /// Watch `fd` (`clap.posix-fd-support`). [main-thread]
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn register_fd(self, fd: core::ffi::c_int, flags: u32) -> bool {
        // SAFETY: `clap.posix-fd-support` is a `clap_host_posix_fd_support`.
        let ext = unsafe { self.extension::<sys::ClapHostPosixFdSupport>(sys::CLAP_EXT_POSIX_FD_SUPPORT) };
        let Some(register) = ext.and_then(|e| e.register_fd) else { return false };
        // SAFETY: the host's own function, on the main thread.
        unsafe { register(self.0, fd, flags) }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn unregister_fd(self, fd: core::ffi::c_int) {
        // SAFETY: as in `register_fd`.
        let ext = unsafe { self.extension::<sys::ClapHostPosixFdSupport>(sys::CLAP_EXT_POSIX_FD_SUPPORT) };
        if let Some(unregister) = ext.and_then(|e| e.unregister_fd) {
            // SAFETY: the host's own function, on the main thread.
            unsafe { unregister(self.0, fd) };
        }
    }

    /// The host's `clap.log`. Only from `init` on (clap.h).
    pub(super) fn log(self) -> Option<HostLog> {
        let get_extension = self.get().get_extension?;
        // SAFETY: the host's own function, called with the host.
        let ext = unsafe { get_extension(self.0, sys::CLAP_EXT_LOG.as_ptr()) };
        // SAFETY: for `clap.log` a host returns null or a
        // `clap_host_log` that lives as long as the host.
        let log = unsafe { ext.cast::<sys::ClapHostLog>().as_ref() }?.log?;
        Some(HostLog { host: self, log })
    }
}

/// The host's `clap.log` as a log sink.
pub(super) struct HostLog {
    host: Host,
    log: unsafe extern "C" fn(*const sys::ClapHost, sys::ClapLogSeverity, *const c_char),
}

impl stave_core::diagnostics::LogSink for HostLog {
    fn write(&self, severity: Severity, message: &dyn core::fmt::Display) {
        let Ok(text) = std::ffi::CString::new(message.to_string().replace('\0', " ")) else {
            return;
        };
        // SAFETY: `clap_host_log.log` is [thread-safe]; `text` outlives
        // the call.
        unsafe { (self.log)(self.host.0, severity as sys::ClapLogSeverity, text.as_ptr()) };
    }
}

/// One `process` call's `clap_process`.
pub(super) struct Process<'a>(&'a sys::ClapProcess);

impl<'a> Process<'a> {
    /// # Safety
    /// `process` is null or the `clap_process` of the current call.
    pub(super) unsafe fn new(process: *const sys::ClapProcess) -> Option<Self> {
        // SAFETY: this function's contract.
        unsafe { process.as_ref() }.map(Self)
    }

    pub(super) fn frames(&self) -> usize {
        self.0.frames_count as usize
    }

    /// Input port `port`'s channel pointers; `None` when the host gave
    /// no such port or no 32-bit buffers for it.
    pub(super) fn input(&self, port: usize) -> Option<&'a [*mut f32]> {
        if port >= self.0.audio_inputs_count as usize || self.0.audio_inputs.is_null() {
            return None;
        }
        // SAFETY: `audio_inputs` holds `audio_inputs_count` buffers.
        channels(unsafe { &*self.0.audio_inputs.add(port) })
    }

    pub(super) fn output(&self) -> Option<&'a [*mut f32]> {
        if self.0.audio_outputs_count == 0 {
            return None;
        }
        // SAFETY: `audio_outputs` is null or holds
        // `audio_outputs_count` buffers.
        channels(unsafe { self.0.audio_outputs.as_ref() }?)
    }

    /// Zero every output channel: what a disabled instance plays.
    pub(super) fn silence_output(&self) {
        let frames = self.frames();
        for &channel in self.output().unwrap_or(&[]) {
            if !channel.is_null() {
                // SAFETY: an output channel is writable for `frames`
                // (clap.h).
                unsafe { core::slice::from_raw_parts_mut(channel, frames) }.fill(0.0);
            }
        }
    }

    /// Flag every channel of the main output constant (silent) or not:
    /// clap.h has the plugin set an output's `constant_mask` each block,
    /// so a block that is not silent clears it.
    pub(super) fn mark_output_constant(&self, constant: bool) {
        if self.0.audio_outputs_count == 0 {
            return;
        }
        // SAFETY: clap.h hands the plugin the outputs to write for the
        // call; `audio_outputs` is null or holds `audio_outputs_count`
        // buffers, and nothing else borrows the first one here.
        if let Some(bus) = unsafe { self.0.audio_outputs.as_mut() } {
            bus.constant_mask = match constant {
                true => (1u64 << bus.channel_count.min(63)).wrapping_sub(1),
                false => 0,
            };
        }
    }

    pub(super) fn transport(&self) -> Option<&'a sys::ClapEventTransport> {
        // SAFETY: null or valid for the call.
        unsafe { self.0.transport.as_ref() }
    }

    pub(super) fn in_events(&self) -> Option<InputEvents<'a>> {
        // SAFETY: null or valid for the call.
        unsafe { InputEvents::new(self.0.in_events) }
    }

    pub(super) fn out_events(&self) -> Option<OutputEvents<'a>> {
        // SAFETY: null or valid for the call.
        unsafe { OutputEvents::new(self.0.out_events) }
    }
}

fn channels(buffer: &sys::ClapAudioBuffer) -> Option<&[*mut f32]> {
    if buffer.data32.is_null() {
        return None;
    }
    // SAFETY: a non-null `data32` holds `channel_count` channel pointers.
    Some(unsafe {
        core::slice::from_raw_parts(buffer.data32.cast_const(), buffer.channel_count as usize)
    })
}

/// The core-space events the wrapper reads.
pub(super) enum CoreEvent<'a> {
    /// `clap_event_param_mod` has `clap_event_param_value`'s layout, its
    /// `amount` where `value` is.
    Param(ParamChangeKind, &'a sys::ClapEventParamValue),
    Note(NoteKind, &'a sys::ClapEventNote),
}

pub(super) struct InputEvents<'a> {
    list: *const sys::ClapInputEvents,
    lifetime: core::marker::PhantomData<&'a sys::ClapInputEvents>,
    size: unsafe extern "C" fn(*const sys::ClapInputEvents) -> u32,
    get: unsafe extern "C" fn(*const sys::ClapInputEvents, u32) -> *const sys::ClapEventHeader,
}

impl<'a> InputEvents<'a> {
    /// # Safety
    /// `list` is null or a `clap_input_events` valid for the current call.
    pub(super) unsafe fn new(list: *const sys::ClapInputEvents) -> Option<Self> {
        // SAFETY: this function's contract.
        let fns = unsafe { list.as_ref() }?;
        Some(Self {
            list,
            lifetime: core::marker::PhantomData,
            size: fns.size?,
            get: fns.get?,
        })
    }

    /// In the host's order (sorted by time, clap.h).
    pub(super) fn iter(&self) -> impl Iterator<Item = CoreEvent<'a>> + '_ {
        // SAFETY: the list's own functions, called with the list.
        let count = unsafe { (self.size)(self.list) };
        (0..count).filter_map(move |i| {
            // SAFETY: as above; `i` < size.
            let event = unsafe { (self.get)(self.list, i) };
            // SAFETY: null or a valid event, header first.
            let header = unsafe { event.as_ref() }?;
            if header.space_id != sys::CLAP_CORE_EVENT_SPACE_ID {
                return None;
            }
            // Cast from the host's pointer, not from `header`: a
            // reference to the header covers only the header's bytes.
            let param = |change| {
                // SAFETY: a core event of this type, whose layout
                // `ClapEventParamValue` mirrors.
                CoreEvent::Param(change, unsafe {
                    &*event.cast::<sys::ClapEventParamValue>()
                })
            };
            let note = |note_kind| {
                // SAFETY: a core note event.
                CoreEvent::Note(note_kind, unsafe { &*event.cast::<sys::ClapEventNote>() })
            };
            match header.r#type {
                sys::CLAP_EVENT_PARAM_VALUE => Some(param(ParamChangeKind::Value)),
                sys::CLAP_EVENT_PARAM_MOD => Some(param(ParamChangeKind::Modulation)),
                sys::CLAP_EVENT_NOTE_ON => Some(note(NoteKind::On)),
                sys::CLAP_EVENT_NOTE_OFF => Some(note(NoteKind::Off)),
                sys::CLAP_EVENT_NOTE_CHOKE => Some(note(NoteKind::Choke)),
                _ => None,
            }
        })
    }
}

pub(super) struct OutputEvents<'a> {
    list: *const sys::ClapOutputEvents,
    lifetime: core::marker::PhantomData<&'a sys::ClapOutputEvents>,
    try_push:
        unsafe extern "C" fn(*const sys::ClapOutputEvents, *const sys::ClapEventHeader) -> bool,
}

impl<'a> OutputEvents<'a> {
    /// # Safety
    /// `list` is null or a `clap_output_events` valid for the current call.
    pub(super) unsafe fn new(list: *const sys::ClapOutputEvents) -> Option<Self> {
        // SAFETY: this function's contract.
        let fns = unsafe { list.as_ref() }?;
        Some(Self {
            list,
            lifetime: core::marker::PhantomData,
            try_push: fns.try_push?,
        })
    }

    /// `event` must begin with its header (every `#[repr(C)]` CLAP
    /// event does) and `header.size` must be its size.
    pub(super) fn try_push<E>(&self, event: &E) -> bool {
        // SAFETY: the list's own function; the host copies `size` bytes
        // from the header, which `event` spans.
        unsafe { (self.try_push)(self.list, core::ptr::from_ref(event).cast()) }
    }
}

/// A host stream to save state into.
pub(super) struct OutputStream<'a> {
    stream: *const sys::ClapOstream,
    lifetime: core::marker::PhantomData<&'a sys::ClapOstream>,
    write: unsafe extern "C" fn(*const sys::ClapOstream, *const c_void, u64) -> i64,
}

impl<'a> OutputStream<'a> {
    /// # Safety
    /// `stream` is null or a `clap_ostream` valid for the current call.
    pub(super) unsafe fn new(stream: *const sys::ClapOstream) -> Result<Self, CallError> {
        // SAFETY: this function's contract.
        let fns = unsafe { stream.as_ref() }.ok_or(CallError::NullArgument("stream"))?;
        Ok(Self {
            stream,
            lifetime: core::marker::PhantomData,
            write: fns.write.ok_or(CallError::IncompleteStream)?,
        })
    }

    pub(super) fn write_all(&self, bytes: &[u8]) -> Result<(), CallError> {
        let mut pos = 0;
        while pos < bytes.len() {
            let rest = &bytes[pos..];
            // SAFETY: the stream's own function; `rest` is readable for
            // its length.
            let n = unsafe { (self.write)(self.stream, rest.as_ptr().cast(), rest.len() as u64) };
            if n <= 0 {
                return Err(CallError::StreamWrite {
                    written: pos,
                    of: bytes.len(),
                });
            }
            pos += n as usize;
        }
        Ok(())
    }
}

/// A host stream to load state from.
pub(super) struct InputStream<'a> {
    stream: *const sys::ClapIstream,
    lifetime: core::marker::PhantomData<&'a sys::ClapIstream>,
    read: unsafe extern "C" fn(*const sys::ClapIstream, *mut c_void, u64) -> i64,
}

impl<'a> InputStream<'a> {
    /// # Safety
    /// `stream` is null or a `clap_istream` valid for the current call.
    pub(super) unsafe fn new(stream: *const sys::ClapIstream) -> Result<Self, CallError> {
        // SAFETY: this function's contract.
        let fns = unsafe { stream.as_ref() }.ok_or(CallError::NullArgument("stream"))?;
        Ok(Self {
            stream,
            lifetime: core::marker::PhantomData,
            read: fns.read.ok_or(CallError::IncompleteStream)?,
        })
    }

    pub(super) fn read_to_end(&self, limit: usize) -> Result<Vec<u8>, CallError> {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            // SAFETY: the stream's own function; `chunk` is writable for
            // its length.
            let n =
                unsafe { (self.read)(self.stream, chunk.as_mut_ptr().cast(), chunk.len() as u64) };
            match n {
                0 => return Ok(bytes),
                n if n < 0 => return Err(CallError::StreamRead),
                n => bytes.extend_from_slice(&chunk[..n as usize]),
            }
            if bytes.len() > limit {
                return Err(CallError::StateTooLarge { limit });
            }
        }
    }
}

/// An out-parameter the host passed.
///
/// # Safety
/// `ptr` is null or valid for writes for the current call.
pub(crate) unsafe fn out_param<'a, T>(
    ptr: *mut T,
    name: &'static str,
) -> Result<&'a mut T, CallError> {
    // SAFETY: this function's contract.
    unsafe { ptr.as_mut() }.ok_or(CallError::NullArgument(name))
}

/// A text buffer of `size` chars the host passed for the plugin to fill.
///
/// # Safety
/// `ptr` is null or valid for `size` char writes for the current call.
pub(super) unsafe fn out_text<'a>(
    ptr: *mut c_char,
    size: u32,
    name: &'static str,
) -> Result<&'a mut [c_char], CallError> {
    if ptr.is_null() || size == 0 {
        return Err(CallError::NullArgument(name));
    }
    // SAFETY: this function's contract.
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr, size as usize) })
}

/// A nul-terminated string the host passed.
///
/// # Safety
/// `ptr` is null or a nul-terminated string valid for the current call.
pub(crate) unsafe fn in_text<'a>(
    ptr: *const c_char,
    name: &'static str,
) -> Result<&'a CStr, CallError> {
    if ptr.is_null() {
        return Err(CallError::NullArgument(name));
    }
    // SAFETY: this function's contract.
    Ok(unsafe { CStr::from_ptr(ptr) })
}
