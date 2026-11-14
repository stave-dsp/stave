//! Parameters: the global scope's list (in table order, ids the
//! plugin's own), each one's info, values in plain units (as on the CLAP
//! wire), value strings for stepped params, text in both directions, and
//! the notifications that let a host record what the face does.
//!
//! The host's writes (`SetParameter`, `ScheduleParameters`) set the
//! table, which the plugin's smoothers read each block: block-accurate,
//! not sample-accurate. Edits from the face go through the instance's
//! `PluginController` (which writes the table) and [`AuGestures`], which
//! tells the host's listeners — AudioToolbox's `AUEventListenerNotify`,
//! the begin / value / end gesture a host records as automation.

use core::ffi::c_void;

use stave_core::params::{AnyParam, Unit};
use stave_core::{GestureSink, PluginController};

use super::*;

/// The param `id` names in the global scope.
pub(crate) fn param<P: AuExport>(
    inst: &Instance<P>,
    scope: sys::AudioUnitScope,
    id: sys::AudioUnitParameterID,
) -> Result<&dyn AnyParam, OSStatus> {
    if scope != sys::kAudioUnitScope_Global {
        return Err(sys::kAudioUnitErr_InvalidScope);
    }
    inst.params.by_id(ParamId(id)).ok_or(sys::kAudioUnitErr_InvalidParameter)
}

/// The global scope's parameter ids, in table order.
pub(crate) fn list<P: AuExport>(inst: &Instance<P>) -> Vec<u32> {
    (0..inst.params.count()).filter_map(|i| inst.params.by_index(i)).map(|p| p.meta().id.0).collect()
}

/// A stepped param whose positions have names (an enum or a switch).
pub(crate) fn indexed(p: &dyn AnyParam) -> bool {
    p.step_count() > 0 && p.step_name(0).is_some()
}

/// A param's AudioUnitParameterInfo. The name is a CFString the caller
/// releases (`kAudioUnitParameterFlag_CFNameRelease`), as is a custom
/// unit's label.
pub(crate) fn info(p: &dyn AnyParam) -> sys::AudioUnitParameterInfo {
    let meta = p.meta();
    let mut flags = sys::kAudioUnitParameterFlag_IsReadable
        | sys::kAudioUnitParameterFlag_IsWritable
        | sys::kAudioUnitParameterFlag_HasCFNameString
        | sys::kAudioUnitParameterFlag_CFNameRelease
        // The host asks the plugin for every value's text.
        | sys::kAudioUnitParameterFlag_ValuesHaveStrings;
    if p.display_log() {
        flags |= sys::kAudioUnitParameterFlag_DisplayLogarithmic;
    }
    if meta.flags.non_automatable {
        flags |= sys::kAudioUnitParameterFlag_NonRealTime;
    }
    if meta.flags.hidden {
        flags |= sys::kAudioUnitParameterFlag_ExpertMode;
    }
    let mut unit_name: sys::CFStringRef = core::ptr::null();
    let named = |i, want: &str| p.step_name(i).is_some_and(|n| n.eq_ignore_ascii_case(want));
    let switch = p.step_count() == 2 && (meta.flags.bypass || (named(0, "off") && named(1, "on")));
    let unit = if indexed(p) && switch {
        sys::kAudioUnitParameterUnit_Boolean
    } else if indexed(p) {
        sys::kAudioUnitParameterUnit_Indexed
    } else {
        match meta.unit {
            Unit::Db | Unit::DbFloor(_) => sys::kAudioUnitParameterUnit_Decibels,
            Unit::Hz => sys::kAudioUnitParameterUnit_Hertz,
            Unit::Ms => sys::kAudioUnitParameterUnit_Milliseconds,
            Unit::Ratio => sys::kAudioUnitParameterUnit_Ratio,
            // 0..=1 read as a percentage: the text says so, the number
            // is the param's own.
            Unit::Percent | Unit::Plain { .. } => sys::kAudioUnitParameterUnit_Generic,
            Unit::Custom { label, .. } => {
                unit_name = interned(label);
                sys::kAudioUnitParameterUnit_CustomUnit
            }
        }
    };
    let mut name = [0 as core::ffi::c_char; 52];
    for (dst, &b) in name.iter_mut().zip(meta.name.as_bytes().iter().take(51)) {
        *dst = b as core::ffi::c_char;
    }
    sys::AudioUnitParameterInfo {
        name,
        unitName: unit_name,
        clumpID: 0,
        cfNameString: cf::string(meta.name).into_raw(),
        unit,
        minValue: p.denormalize(0.0) as f32,
        maxValue: p.denormalize(1.0) as f32,
        defaultValue: p.denormalize(p.default_normalized()) as f32,
        flags,
    }
}

/// A custom unit's label as a CFString that lives for the process: hosts
/// do not release `unitName` (only the name, under `CFNameRelease`), so
/// each label is made once.
fn interned(label: &'static str) -> sys::CFStringRef {
    static LABELS: Mutex<Vec<(&'static str, usize)>> = Mutex::new(Vec::new());
    let mut labels = lock(&LABELS);
    if let Some(&(_, s)) = labels.iter().find(|(l, _)| *l == label) {
        return s as sys::CFStringRef;
    }
    let s = cf::string(label).into_raw();
    labels.push((label, s as usize));
    s
}

/// The names of a stepped param's positions, or `None`.
pub(crate) fn value_strings(p: &dyn AnyParam) -> Option<Vec<&'static str>> {
    indexed(p).then(|| (0..p.step_count()).map(|i| p.step_name(i).unwrap_or("")).collect())
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; `out` is null or
/// writable.
pub(crate) unsafe extern "C" fn get_parameter<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitParameterID,
    scope: sys::AudioUnitScope,
    _element: sys::AudioUnitElement,
    out: *mut f32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if out.is_null() {
        return sys::kAudio_ParamError;
    }
    match param(inst, scope, id) {
        // SAFETY: `out` is non-null and writable (the contract).
        Ok(p) => unsafe {
            *out = p.value() as f32;
            sys::noErr
        },
        Err(status) => status,
    }
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn set_parameter<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitParameterID,
    scope: sys::AudioUnitScope,
    _element: sys::AudioUnitElement,
    value: f32,
    _offset: u32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    match param(inst, scope, id) {
        Ok(p) => {
            p.set_value(f64::from(value));
            sys::noErr
        }
        Err(status) => status,
    }
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; `events` holds
/// `count` events.
pub(crate) unsafe extern "C" fn schedule_parameters<P: AuExport>(
    this: *mut c_void,
    events: *const sys::AudioUnitParameterEvent,
    count: u32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if events.is_null() || count == 0 {
        return sys::noErr;
    }
    // SAFETY: `events` holds `count` events (the contract).
    let events = unsafe { core::slice::from_raw_parts(events, count as usize) };
    for e in events {
        let Ok(p) = param(inst, e.scope, e.parameter) else {
            continue;
        };
        // SAFETY: the union member the event type names.
        let value = unsafe {
            match e.eventType {
                sys::kParameterEvent_Ramped => e.eventValues.ramp.endValue,
                _ => e.eventValues.immediate.value,
            }
        };
        p.set_value(f64::from(value));
    }
    sys::noErr
}

/// The controller a face of this instance edits through.
pub(crate) fn controller_of<P: AuExport>(inst: &Instance<P>) -> PluginController {
    PluginController::new(
        inst.params.clone(),
        inst.telemetry.clone(),
        inst.presets.clone(),
        Box::new(AuGestures { unit: inst.unit.clone() }),
    )
}

/// The [`GestureSink`] of an Audio Unit face: each edit an
/// `AUEventListenerNotify`, so the host records and undoes it. The
/// controller has already written the table. [main thread]
pub(crate) struct AuGestures {
    unit: Arc<AtomicPtr<c_void>>,
}

impl AuGestures {
    fn notify(&self, kind: u32, id: ParamId) {
        let unit = self.unit.load(Ordering::Acquire);
        if unit.is_null() {
            return; // the instance closed under the face
        }
        let event = sys::AudioUnitEvent {
            mEventType: kind,
            mArgument: sys::AudioUnitEventArgument {
                mParameter: sys::AudioUnitParameter {
                    mAudioUnit: unit,
                    mParameterID: id.0,
                    mScope: sys::kAudioUnitScope_Global,
                    mElement: 0,
                },
            },
        };
        // SAFETY: a complete event naming a live unit; AudioToolbox
        // copies it to the listeners.
        unsafe { sys::AUEventListenerNotify(core::ptr::null_mut(), core::ptr::null_mut(), &event) };
    }
}

impl GestureSink for AuGestures {
    fn begin(&self, id: ParamId) {
        self.notify(sys::kAudioUnitEvent_BeginParameterChangeGesture, id);
    }
    fn perform(&self, id: ParamId, _value: f64) {
        self.notify(sys::kAudioUnitEvent_ParameterValueChange, id);
    }
    fn end(&self, id: ParamId) {
        self.notify(sys::kAudioUnitEvent_EndParameterChangeGesture, id);
    }
}

/// Every parameter changed (a state restored): hosts re-read them all.
pub(crate) fn all_changed<P: AuExport>(inst: &Instance<P>) {
    let unit = inst.unit();
    if unit.is_null() {
        return;
    }
    let p = sys::AudioUnitParameter {
        mAudioUnit: unit,
        mParameterID: sys::kAUParameterListener_AnyParameter,
        mScope: sys::kAudioUnitScope_Global,
        mElement: 0,
    };
    // SAFETY: a complete parameter naming this live unit.
    unsafe { sys::AUParameterListenerNotify(core::ptr::null_mut(), core::ptr::null_mut(), &p) };
}
