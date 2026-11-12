//! The edit controller: the parameter inventory and the ONLY correct
//! write path (plain → normalized → `setParamNormalized`). Writing a
//! stave bus mirror does nothing for table plugins — the sweep that
//! measured 32 identical configs found that out.
//!
//! Split-architecture plugins (the commercial norm) have a separate
//! controller object, and their processor hears an edit only through
//! `ProcessData.inputParameterChanges`; every write is therefore also
//! queued for the next `process` call.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use stave_vst3_sys::{IEditControllerVtbl, ParamId, ParamValue, ParameterInfo, K_RESULT_OK};

use super::com::{utf16_z, Com, Initialized};
use crate::param::{find, Param, ParamError, ParamFlags};

// ParameterInfo::flags (VST3 ivsteditcontroller.h).
const CAN_AUTOMATE: i32 = 1 << 0;
const IS_READ_ONLY: i32 = 1 << 1;
const IS_HIDDEN: i32 = 1 << 4;
const IS_PROGRAM_CHANGE: i32 = 1 << 15;
const IS_BYPASS: i32 = 1 << 16;

/// Discrete parameters with at most this many positions get step names.
const MAX_NAMED_STEPS: i32 = 64;

/// Edits waiting for the processor (split plugins). Bounded; overflow
/// is counted, never silent.
pub(crate) struct Edits {
    queue: ArrayQueue<(ParamId, ParamValue)>,
    dropped: AtomicU64,
}

impl Edits {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            queue: ArrayQueue::new(capacity.max(64)),
            dropped: AtomicU64::new(0),
        })
    }

    fn push(&self, id: ParamId, norm: ParamValue) {
        if self.queue.push((id, norm)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn pop(&self) -> Option<(ParamId, ParamValue)> {
        self.queue.pop()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// How the controller object is owned.
pub(crate) enum Ownership {
    /// The component itself (single-object plugins, e.g. stave's).
    Shared(Com<IEditControllerVtbl>),
    /// A separately created + initialized object (split plugins).
    Separate(Initialized<IEditControllerVtbl>),
}

impl std::ops::Deref for Ownership {
    type Target = Com<IEditControllerVtbl>;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Shared(c) => c,
            Self::Separate(i) => i.com(),
        }
    }
}

pub(crate) struct Controller {
    obj: Ownership,
    handle: ControllerHandle,
}

impl Controller {
    pub fn new(obj: Ownership) -> Self {
        // SAFETY: a live controller we own a reference to.
        let n = unsafe { (obj.vtbl().get_parameter_count)(obj.as_ptr()) }.max(0) as usize;
        let mut handle = ControllerHandle {
            obj: obj.as_ptr(),
            separate: matches!(obj, Ownership::Separate(_)),
            edits: Edits::new(n * 4),
            alive: Arc::new(AtomicBool::new(true)),
            inventory: Arc::new(Vec::new()),
        };
        handle.inventory = Arc::new(handle.params());
        Self { obj, handle }
    }

    pub fn is_separate(&self) -> bool {
        matches!(self.obj, Ownership::Separate(_))
    }

    pub fn com(&self) -> &Com<IEditControllerVtbl> {
        &self.obj
    }

    pub fn edits(&self) -> &Arc<Edits> {
        &self.handle.edits
    }

    pub fn handle(&self) -> &ControllerHandle {
        &self.handle
    }

    /// Invalidate every handle (called before teardown).
    pub fn retire(&self) {
        self.handle.alive.store(false, Ordering::Release);
    }
}

/// A non-owning view of a live controller, for UIs that outlive one
/// plugin instance (stave-play reloads plugins). Every call is a no-op
/// once the plugin is torn down. Use it from the thread that tears the
/// plugin down (the UI thread) — liveness is checked, not locked.
#[derive(Clone)]
pub struct ControllerHandle {
    obj: *mut stave_vst3_sys::FUnknown,
    separate: bool,
    edits: Arc<Edits>,
    /// Cleared when the plugin is torn down: every call goes inert.
    alive: Arc<AtomicBool>,
    /// Ids, names and flags as of load (the inventory is static; only
    /// values move) — name lookups without re-walking the controller.
    inventory: Arc<Vec<Param>>,
}

impl ControllerHandle {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn vtbl(&self) -> Option<&IEditControllerVtbl> {
        // SAFETY: `alive` is cleared before the controller is released.
        self.is_alive()
            .then(|| unsafe { &**(self.obj as *const *const IEditControllerVtbl) })
    }

    /// Every parameter, values read live.
    pub fn params(&self) -> Vec<Param> {
        let Some(vt) = self.vtbl() else {
            return Vec::new();
        };
        // SAFETY: the controller's own method; `vtbl()` returned it, so the
        // controller is alive.
        let n = unsafe { (vt.get_parameter_count)(self.obj) };
        (0..n).filter_map(|i| self.param_at(vt, i)).collect()
    }

    fn param_at(&self, vt: &IEditControllerVtbl, index: i32) -> Option<Param> {
        // SAFETY: all-zero is a valid `ParameterInfo` (integers, a float,
        // UTF-16 arrays).
        let mut info = unsafe { std::mem::zeroed::<ParameterInfo>() };
        // SAFETY: the controller's own method on the live controller (the
        // caller holds `vt` from `vtbl()`), writing our `info`.
        if unsafe { (vt.get_parameter_info)(self.obj, index, &mut info) } != K_RESULT_OK {
            return None;
        }
        let plain = |norm: f64| {
            // SAFETY: as above.
            unsafe { (vt.normalized_param_to_plain)(self.obj, info.id, norm) }
        };
        // SAFETY: as above.
        let normalized = unsafe { (vt.get_param_normalized)(self.obj, info.id) };
        let name = utf16_z(&info.title);
        // A program list's positions are its preset names, however many.
        let program = info.flags & IS_PROGRAM_CHANGE != 0;
        // VST3 counts steps between positions; stave counts positions.
        let step_count = match info.step_count.max(0) as u32 {
            0 if program => 1,
            0 => 0,
            steps => steps + 1,
        };
        let step_names = if program || (1..=MAX_NAMED_STEPS).contains(&info.step_count) {
            (0..=info.step_count)
                .map(|k| self.value_string(vt, info.id, f64::from(k) / f64::from(info.step_count)))
                .collect()
        } else {
            Vec::new()
        };
        Some(Param {
            id: stave_core::params::ParamId(info.id),
            group: name
                .rsplit_once('·')
                .map(|(g, _)| g.trim().to_string())
                .unwrap_or_default(),
            name,
            unit: utf16_z(&info.units),
            min: plain(0.0),
            max: plain(1.0),
            default: plain(info.default_normalized_value),
            value: plain(normalized),
            normalized,
            step_count,
            step_names,
            flags: ParamFlags {
                bypass: info.flags & IS_BYPASS != 0,
                non_automatable: info.flags & CAN_AUTOMATE == 0,
                hidden: info.flags & IS_HIDDEN != 0,
            },
            read_only: info.flags & IS_READ_ONLY != 0,
            program,
        })
    }

    fn value_string(&self, vt: &IEditControllerVtbl, id: ParamId, norm: f64) -> String {
        let mut s = [0u16; 128];
        // SAFETY: the controller's own method on the live controller (the
        // caller holds `vt` from `vtbl()`); `s` is a `String128`.
        let r = unsafe { (vt.get_param_string_by_value)(self.obj, id, norm, s.as_mut_ptr()) };
        if r == K_RESULT_OK {
            utf16_z(&s)
        } else {
            String::new()
        }
    }

    fn writable(&self, name: &str) -> Result<&Param, ParamError> {
        let c = find(&self.inventory, name).ok_or_else(|| ParamError::NotFound(name.into()))?;
        if c.read_only {
            return Err(ParamError::ReadOnly(c.name.clone()));
        }
        Ok(c)
    }

    /// Set a parameter by name, in its own units.
    pub fn set(&self, name: &str, value: f64) -> Result<(), ParamError> {
        let id = self.writable(name)?.id;
        self.set_by_id(id.0, value);
        Ok(())
    }

    /// Set a parameter by name at a normalized position (its own taper).
    pub fn set_normalized(&self, name: &str, normalized: f64) -> Result<(), ParamError> {
        let id = self.writable(name)?.id;
        self.apply(id.0, normalized);
        Ok(())
    }

    /// The plugin's own text for `name` at a normalized position.
    pub fn display(&self, name: &str, normalized: f64) -> Option<String> {
        let vt = self.vtbl()?;
        let c = find(&self.inventory, name)?;
        Some(self.value_string(vt, c.id.0, normalized))
    }

    /// Set a parameter by id, PLAIN value.
    pub fn set_by_id(&self, id: ParamId, plain: f64) {
        let Some(vt) = self.vtbl() else { return };
        // SAFETY: the controller's own method; `vtbl()` returned it, so the
        // controller is alive.
        let normalized = unsafe { (vt.plain_param_to_normalized)(self.obj, id, plain) };
        self.apply(id, normalized);
    }

    fn apply(&self, id: ParamId, normalized: f64) {
        let Some(vt) = self.vtbl() else { return };
        // SAFETY: as in `set_by_id`.
        unsafe { (vt.set_param_normalized)(self.obj, id, normalized) };
        if self.separate {
            self.edits.push(id, normalized);
        }
    }

    /// Edits refused because the queue to the processor was full.
    pub fn dropped_edits(&self) -> u64 {
        self.edits.dropped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_queue_overflow_is_counted() {
        let e = Edits::new(0); // floor of 64
        for i in 0..70 {
            e.push(i, 0.0);
        }
        assert_eq!(e.dropped(), 6);
        assert_eq!(e.pop(), Some((0, 0.0)), "FIFO");
    }
}
