//! `GetPropertyInfo`, `GetProperty`, `SetProperty` and the property
//! listeners: every property this Audio Unit serves, one `match` each
//! way. A property answers `kAudioUnitErr_InvalidProperty` when the unit
//! does not have it and `kAudioUnitErr_InvalidScope`/`_InvalidElement`
//! when it does but not there, as Apple's `AUBase` does.

// The properties are matched by their header names.
#![allow(non_upper_case_globals)]

use core::ffi::c_void;
use core::mem::size_of;

use stave_core::Tail;

use super::formats::{self, Listener, Source};
use super::*;

/// The private property a view uses to find its instance:
/// `[binary anchor, instance pointer]`.
pub(crate) const INSTANCE_PROPERTY: sys::AudioUnitPropertyID = sys::kAudioUnitProperty_FirstPrivate + 0x5354;

/// A static whose address names this binary (and so this plugin type),
/// so a view never takes another plugin's instance for its own.
pub(crate) static ANCHOR: u8 = 0;

/// Where `GetProperty` writes: `data` (null to ask only the size) and
/// its size, in and out.
struct Out {
    data: *mut c_void,
    size: *mut u32,
}

impl Out {
    /// Write `value`, or with no buffer report only its size.
    fn put<T: Copy>(&self, value: T) -> OSStatus {
        self.put_slice(core::slice::from_ref(&value))
    }

    fn put_slice<T: Copy>(&self, values: &[T]) -> OSStatus {
        let need = core::mem::size_of_val(values);
        // SAFETY: `size` is non-null (checked by `get_property`) and
        // `data` null or writable for `*size` bytes (AudioToolbox's
        // contract); unaligned writes, since nothing promises alignment.
        unsafe {
            if !self.data.is_null() {
                if (*self.size as usize) < need {
                    return sys::kAudio_ParamError;
                }
                for (i, v) in values.iter().enumerate() {
                    self.data.cast::<T>().add(i).write_unaligned(*v);
                }
            }
            *self.size = need as u32;
        }
        sys::noErr
    }

    /// The caller's payload, read before it is answered in place.
    fn read<T: Copy>(&self) -> Result<T, OSStatus> {
        // SAFETY: as `put_slice`; the size is checked first.
        unsafe {
            if self.data.is_null() || (*self.size as usize) < size_of::<T>() {
                return Err(sys::kAudio_ParamError);
            }
            Ok(self.data.cast::<T>().read_unaligned())
        }
    }
}

/// What `SetProperty` was handed.
struct In {
    data: *const c_void,
    size: u32,
}

impl In {
    fn read<T: Copy>(&self) -> Result<T, OSStatus> {
        if self.data.is_null() || (self.size as usize) < size_of::<T>() {
            return Err(sys::kAudio_ParamError);
        }
        // SAFETY: readable for `size` bytes (AudioToolbox's contract),
        // at least a `T`.
        Ok(unsafe { self.data.cast::<T>().read_unaligned() })
    }
}

fn global(scope: sys::AudioUnitScope) -> Result<(), OSStatus> {
    if scope == sys::kAudioUnitScope_Global {
        Ok(())
    } else {
        Err(sys::kAudioUnitErr_InvalidScope)
    }
}

/// The element `scope`/`element` names exists.
fn element<P: AuExport>(scope: sys::AudioUnitScope, element: sys::AudioUnitElement) -> Result<(), OSStatus> {
    let count = match scope {
        sys::kAudioUnitScope_Global | sys::kAudioUnitScope_Output => 1,
        sys::kAudioUnitScope_Input => formats::input_elements::<P>(),
        _ => return Err(sys::kAudioUnitErr_InvalidScope),
    };
    if (element as usize) < count {
        Ok(())
    } else {
        Err(sys::kAudioUnitErr_InvalidElement)
    }
}

/// Size and writability, for every property `get`/`set` serve.
fn info<P: AuExport>(
    inst: &Instance<P>,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
) -> Result<(usize, bool), OSStatus> {
    use sys::*;
    let ptr = size_of::<*const c_void>();
    Ok(match id {
        kAudioUnitProperty_ClassInfo => (global(scope).map(|_| ptr)?, true),
        kAudioUnitProperty_MakeConnection | kAudioUnitProperty_SetRenderCallback => {
            if scope != kAudioUnitScope_Input {
                return Err(kAudioUnitErr_InvalidScope);
            }
            element::<P>(scope, elem)?;
            (if id == kAudioUnitProperty_MakeConnection { size_of::<AudioUnitConnection>() } else { size_of::<AURenderCallbackStruct>() }, true)
        }
        kAudioUnitProperty_SampleRate => (element::<P>(scope, elem).map(|_| size_of::<f64>())?, true),
        kAudioUnitProperty_ParameterList => {
            let n = if scope == kAudioUnitScope_Global { inst.params.count() } else { 0 };
            (n * size_of::<u32>(), false)
        }
        kAudioUnitProperty_ParameterInfo => {
            params::param(inst, scope, elem)?;
            (size_of::<AudioUnitParameterInfo>(), false)
        }
        kAudioUnitProperty_ParameterValueStrings => {
            let p = params::param(inst, scope, elem)?;
            if !params::indexed(p) {
                return Err(kAudioUnitErr_InvalidProperty);
            }
            (ptr, false)
        }
        kAudioUnitProperty_ParameterStringFromValue => (global(scope).map(|_| size_of::<AudioUnitParameterStringFromValue>())?, false),
        kAudioUnitProperty_ParameterValueFromString => (global(scope).map(|_| size_of::<AudioUnitParameterValueFromString>())?, false),
        kAudioUnitProperty_StreamFormat => {
            if scope == kAudioUnitScope_Global {
                return Err(kAudioUnitErr_InvalidScope);
            }
            element::<P>(scope, elem)?;
            (size_of::<AudioStreamBasicDescription>(), true)
        }
        kAudioUnitProperty_ElementCount => (size_of::<u32>(), false),
        kAudioUnitProperty_Latency | kAudioUnitProperty_TailTime => (global(scope).map(|_| size_of::<f64>())?, false),
        kAudioUnitProperty_SupportedNumChannels => {
            global(scope)?;
            (formats::supported::<P>().len() * size_of::<AUChannelInfo>(), false)
        }
        kAudioUnitProperty_MaximumFramesPerSlice => (global(scope).map(|_| size_of::<u32>())?, true),
        kAudioUnitProperty_BypassEffect if Instance::<P>::is_effect() => (global(scope).map(|_| size_of::<u32>())?, true),
        kAudioUnitProperty_LastRenderError => (global(scope).map(|_| size_of::<i32>())?, false),
        kAudioUnitProperty_HostCallbacks => (global(scope).map(|_| size_of::<HostCallbackInfo>())?, true),
        kAudioUnitProperty_InPlaceProcessing | kAudioUnitProperty_OfflineRender => {
            (global(scope).map(|_| size_of::<u32>())?, true)
        }
        kAudioUnitProperty_CPULoad => (global(scope).map(|_| size_of::<f64>())?, false),
        kAudioUnitProperty_FactoryPresets if !inst.factory.is_empty() => (global(scope).map(|_| ptr)?, false),
        kAudioUnitProperty_PresentPreset => (global(scope).map(|_| size_of::<AUPreset>())?, true),
        kAudioUnitProperty_CocoaUI if inst.face.is_some() => {
            (global(scope).map(|_| size_of::<AudioUnitCocoaViewInfo>())?, false)
        }
        kMusicDeviceProperty_InstrumentCount if !Instance::<P>::is_effect() => (global(scope).map(|_| size_of::<u32>())?, false),
        INSTANCE_PROPERTY => (global(scope).map(|_| 2 * ptr)?, false),
        _ => return Err(kAudioUnitErr_InvalidProperty),
    })
}

fn get<P: AuExport>(
    inst: &Instance<P>,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    out: &Out,
) -> OSStatus {
    use sys::*;
    if let Err(status) = info(inst, id, scope, elem) {
        return status;
    }
    let rate = f64::from(inst.shared.sample_rate());
    match id {
        kAudioUnitProperty_ClassInfo => out.put(state::save(inst).into_raw()),
        kAudioUnitProperty_MakeConnection => match lock(&inst.io).sources[elem as usize] {
            Source::Connection(c) => out.put(c),
            _ => out.put(AudioUnitConnection { sourceAudioUnit: core::ptr::null_mut(), sourceOutputNumber: 0, destInputNumber: elem }),
        },
        kAudioUnitProperty_SetRenderCallback => kAudioUnitErr_InvalidProperty,
        kAudioUnitProperty_SampleRate => out.put(lock(&inst.props).sample_rate),
        kAudioUnitProperty_ParameterList => {
            if scope == kAudioUnitScope_Global {
                out.put_slice(&params::list(inst))
            } else {
                out.put_slice::<u32>(&[])
            }
        }
        kAudioUnitProperty_ParameterInfo => match params::param(inst, scope, elem) {
            Ok(p) => out.put(params::info(p)),
            Err(status) => status,
        },
        kAudioUnitProperty_ParameterValueStrings => match params::param(inst, scope, elem).map(params::value_strings) {
            Ok(Some(names)) => out.put(cf::strings(&names).into_raw()),
            Ok(None) => kAudioUnitErr_InvalidProperty,
            Err(status) => status,
        },
        kAudioUnitProperty_ParameterStringFromValue => {
            let mut q: AudioUnitParameterStringFromValue = match out.read() {
                Ok(q) => q,
                Err(status) => return status,
            };
            let Ok(p) = params::param(inst, kAudioUnitScope_Global, q.inParamID) else {
                return kAudioUnitErr_InvalidParameter;
            };
            let value = if q.inValue.is_null() {
                p.value()
            } else {
                // SAFETY: a non-null value pointer is the caller's f32.
                f64::from(unsafe { *q.inValue })
            };
            q.outString = cf::string(&p.format(value)).into_raw();
            out.put(q)
        }
        kAudioUnitProperty_ParameterValueFromString => {
            let mut q: AudioUnitParameterValueFromString = match out.read() {
                Ok(q) => q,
                Err(status) => return status,
            };
            let Ok(p) = params::param(inst, kAudioUnitScope_Global, q.inParamID) else {
                return kAudioUnitErr_InvalidParameter;
            };
            // SAFETY: the caller's CFString (type-checked by the read).
            let parsed = unsafe { cf::read_string(q.inString) }.and_then(|s| p.parse(&s));
            match parsed {
                Some(v) => {
                    q.outValue = v as f32;
                    out.put(q)
                }
                None => kAudioUnitErr_InvalidPropertyValue,
            }
        }
        kAudioUnitProperty_StreamFormat => {
            let props = lock(&inst.props);
            let channels = if scope == kAudioUnitScope_Output { props.output } else { props.inputs[elem as usize] };
            out.put(formats::canonical(props.sample_rate, channels))
        }
        kAudioUnitProperty_ElementCount => out.put(match scope {
            kAudioUnitScope_Global | kAudioUnitScope_Output => 1u32,
            kAudioUnitScope_Input => formats::input_elements::<P>() as u32,
            _ => 0,
        }),
        kAudioUnitProperty_Latency => out.put(f64::from(inst.shared.latency()) / rate),
        kAudioUnitProperty_TailTime => {
            let tail = match inst.tail.load(Ordering::Acquire) {
                u32::MAX => Tail::Infinite,
                0 => Tail::None,
                n => Tail::Samples(n),
            };
            // An endless tail reads as a day: hosts take any large value
            // as "keep processing".
            out.put(tail.to_samples().map_or(86_400.0, |n| f64::from(n) / rate))
        }
        kAudioUnitProperty_SupportedNumChannels => out.put_slice(&formats::supported::<P>()),
        kAudioUnitProperty_MaximumFramesPerSlice => out.put(lock(&inst.props).max_frames),
        kAudioUnitProperty_BypassEffect => out.put(u32::from(inst.effect_bypass.load(Ordering::Relaxed))),
        kAudioUnitProperty_LastRenderError => out.put(inst.last_error.swap(0, Ordering::Relaxed)),
        kAudioUnitProperty_HostCallbacks => match lock(&inst.io).host {
            Some(h) => out.put(h),
            None => out.put(HostCallbackInfo {
                hostUserData: core::ptr::null_mut(),
                beatAndTempoProc: None,
                musicalTimeLocationProc: None,
                transportStateProc: None,
                transportStateProc2: None,
            }),
        },
        // Outputs are never written over the inputs' buffers.
        kAudioUnitProperty_InPlaceProcessing => out.put(0u32),
        kAudioUnitProperty_FactoryPresets => {
            let items: Vec<*const c_void> = inst.factory.iter().map(|p| core::ptr::from_ref(p).cast()).collect();
            out.put(cf::pointers(&items).into_raw())
        }
        kAudioUnitProperty_PresentPreset => {
            let (number, name) = lock(&inst.present).clone();
            out.put(AUPreset { presetNumber: number, presetName: cf::string(&name).into_raw() })
        }
        kAudioUnitProperty_OfflineRender => out.put(lock(&inst.props).offline),
        kAudioUnitProperty_CPULoad => out.put(0.0f64),
        kAudioUnitProperty_CocoaUI => match view::cocoa_view_info::<P>() {
            Some(info) => out.put(info),
            None => kAudioUnitErr_InvalidProperty,
        },
        kMusicDeviceProperty_InstrumentCount => out.put(0u32),
        INSTANCE_PROPERTY => out.put([core::ptr::from_ref(&ANCHOR) as usize, core::ptr::from_ref(inst) as usize]),
        _ => kAudioUnitErr_InvalidProperty,
    }
}

fn set<P: AuExport>(
    inst: &Instance<P>,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    input: &In,
) -> OSStatus {
    use sys::*;
    match info(inst, id, scope, elem) {
        Ok((_, true)) => {}
        Ok((_, false)) => return kAudioUnitErr_PropertyNotWritable,
        Err(status) => return status,
    }
    match id {
        kAudioUnitProperty_ClassInfo => match input.read::<CFPropertyListRef>() {
            // SAFETY: the host's property list (a live CF object or null).
            Ok(list) => unsafe { state::load(inst, list) },
            Err(status) => status,
        },
        kAudioUnitProperty_MakeConnection => match input.read::<AudioUnitConnection>() {
            Ok(c) => {
                lock(&inst.io).sources[elem as usize] =
                    if c.sourceAudioUnit.is_null() { Source::None } else { Source::Connection(c) };
                noErr
            }
            Err(status) => status,
        },
        kAudioUnitProperty_SetRenderCallback => match input.read::<AURenderCallbackStruct>() {
            Ok(cb) => {
                lock(&inst.io).sources[elem as usize] =
                    if cb.inputProc.is_some() { Source::Callback(cb) } else { Source::None };
                noErr
            }
            Err(status) => status,
        },
        kAudioUnitProperty_SampleRate => match input.read::<f64>() {
            Ok(rate) if rate > 0.0 && rate.is_finite() => change(inst, |p| p.sample_rate = rate),
            Ok(_) => kAudioUnitErr_InvalidPropertyValue,
            Err(status) => status,
        },
        kAudioUnitProperty_StreamFormat => match input.read::<AudioStreamBasicDescription>() {
            Ok(f) if formats::is_canonical(&f) && formats::channels_allowed::<P>(scope, elem, f.mChannelsPerFrame) => {
                set_format(inst, scope, elem, &f)
            }
            Ok(_) => kAudioUnitErr_FormatNotSupported,
            Err(status) => status,
        },
        kAudioUnitProperty_MaximumFramesPerSlice => match input.read::<u32>() {
            Ok(frames) if frames > 0 => {
                let status = change(inst, |p| p.max_frames = frames);
                if status == noErr {
                    inst.changed(kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, 0);
                }
                status
            }
            Ok(_) => kAudioUnitErr_InvalidPropertyValue,
            Err(status) => status,
        },
        kAudioUnitProperty_BypassEffect => match input.read::<u32>() {
            Ok(on) => {
                inst.effect_bypass.store(on != 0, Ordering::Relaxed);
                noErr
            }
            Err(status) => status,
        },
        kAudioUnitProperty_HostCallbacks => {
            // A host may pass an older, shorter struct: take what it
            // covers, leave the rest null.
            let mut h = HostCallbackInfo {
                hostUserData: core::ptr::null_mut(),
                beatAndTempoProc: None,
                musicalTimeLocationProc: None,
                transportStateProc: None,
                transportStateProc2: None,
            };
            if input.data.is_null() {
                return kAudio_ParamError;
            }
            let n = (input.size as usize).min(size_of::<HostCallbackInfo>());
            // SAFETY: `n` bytes are readable (the contract) and fit `h`;
            // every bit pattern of the prefix is a pointer or an
            // optional function pointer, as the host wrote it.
            unsafe { core::ptr::copy_nonoverlapping(input.data.cast::<u8>(), (&mut h as *mut HostCallbackInfo).cast::<u8>(), n) };
            lock(&inst.io).host = Some(h);
            noErr
        }
        kAudioUnitProperty_InPlaceProcessing => noErr,
        kAudioUnitProperty_PresentPreset => match input.read::<AUPreset>() {
            Ok(p) => present(inst, p),
            Err(status) => status,
        },
        kAudioUnitProperty_OfflineRender => match input.read::<u32>() {
            Ok(v) => {
                lock(&inst.props).offline = v;
                noErr
            }
            Err(status) => status,
        },
        _ => kAudioUnitErr_InvalidProperty,
    }
}

/// Choose the present preset: a factory preset by number (its values
/// applied), or the host's own by name (negative number).
fn present<P: AuExport>(inst: &Instance<P>, p: sys::AUPreset) -> OSStatus {
    let name = if p.presetNumber >= 0 {
        let Some(preset) = inst.presets.get(p.presetNumber as usize) else {
            return sys::kAudioUnitErr_InvalidPropertyValue;
        };
        for &(id, value) in &preset.values {
            if let Some(param) = inst.params.by_id(id) {
                param.set_value(value);
            }
        }
        preset.name.to_string()
    } else {
        // SAFETY: the host's CFString, or null (type-checked by the read).
        unsafe { cf::read_string(p.presetName) }.unwrap_or_else(|| state::UNTITLED.to_string())
    };
    *lock(&inst.present) = (p.presetNumber.max(-1), name);
    params::all_changed(inst);
    inst.changed(sys::kAudioUnitProperty_PresentPreset, sys::kAudioUnitScope_Global, 0);
    sys::noErr
}

/// Apply a rate or block-size change; an initialized unit is prepared
/// again for it.
fn change<P: AuExport>(inst: &Instance<P>, apply: impl FnOnce(&mut formats::Props)) -> OSStatus {
    let mut props = lock(&inst.props);
    let before = (props.sample_rate, props.max_frames);
    apply(&mut props);
    if !props.initialized || before == (props.sample_rate, props.max_frames) {
        return sys::noErr;
    }
    let status = formats::prepare(inst, &mut props);
    drop(props);
    inst.changed(sys::kAudioUnitProperty_Latency, sys::kAudioUnitScope_Global, 0);
    status
}

/// A stream format: its rate is every element's; its channel count is
/// the element's, and fixed while initialized.
fn set_format<P: AuExport>(
    inst: &Instance<P>,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    f: &sys::AudioStreamBasicDescription,
) -> OSStatus {
    {
        let mut props = lock(&inst.props);
        let current = if scope == sys::kAudioUnitScope_Output { props.output } else { props.inputs[elem as usize] };
        if current != f.mChannelsPerFrame {
            if props.initialized {
                return sys::kAudioUnitErr_PropertyNotWritable;
            }
            match scope {
                sys::kAudioUnitScope_Output => props.output = f.mChannelsPerFrame,
                _ => props.inputs[elem as usize] = f.mChannelsPerFrame,
            }
        }
    }
    let status = change(inst, |p| p.sample_rate = f.mSampleRate);
    if status == sys::noErr {
        inst.changed(sys::kAudioUnitProperty_StreamFormat, scope, elem);
    }
    status
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; the out-pointers
/// are null or writable.
pub(crate) unsafe extern "C" fn get_property_info<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    size: *mut u32,
    writable: *mut sys::Boolean,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    inst.guarded("GetPropertyInfo", || sys::kAudioUnitErr_CannotDoInCurrentContext, || match info(inst, id, scope, elem) {
        Ok((n, w)) => {
            // SAFETY: each out-pointer is null or writable (the contract).
            unsafe {
                if !size.is_null() {
                    *size = n as u32;
                }
                if !writable.is_null() {
                    *writable = u8::from(w);
                }
            }
            sys::noErr
        }
        Err(status) => status,
    })
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; `data` is null
/// or writable for `*size` bytes.
pub(crate) unsafe extern "C" fn get_property<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    data: *mut c_void,
    size: *mut u32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if size.is_null() {
        return sys::kAudio_ParamError;
    }
    inst.guarded("GetProperty", || sys::kAudioUnitErr_CannotDoInCurrentContext, || get(inst, id, scope, elem, &Out { data, size }))
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`; `data` is null or
/// readable for `size` bytes.
pub(crate) unsafe extern "C" fn set_property<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    scope: sys::AudioUnitScope,
    elem: sys::AudioUnitElement,
    data: *const c_void,
    size: u32,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    if data.is_null() {
        // A null value removes what a few properties hold.
        return match id {
            sys::kAudioUnitProperty_SetRenderCallback | sys::kAudioUnitProperty_MakeConnection
                if scope == sys::kAudioUnitScope_Input && element::<P>(scope, elem).is_ok() =>
            {
                lock(&inst.io).sources[elem as usize] = Source::None;
                sys::noErr
            }
            _ => sys::kAudio_ParamError,
        };
    }
    inst.guarded("SetProperty", || sys::kAudioUnitErr_CannotDoInCurrentContext, || set(inst, id, scope, elem, &In { data, size }))
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn add_listener<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    proc_: Option<sys::AudioUnitPropertyListenerProc>,
    refcon: *mut c_void,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let Some(proc_) = proc_ else {
        return sys::kAudio_ParamError;
    };
    lock(&inst.props).listeners.push(Listener { id, proc_, refcon });
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn remove_listener<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    proc_: Option<sys::AudioUnitPropertyListenerProc>,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let target = proc_.map_or(0, |p| p as usize);
    lock(&inst.props).listeners.retain(|l| !(l.id == id && l.proc_ as usize == target));
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn remove_listener_with_user_data<P: AuExport>(
    this: *mut c_void,
    id: sys::AudioUnitPropertyID,
    proc_: Option<sys::AudioUnitPropertyListenerProc>,
    refcon: *mut c_void,
) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let target = proc_.map_or(0, |p| p as usize);
    lock(&inst.props).listeners.retain(|l| !(l.id == id && l.proc_ as usize == target && l.refcon == refcon));
    sys::noErr
}
