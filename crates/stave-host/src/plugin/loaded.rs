//! `LoadedPlugin` — a VST3 instance, activated and processing, as an
//! [`Instance`]. Built by [`open`](super::open); torn down by `Drop` in
//! spec order: setProcessing(false), setActive(false), disconnect,
//! terminate + release the controller, then the component, then the
//! factory, then `bundleExit` and dlclose — the field order below.

use std::path::PathBuf;

use stave_telemetry::Telemetry;
use stave_vst3_sys::{
    AudioBusBuffers, IAudioProcessorVtbl, IComponentVtbl, ProcessData, K_OUT_OF_MEMORY, K_RESULT_OK,
    PROCESS_MODE_REALTIME, SAMPLE_SIZE_32,
};

use super::com::connection::Connection;
use super::com::event_list::EventList;
use super::com::param_changes::ParamChanges;
use super::com::stream::StreamHandle;
use super::com::{Com, Initialized};
use super::controller::{Controller, ControllerHandle};
use super::factory::{ClassInfo, Factory};
use super::module::Module;
use super::telemetry::TelemetryRef;
use super::trace::LoadTrace;
use super::transport::Transport;
use crate::param::{Param, ParamError};
use crate::instance::{Instance, Layout, PluginMessage, TapReading, StateError, Tail};
use stave_core::Event;

/// How the plugin's parameters are reached.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerKind {
    None,
    /// The component is its own controller (stave plugins).
    SingleObject,
    /// A separate controller object (the commercial norm).
    Separate,
}

/// What was loaded, and how the load went.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginInfo {
    pub bundle: Option<PathBuf>,
    pub class: ClassInfo,
    pub factory_vendor: Option<String>,
    pub controller: ControllerKind,
    /// Component and controller wired via IConnectionPoint.
    pub connected: bool,
    pub event_inputs: bool,
    /// A stave plugin: its telemetry is readable.
    pub stave_telemetry: bool,
    pub load: LoadTrace,
}

/// Host-side counters: everything the host had to refuse, by cause.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct HostStats {
    pub blocks: u64,
    /// Note events past the per-block event capacity.
    pub dropped_events: u64,
    /// Parameter changes past the per-block queue capacity.
    pub dropped_param_changes: u64,
    /// Controller edits past the edit queue's capacity.
    pub dropped_edits: u64,
}

/// Audio buffers the plugin reads and writes through raw pointers.
pub(super) struct Io {
    /// Input scratch: plugins may write their inputs; callers' never are.
    pub inputs: Vec<Vec<f32>>,
    /// Output channels beyond what the caller supplied.
    pub spare_outputs: Vec<Vec<f32>>,
    pub in_ptrs: Vec<*mut f32>,
    pub out_ptrs: Vec<*mut f32>,
}

impl Io {
    pub fn new(layout: &Layout) -> Self {
        Self {
            inputs: vec![vec![0.0; layout.max_block]; layout.inputs],
            spare_outputs: vec![vec![0.0; layout.max_block]; layout.outputs],
            in_ptrs: vec![std::ptr::null_mut(); layout.inputs],
            out_ptrs: vec![std::ptr::null_mut(); layout.outputs],
        }
    }
}

pub struct LoadedPlugin {
    pub(super) layout: Layout,
    pub(super) info: PluginInfo,
    pub(super) io: Io,
    pub(super) events: Box<EventList>,
    pub(super) changes: Box<ParamChanges>,
    pub(super) transport: Transport,
    pub(super) chunk_events: Vec<Event>,
    pub(super) stats: HostStats,
    pub(super) telemetry: *const Telemetry,
    pub(super) active: bool,
    pub(super) processing: bool,
    // ---- COM objects: dropped in declaration order (= teardown order).
    pub(super) _connection: Option<Connection>,
    pub(super) controller: Option<Controller>,
    pub(super) processor: Com<IAudioProcessorVtbl>,
    pub(super) component: Initialized<IComponentVtbl>,
    pub(super) _factory: Factory,
    pub(super) _module: Module,
}

// SAFETY: a LoadedPlugin is used by one thread at a time (built on the
// UI thread, then moved whole into the audio callback). Its raw
// pointers reference the plugin's heap objects and its own buffers,
// never thread-local state.
unsafe impl Send for LoadedPlugin {}

impl LoadedPlugin {
    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    pub fn stats(&self) -> HostStats {
        HostStats {
            dropped_edits: self.controller.as_ref().map_or(0, |c| c.edits().dropped()),
            ..self.stats
        }
    }

    /// A handle UIs keep across the plugin's move to the audio thread.
    pub fn controller_handle(&self) -> Option<ControllerHandle> {
        self.controller.as_ref().map(|c| c.handle().clone())
    }

    /// The object that creates the plugin's editor view: the controller
    /// (the separate one for split plugins — the live instance's, never
    /// a ghost), else the component. Borrowed: valid while this lives.
    pub fn editor_owner_raw(&self) -> *mut stave_vst3_sys::FUnknown {
        self.controller
            .as_ref()
            .map_or(self.component.as_ptr(), |c| c.com().as_ptr())
    }

    /// The plugin's telemetry, for callers that must keep its address
    /// after this value moves (the audio callback). Valid while the
    /// instance lives; null for non-stave plugins.
    pub fn telemetry_raw(&self) -> *const Telemetry {
        self.telemetry
    }

    fn telemetry(&self) -> Option<TelemetryRef<'_>> {
        // SAFETY: owned by the instance, which `self` keeps alive.
        unsafe { self.telemetry.as_ref() }.map(TelemetryRef)
    }

    pub(super) fn activate(&mut self) -> Result<(), super::HostError> {
        use super::HostError;
        let c = &self.component;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        let r = unsafe { (c.vtbl().set_active)(c.as_ptr(), 1) };
        (r == K_RESULT_OK)
            .then_some(())
            .ok_or_else(|| HostError::call("setActive")(r))?;
        self.active = true;
        let p = &self.processor;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        let r = unsafe { (p.vtbl().set_processing)(p.as_ptr(), 1) };
        // Some plugins return kNotImplemented: processing is implied.
        if r != K_RESULT_OK && r != stave_vst3_sys::K_NOT_IMPLEMENTED {
            return Err(HostError::call("setProcessing")(r));
        }
        self.processing = true;
        Ok(())
    }

    /// One plugin call over ≤ max_block frames whose pointers are set.
    fn process_raw(&mut self, frames: usize) {
        if let Some(c) = &self.controller {
            self.changes.clear();
            while let Some((id, norm)) = c.edits().pop() {
                if !self.changes.push(id, 0, norm) {
                    self.stats.dropped_param_changes += 1;
                }
            }
        }
        self.stats.dropped_events += self.events.set(&self.chunk_events) as u64;
        let mut in_bus = AudioBusBuffers {
            num_channels: self.layout.inputs as i32,
            silence_flags: 0,
            channel_buffers_32: self.io.in_ptrs.as_mut_ptr(),
        };
        let mut out_bus = AudioBusBuffers {
            num_channels: self.layout.outputs as i32,
            silence_flags: 0,
            channel_buffers_32: self.io.out_ptrs.as_mut_ptr(),
        };
        let mut data = ProcessData {
            process_mode: PROCESS_MODE_REALTIME,
            symbolic_sample_size: SAMPLE_SIZE_32,
            num_samples: frames as i32,
            num_inputs: i32::from(self.layout.inputs > 0),
            num_outputs: i32::from(self.layout.outputs > 0),
            inputs: if self.layout.inputs > 0 {
                &mut in_bus
            } else {
                std::ptr::null_mut()
            },
            outputs: if self.layout.outputs > 0 {
                &mut out_bus
            } else {
                std::ptr::null_mut()
            },
            input_parameter_changes: if self.changes.is_empty() {
                std::ptr::null_mut()
            } else {
                self.changes.as_raw()
            },
            output_parameter_changes: std::ptr::null_mut(),
            input_events: self.events.as_raw(),
            output_events: std::ptr::null_mut(),
            process_context: self.transport.next_block(frames),
        };
        let p = &self.processor;
        // SAFETY: every pointer in `data` is live for this call.
        unsafe { (p.vtbl().process)(p.as_ptr(), &mut data) };
        self.stats.blocks += 1;
    }
}

/// The frames a `process` call may run: the shortest of the first
/// `n_in` inputs and `n_out` outputs (the channels the plugin touches),
/// or 0 when the caller passed none of them.
fn run_length(inputs: &[&[f32]], outputs: &[&mut [f32]], n_in: usize, n_out: usize) -> usize {
    let ins = inputs.iter().take(n_in).map(|c| c.len());
    let outs = outputs.iter().take(n_out).map(|c| c.len());
    ins.chain(outs).min().unwrap_or(0)
}

impl Instance for LoadedPlugin {
    fn layout(&self) -> Layout {
        self.layout
    }

    fn latency(&self) -> u32 {
        let p = &self.processor;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        unsafe { (p.vtbl().get_latency_samples)(p.as_ptr()) }
    }

    fn tail(&self) -> Tail {
        let p = &self.processor;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        match unsafe { (p.vtbl().get_tail_samples)(p.as_ptr()) } {
            u32::MAX => Tail::Infinite,
            n => Tail::Samples(n),
        }
    }

    /// Any length: split into `max_block` chunks, events re-based per chunk.
    ///
    /// The run is the shortest channel the plugin reads or writes (every
    /// input and output up to the layout's counts), so a ragged caller can
    /// never make the plugin read or write past a slice. Output frames past
    /// that run are silenced rather than left stale.
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], events: &[Event]) {
        let total = run_length(inputs, outputs, self.layout.inputs, self.layout.outputs);
        for out in outputs.iter_mut().take(self.layout.outputs) {
            out[total..].fill(0.0);
        }
        let mut start = 0;
        while start < total {
            let n = (total - start).min(self.layout.max_block);
            for (c, scratch) in self.io.inputs.iter_mut().enumerate() {
                match inputs.get(c) {
                    Some(src) => scratch[..n].copy_from_slice(&src[start..start + n]),
                    None => scratch[..n].fill(0.0),
                }
                self.io.in_ptrs[c] = scratch.as_mut_ptr();
            }
            for c in 0..self.layout.outputs {
                self.io.out_ptrs[c] = match outputs.get_mut(c) {
                    Some(out) => out[start..].as_mut_ptr(),
                    None => self.io.spare_outputs[c].as_mut_ptr(),
                };
            }
            self.chunk_events.clear();
            self.chunk_events.extend(
                events
                    .iter()
                    .filter(|e| (start..start + n).contains(&e.frame))
                    .map(|e| Event {
                        frame: e.frame - start,
                        ..*e
                    }),
            );
            self.process_raw(n);
            start += n;
        }
    }

    fn params(&self) -> Vec<Param> {
        self.controller
            .as_ref()
            .map_or_else(Vec::new, |c| c.handle().params())
    }

    fn set_param(&mut self, name: &str, value: f64) -> Result<(), ParamError> {
        self.controller
            .as_ref()
            .ok_or_else(|| ParamError::NotFound(name.into()))?
            .handle()
            .set(name, value)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        let s = StreamHandle::new(Vec::new());
        let c = &self.component;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        match unsafe { (c.vtbl().get_state)(c.as_ptr(), s.as_funknown()) } {
            // A write past `MAX_STATE_BYTES` was refused: the bytes are
            // incomplete even if the plugin ignored the refusal.
            K_RESULT_OK if s.overflowed() => Err(StateError::Refused {
                call: "getState",
                result: K_OUT_OF_MEMORY,
            }),
            K_RESULT_OK => Ok(s.bytes()),
            result => Err(StateError::Refused {
                call: "getState",
                result,
            }),
        }
    }

    /// Session-load semantics: the component takes the bytes, then a
    /// separate controller is told (`setComponentState`) so its values
    /// follow the DSP's.
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let mut s = StreamHandle::new(bytes.to_vec());
        let c = &self.component;
        // SAFETY: the object's own method, called on the live object `Com` holds.
        let r = unsafe { (c.vtbl().set_state)(c.as_ptr(), s.as_funknown()) };
        if r != K_RESULT_OK {
            return Err(StateError::Refused {
                call: "setState",
                result: r,
            });
        }
        if let Some(ctrl) = self.controller.as_ref().filter(|c| c.is_separate()) {
            s.rewind();
            let com = ctrl.com();
            // SAFETY: the object's own method, called on the live object `Com` holds.
            unsafe { (com.vtbl().set_component_state)(com.as_ptr(), s.as_funknown()) };
        }
        Ok(())
    }

    fn taps(&self) -> Vec<TapReading> {
        self.telemetry().map(TelemetryRef::taps).unwrap_or_default()
    }

    fn messages(&self, from: u64) -> Vec<PluginMessage> {
        self.telemetry().map(|t| t.messages(from)).unwrap_or_default()
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if let Some(c) = &self.controller {
            c.retire();
        }
        // SAFETY: live objects; the flags record what succeeded.
        unsafe {
            if self.processing {
                (self.processor.vtbl().set_processing)(self.processor.as_ptr(), 0);
            }
            if self.active {
                (self.component.vtbl().set_active)(self.component.as_ptr(), 0);
            }
        }
        // Fields drop next, in declaration (= teardown) order.
    }
}
