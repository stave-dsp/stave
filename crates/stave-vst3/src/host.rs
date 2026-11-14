//! Safe views of what a VST3 host hands the plugin: its COM objects and
//! the process data. Each view has one `unsafe` constructor carrying the
//! SDK's contract — a pointer the host passes is null or a live object
//! for the call that passed it — and safe methods after it. A view keeps
//! the host's own pointer to pass back to the object's methods: a COM
//! object finds itself from `this`, which a pointer derived from a Rust
//! reference to the vtable header alone would not allow.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use stave_core::diagnostics::CallError;

use crate::sys::*;

/// The host's `IComponentHandler`, referenced while held. The VST3
/// threading model puts every handler call, and its replacement, on the
/// main thread.
pub(crate) struct ComponentHandler(AtomicPtr<IComponentHandler>);

impl ComponentHandler {
    pub(crate) const fn none() -> Self {
        Self(AtomicPtr::new(ptr::null_mut()))
    }

    /// Takes a reference on `handler`, releases the one held.
    ///
    /// # Safety
    /// `handler` is null or a live `IComponentHandler`.
    pub(crate) unsafe fn replace(&self, handler: *mut IComponentHandler) {
        if !handler.is_null() {
            // SAFETY: this function's contract.
            unsafe { ((*(*handler).vtbl).add_ref)(handler) };
        }
        release(self.0.swap(handler, Ordering::AcqRel));
    }

    fn with(&self, call: impl FnOnce(*mut IComponentHandler, &IComponentHandlerVtbl)) {
        let handler = self.0.load(Ordering::Acquire);
        if !handler.is_null() {
            // SAFETY: a held reference keeps the handler alive; replacing
            // it happens on this same (main) thread.
            call(handler, unsafe { &*(*handler).vtbl });
        }
    }

    pub(crate) fn begin_edit(&self, id: ParamId) {
        self.with(|h, v| {
            // SAFETY: the handler's own method, called with the handler.
            unsafe { (v.begin_edit)(h, id) };
        });
    }

    pub(crate) fn perform_edit(&self, id: ParamId, normalized: ParamValue) {
        self.with(|h, v| {
            // SAFETY: as above.
            unsafe { (v.perform_edit)(h, id, normalized) };
        });
    }

    pub(crate) fn end_edit(&self, id: ParamId) {
        self.with(|h, v| {
            // SAFETY: as above.
            unsafe { (v.end_edit)(h, id) };
        });
    }

    pub(crate) fn restart_component(&self, flags: i32) {
        self.with(|h, v| {
            // SAFETY: as above.
            unsafe { (v.restart_component)(h, flags) };
        });
    }
}

impl Drop for ComponentHandler {
    fn drop(&mut self) {
        release(*self.0.get_mut());
    }
}

fn release(handler: *mut IComponentHandler) {
    if !handler.is_null() {
        // SAFETY: a reference this slot took in `replace`.
        unsafe { ((*(*handler).vtbl).release)(handler) };
    }
}

/// A host `IBStream` for state.
pub(crate) struct Stream<'a> {
    stream: *mut IBStream,
    lifetime: PhantomData<&'a IBStream>,
}

impl Stream<'_> {
    /// # Safety
    /// `stream` is null or a live `IBStream` for the current call.
    pub(crate) unsafe fn new(stream: *mut FUnknown) -> Result<Self, CallError> {
        if stream.is_null() {
            return Err(CallError::NullArgument("state"));
        }
        Ok(Self {
            stream: stream.cast(),
            lifetime: PhantomData,
        })
    }

    fn vtbl(&self) -> &IBStreamVtbl {
        // SAFETY: a live stream (`new`).
        unsafe { &*(*self.stream).vtbl }
    }

    /// To end of stream. Hosts differ in signalling the end — `kResultOk`
    /// with nothing read, an error after the last byte, a short read —
    /// so each ends the read rather than fails it.
    pub(crate) fn read_to_end(&self, limit: usize) -> Result<Vec<u8>, CallError> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let mut read: i32 = 0;
            // SAFETY: the stream's own method; `chunk` is writable for
            // its length.
            let res = unsafe {
                (self.vtbl().read)(
                    self.stream,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as i32,
                    &mut read,
                )
            };
            if res != K_RESULT_OK || read <= 0 {
                return Ok(out);
            }
            // A host claiming more bytes than it was asked for is lying
            // about the buffer: refused, never sliced past its end.
            let Some(bytes) = chunk.get(..read as usize) else {
                return Err(CallError::StreamRead);
            };
            out.extend_from_slice(bytes);
            if out.len() > limit {
                return Err(CallError::StateTooLarge { limit });
            }
            if (read as usize) < chunk.len() {
                return Ok(out);
            }
        }
    }

    pub(crate) fn write_all(&self, bytes: &[u8]) -> Result<(), CallError> {
        let mut pos = 0usize;
        while pos < bytes.len() {
            let n = (bytes.len() - pos).min(1 << 20) as i32;
            let mut written: i32 = 0;
            // SAFETY: the stream's own method; the host only reads the
            // `n` bytes at `pos` (the SDK's `void*` is not a write).
            let res = unsafe {
                (self.vtbl().write)(
                    self.stream,
                    bytes[pos..].as_ptr() as *mut c_void,
                    n,
                    &mut written,
                )
            };
            if res != K_RESULT_OK || written <= 0 {
                return Err(CallError::StreamWrite {
                    written: pos,
                    of: bytes.len(),
                });
            }
            pos += written as usize;
        }
        Ok(())
    }
}

/// One `process` call's `ProcessData`.
pub(crate) struct Process<'a>(&'a ProcessData);

impl<'a> Process<'a> {
    /// # Safety
    /// `data` is null or the `ProcessData` of the current call, its
    /// pointers null or valid as the SDK documents them.
    pub(crate) unsafe fn new(data: *mut ProcessData) -> Option<Self> {
        // SAFETY: this function's contract.
        unsafe { data.as_ref() }.map(Self)
    }

    pub(crate) fn data(&self) -> &'a ProcessData {
        self.0
    }

    /// Input bus `bus`'s channel pointers; `None` when the host gave no
    /// such bus or no buffers for it.
    pub(crate) fn input(&self, bus: usize) -> Option<&'a [*mut f32]> {
        if bus >= self.0.num_inputs.max(0) as usize || self.0.inputs.is_null() {
            return None;
        }
        // SAFETY: `inputs` holds `num_inputs` buses.
        channels(unsafe { &*self.0.inputs.add(bus) })
    }

    /// The main output's channel pointers, marked not silent. `None`
    /// when the host gave no output bus (a parameter flush); `Some(None)`
    /// when it gave one without buffers.
    pub(crate) fn output(&self) -> Option<Option<&'a [*mut f32]>> {
        if self.0.num_outputs == 0 || self.0.outputs.is_null() {
            return None;
        }
        // SAFETY: `outputs` holds `num_outputs` buses, and the plugin
        // owns their silence flags for the call.
        let bus = unsafe { &mut *self.0.outputs };
        if bus.channel_buffers_32.is_null() {
            return Some(None);
        }
        bus.silence_flags = 0;
        let bus: &'a AudioBusBuffers = bus;
        Some(channels(bus))
    }

    /// Zero the main output and flag it silent: what a disabled instance
    /// plays.
    pub(crate) fn silence_output(&self) {
        let frames = self.0.num_samples.max(0) as usize;
        if let Some(Some(channels)) = self.output() {
            for &channel in channels.iter().filter(|c| !c.is_null()) {
                // SAFETY: an output channel is writable for `num_samples`
                // (SDK).
                unsafe { core::slice::from_raw_parts_mut(channel, frames) }.fill(0.0);
            }
            self.mark_output_silent();
        }
    }

    /// Every output channel silent (the SDK's hint to skip them).
    pub(crate) fn mark_output_silent(&self) {
        // SAFETY: as in `output`; only reached after it returned buffers.
        if let Some(bus) = unsafe { self.0.outputs.as_mut() } {
            bus.silence_flags = (1u64 << bus.num_channels.clamp(0, 63) as u64).wrapping_sub(1);
        }
    }

    pub(crate) fn param_changes(&self) -> Option<ParamChanges<'a>> {
        // SAFETY: null or live for the call.
        unsafe { ParamChanges::new(self.0.input_parameter_changes) }
    }

    pub(crate) fn events(&self) -> Option<EventList<'a>> {
        // SAFETY: null or live for the call.
        unsafe { EventList::new(self.0.input_events) }
    }

    pub(crate) fn context(&self) -> Option<&'a ProcessContext> {
        // SAFETY: null or valid for the call.
        unsafe { self.0.process_context.as_ref() }
    }
}

fn channels(bus: &AudioBusBuffers) -> Option<&[*mut f32]> {
    if bus.channel_buffers_32.is_null() {
        return None;
    }
    // SAFETY: non-null buffers hold `num_channels` channel pointers.
    Some(unsafe {
        core::slice::from_raw_parts(
            bus.channel_buffers_32.cast_const(),
            bus.num_channels.max(0) as usize,
        )
    })
}

/// The host's queues of parameter changes for one block.
pub(crate) struct ParamChanges<'a> {
    changes: *mut IParameterChanges,
    lifetime: PhantomData<&'a IParameterChanges>,
}

impl<'a> ParamChanges<'a> {
    /// # Safety
    /// `changes` is null or a live `IParameterChanges` for the current call.
    pub(crate) unsafe fn new(changes: *mut IParameterChanges) -> Option<Self> {
        (!changes.is_null()).then_some(Self {
            changes,
            lifetime: PhantomData,
        })
    }

    pub(crate) fn queues(&self) -> impl Iterator<Item = ParamQueue<'a>> + '_ {
        // SAFETY: a live object (`new`).
        let vtbl = unsafe { &*(*self.changes).vtbl };
        // SAFETY: its own method.
        let count = unsafe { (vtbl.get_parameter_count)(self.changes) };
        (0..count).filter_map(move |i| {
            // SAFETY: as above, `i` < count; a queue lives as long as its list.
            let queue = unsafe { (vtbl.get_parameter_data)(self.changes, i) };
            (!queue.is_null()).then_some(ParamQueue {
                queue,
                lifetime: PhantomData,
            })
        })
    }
}

pub(crate) struct ParamQueue<'a> {
    queue: *mut IParamValueQueue,
    lifetime: PhantomData<&'a IParamValueQueue>,
}

impl ParamQueue<'_> {
    fn vtbl(&self) -> &IParamValueQueueVtbl {
        // SAFETY: a live queue (from `ParamChanges::queues`).
        unsafe { &*(*self.queue).vtbl }
    }

    pub(crate) fn id(&self) -> ParamId {
        // SAFETY: the queue's own method.
        unsafe { (self.vtbl().get_parameter_id)(self.queue) }
    }

    /// (sample offset, normalized value), in the host's order.
    pub(crate) fn points(&self) -> impl Iterator<Item = (i32, ParamValue)> + '_ {
        // SAFETY: the queue's own method.
        let count = unsafe { (self.vtbl().get_point_count)(self.queue) };
        (0..count).filter_map(move |i| {
            let (mut offset, mut value) = (0, 0.0);
            // SAFETY: as above, with out-params we own.
            let res = unsafe { (self.vtbl().get_point)(self.queue, i, &mut offset, &mut value) };
            (res == K_RESULT_OK).then_some((offset, value))
        })
    }
}

/// The host's input events for one block.
pub(crate) struct EventList<'a> {
    list: *mut IEventList,
    lifetime: PhantomData<&'a IEventList>,
}

impl EventList<'_> {
    /// # Safety
    /// `list` is null or a live `IEventList` for the current call.
    pub(crate) unsafe fn new(list: *mut IEventList) -> Option<Self> {
        (!list.is_null()).then_some(Self {
            list,
            lifetime: PhantomData,
        })
    }

    /// Each event the host could hand over, copied out.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Event> + '_ {
        // SAFETY: a live list (`new`).
        let vtbl = unsafe { &*(*self.list).vtbl };
        // SAFETY: its own method.
        let count = unsafe { (vtbl.get_event_count)(self.list) };
        (0..count).filter_map(move |i| {
            // SAFETY: all-zero is a valid `Event` (integers, floats, a
            // union of plain data).
            let mut event: Event = unsafe { core::mem::zeroed() };
            // SAFETY: its own method, `i` < count, writing our `event`.
            let res = unsafe { (vtbl.get_event)(self.list, i, &mut event) };
            (res == K_RESULT_OK).then_some(event)
        })
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

/// A `String128` (128 UTF-16 units, terminator included) the host passed.
///
/// # Safety
/// `ptr` is null or a `String128` valid for the current call.
pub(crate) unsafe fn string128<'a>(
    ptr: *mut TChar,
    name: &'static str,
) -> Result<&'a mut String128, CallError> {
    // SAFETY: this function's contract.
    unsafe { ptr.cast::<String128>().as_mut() }.ok_or(CallError::NullArgument(name))
}

/// An array of `len` values the host passed; `None` when null with a
/// non-zero length or a negative length.
///
/// # Safety
/// `ptr` is null or valid for `len` reads for the current call.
pub(crate) unsafe fn host_array<'a, T>(ptr: *const T, len: i32) -> Option<&'a [T]> {
    match (ptr.is_null(), usize::try_from(len).ok()?) {
        (_, 0) => Some(&[]),
        (true, _) => None,
        // SAFETY: this function's contract.
        (false, len) => Some(unsafe { core::slice::from_raw_parts(ptr, len) }),
    }
}

/// A nul-terminated UTF-16 string the host passed, up to its terminator
/// (at most `max` units are read).
///
/// # Safety
/// `ptr` is null or a nul-terminated `TChar` string valid for the current
/// call, or one valid for `max` reads.
pub(crate) unsafe fn tchar_str<'a>(
    ptr: *const TChar,
    max: usize,
    name: &'static str,
) -> Result<&'a [TChar], CallError> {
    if ptr.is_null() {
        return Err(CallError::NullArgument(name));
    }
    // SAFETY: this function's contract: every unit up to the terminator
    // (or `max`) is readable.
    let len = (0..max)
        .take_while(|&i| unsafe { *ptr.add(i) } != 0)
        .count();
    // SAFETY: the `len` units just read.
    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}
