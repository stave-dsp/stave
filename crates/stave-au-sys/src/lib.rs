//! The raw Audio Unit (v2) ABI: the AudioToolbox and CoreFoundation
//! types, constants and function declarations an Audio Unit (`stave-au`)
//! and an Audio Unit host (`stave-host`) need, written by
//! hand against Apple's headers (`AudioUnit/AUComponent.h`,
//! `AudioUnitProperties.h`, `AudioComponent.h`, `MusicDevice.h`,
//! `AudioToolbox/AudioUnitUtilities.h`, `CoreAudioTypes.h`) — no bindings
//! crate, and no logic.
//!
//! Names follow the headers (so they read against Apple's documentation).
//! Every struct is `#[repr(C)]` and its size is asserted below for the
//! 64-bit targets macOS runs on. The types are portable (they compile
//! and their layouts are checked on every OS); the framework
//! declarations exist on macOS only.

#![cfg_attr(not(test), no_std)]
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

use core::ffi::{c_char, c_void};

pub type OSStatus = i32;
pub type AudioComponent = *mut c_void;
/// An Audio Unit instance (`AudioComponentInstance`).
pub type AudioUnit = *mut c_void;
pub type AudioUnitPropertyID = u32;
pub type AudioUnitScope = u32;
pub type AudioUnitElement = u32;
pub type AudioUnitParameterID = u32;
pub type AudioUnitParameterValue = f32;
pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFMutableDictionaryRef = *mut c_void;
pub type CFURLRef = *const c_void;
pub type CFPropertyListRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFTypeID = usize;
pub type CFIndex = isize;
/// CoreFoundation's `Boolean` (an unsigned char).
pub type Boolean = u8;

/// A four-character code, as the headers spell them (`'aufx'`).
pub const fn fourcc(s: &[u8; 4]) -> u32 {
    (s[0] as u32) << 24 | (s[1] as u32) << 16 | (s[2] as u32) << 8 | s[3] as u32
}

// ===========================================================================
// Components (AudioComponent.h)
// ===========================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioComponentDescription {
    pub componentType: u32,
    pub componentSubType: u32,
    pub componentManufacturer: u32,
    pub componentFlags: u32,
    pub componentFlagsMask: u32,
}

/// What a component's factory function returns: the instance's entry
/// points. `Lookup` answers a selector with the function that serves it
/// (each takes the interface pointer as its first argument), or null.
#[repr(C)]
pub struct AudioComponentPlugInInterface {
    pub Open: unsafe extern "C" fn(this: *mut c_void, instance: AudioUnit) -> OSStatus,
    pub Close: unsafe extern "C" fn(this: *mut c_void) -> OSStatus,
    pub Lookup: extern "C" fn(selector: i16) -> *const c_void,
    pub reserved: *mut c_void,
}

/// A component's factory (the `factoryFunction` its Info.plist names).
pub type AudioComponentFactoryFunction =
    unsafe extern "C" fn(desc: *const AudioComponentDescription) -> *mut AudioComponentPlugInInterface;

pub const kAudioUnitType_Output: u32 = fourcc(b"auou");
pub const kAudioUnitType_MusicDevice: u32 = fourcc(b"aumu");
pub const kAudioUnitType_MusicEffect: u32 = fourcc(b"aumf");
pub const kAudioUnitType_Effect: u32 = fourcc(b"aufx");
pub const kAudioUnitType_MIDIProcessor: u32 = fourcc(b"aumi");

// ===========================================================================
// Selectors (AUComponent.h, MusicDevice.h)
// ===========================================================================

pub const kAudioUnitInitializeSelect: i16 = 0x0001;
pub const kAudioUnitUninitializeSelect: i16 = 0x0002;
pub const kAudioUnitGetPropertyInfoSelect: i16 = 0x0003;
pub const kAudioUnitGetPropertySelect: i16 = 0x0004;
pub const kAudioUnitSetPropertySelect: i16 = 0x0005;
pub const kAudioUnitGetParameterSelect: i16 = 0x0006;
pub const kAudioUnitSetParameterSelect: i16 = 0x0007;
pub const kAudioUnitResetSelect: i16 = 0x0009;
pub const kAudioUnitAddPropertyListenerSelect: i16 = 0x000A;
pub const kAudioUnitRemovePropertyListenerSelect: i16 = 0x000B;
pub const kAudioUnitRenderSelect: i16 = 0x000E;
pub const kAudioUnitAddRenderNotifySelect: i16 = 0x000F;
pub const kAudioUnitRemoveRenderNotifySelect: i16 = 0x0010;
pub const kAudioUnitScheduleParametersSelect: i16 = 0x0011;
pub const kAudioUnitRemovePropertyListenerWithUserDataSelect: i16 = 0x0012;
pub const kMusicDeviceMIDIEventSelect: i16 = 0x0101;
pub const kMusicDeviceSysExSelect: i16 = 0x0102;

// ===========================================================================
// Errors (AUComponent.h, MacErrors.h)
// ===========================================================================

pub const noErr: OSStatus = 0;
pub const kAudio_ParamError: OSStatus = -50;
pub const kAudio_MemFullError: OSStatus = -108;
pub const kAudioUnitErr_InvalidProperty: OSStatus = -10879;
pub const kAudioUnitErr_InvalidParameter: OSStatus = -10878;
pub const kAudioUnitErr_InvalidElement: OSStatus = -10877;
pub const kAudioUnitErr_NoConnection: OSStatus = -10876;
pub const kAudioUnitErr_FailedInitialization: OSStatus = -10875;
pub const kAudioUnitErr_TooManyFramesToProcess: OSStatus = -10874;
pub const kAudioUnitErr_InvalidFile: OSStatus = -10871;
pub const kAudioUnitErr_FormatNotSupported: OSStatus = -10868;
pub const kAudioUnitErr_Uninitialized: OSStatus = -10867;
pub const kAudioUnitErr_InvalidScope: OSStatus = -10866;
pub const kAudioUnitErr_PropertyNotWritable: OSStatus = -10865;
pub const kAudioUnitErr_CannotDoInCurrentContext: OSStatus = -10863;
pub const kAudioUnitErr_InvalidPropertyValue: OSStatus = -10851;
pub const kAudioUnitErr_PropertyNotInUse: OSStatus = -10850;
pub const kAudioUnitErr_Initialized: OSStatus = -10849;
pub const kAudioUnitErr_Unauthorized: OSStatus = -10847;

// ===========================================================================
// Scopes and properties (AudioUnitProperties.h)
// ===========================================================================

pub const kAudioUnitScope_Global: AudioUnitScope = 0;
pub const kAudioUnitScope_Input: AudioUnitScope = 1;
pub const kAudioUnitScope_Output: AudioUnitScope = 2;
pub const kAudioUnitScope_Group: AudioUnitScope = 3;
pub const kAudioUnitScope_Part: AudioUnitScope = 4;

pub const kAudioUnitProperty_ClassInfo: AudioUnitPropertyID = 0;
pub const kAudioUnitProperty_MakeConnection: AudioUnitPropertyID = 1;
pub const kAudioUnitProperty_SampleRate: AudioUnitPropertyID = 2;
pub const kAudioUnitProperty_ParameterList: AudioUnitPropertyID = 3;
pub const kAudioUnitProperty_ParameterInfo: AudioUnitPropertyID = 4;
pub const kAudioUnitProperty_CPULoad: AudioUnitPropertyID = 6;
pub const kAudioUnitProperty_StreamFormat: AudioUnitPropertyID = 8;
pub const kAudioUnitProperty_ElementCount: AudioUnitPropertyID = 11;
pub const kAudioUnitProperty_Latency: AudioUnitPropertyID = 12;
pub const kAudioUnitProperty_SupportedNumChannels: AudioUnitPropertyID = 13;
pub const kAudioUnitProperty_MaximumFramesPerSlice: AudioUnitPropertyID = 14;
pub const kAudioUnitProperty_ParameterValueStrings: AudioUnitPropertyID = 16;
pub const kAudioUnitProperty_AudioChannelLayout: AudioUnitPropertyID = 19;
pub const kAudioUnitProperty_TailTime: AudioUnitPropertyID = 20;
pub const kAudioUnitProperty_BypassEffect: AudioUnitPropertyID = 21;
pub const kAudioUnitProperty_LastRenderError: AudioUnitPropertyID = 22;
pub const kAudioUnitProperty_SetRenderCallback: AudioUnitPropertyID = 23;
pub const kAudioUnitProperty_FactoryPresets: AudioUnitPropertyID = 24;
pub const kAudioUnitProperty_ContextName: AudioUnitPropertyID = 25;
pub const kAudioUnitProperty_HostCallbacks: AudioUnitPropertyID = 27;
pub const kAudioUnitProperty_InPlaceProcessing: AudioUnitPropertyID = 29;
pub const kAudioUnitProperty_ElementName: AudioUnitPropertyID = 30;
pub const kAudioUnitProperty_ParameterStringFromValue: AudioUnitPropertyID = 33;
pub const kAudioUnitProperty_PresentPreset: AudioUnitPropertyID = 36;
pub const kAudioUnitProperty_OfflineRender: AudioUnitPropertyID = 37;
pub const kAudioUnitProperty_ParameterValueFromString: AudioUnitPropertyID = 38;
pub const kAudioUnitProperty_ShouldAllocateBuffer: AudioUnitPropertyID = 51;
pub const kAudioUnitProperty_SupportsMPE: AudioUnitPropertyID = 58;
pub const kAudioUnitProperty_CocoaUI: AudioUnitPropertyID = 4013;
pub const kMusicDeviceProperty_InstrumentCount: AudioUnitPropertyID = 1000;

/// The first property id Apple leaves to an Audio Unit's own use.
pub const kAudioUnitProperty_FirstPrivate: AudioUnitPropertyID = 64000;

/// `kAudioUnitRenderAction_*` (the action flags of the audio callback).
pub const kAudioUnitRenderAction_PreRender: u32 = 1 << 2;
pub const kAudioUnitRenderAction_PostRender: u32 = 1 << 3;
pub const kAudioUnitRenderAction_OutputIsSilence: u32 = 1 << 4;

// ===========================================================================
// Audio data (CoreAudioTypes.h)
// ===========================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct AudioStreamBasicDescription {
    pub mSampleRate: f64,
    pub mFormatID: u32,
    pub mFormatFlags: u32,
    pub mBytesPerPacket: u32,
    pub mFramesPerPacket: u32,
    pub mBytesPerFrame: u32,
    pub mChannelsPerFrame: u32,
    pub mBitsPerChannel: u32,
    pub mReserved: u32,
}

pub const kAudioFormatLinearPCM: u32 = fourcc(b"lpcm");
pub const kAudioFormatFlagIsFloat: u32 = 1;
pub const kAudioFormatFlagIsBigEndian: u32 = 1 << 1;
pub const kAudioFormatFlagIsPacked: u32 = 1 << 3;
pub const kAudioFormatFlagIsNonInterleaved: u32 = 1 << 5;

/// One buffer of an [`AudioBufferList`] (named apart from the header, whose name the glossary retires).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AuBuffer {
    pub mNumberChannels: u32,
    pub mDataByteSize: u32,
    pub mData: *mut c_void,
}

/// The C type: a count, then that many buffers (the struct declares one;
/// the caller allocates the rest after it).
#[repr(C)]
pub struct AudioBufferList {
    pub mNumberBuffers: u32,
    pub mBuffers: [AuBuffer; 1],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SMPTETime {
    pub mSubframes: i16,
    pub mSubframeDivisor: i16,
    pub mCounter: u32,
    pub mType: u32,
    pub mFlags: u32,
    pub mHours: i16,
    pub mMinutes: i16,
    pub mSeconds: i16,
    pub mFrames: i16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AudioTimeStamp {
    pub mSampleTime: f64,
    pub mHostTime: u64,
    pub mRateScalar: f64,
    pub mWordClockTime: u64,
    pub mSMPTETime: SMPTETime,
    pub mFlags: u32,
    pub mReserved: u32,
}

pub const kAudioTimeStampSampleTimeValid: u32 = 1;

// ===========================================================================
// Property payloads (AudioUnitProperties.h)
// ===========================================================================

/// The audio callback's shape: a host's input provider, a notification around each block.
pub type AURenderCallback = unsafe extern "C" fn(
    refcon: *mut c_void,
    action_flags: *mut u32,
    time: *const AudioTimeStamp,
    bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> OSStatus;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AURenderCallbackStruct {
    pub inputProc: Option<AURenderCallback>,
    pub inputProcRefCon: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AudioUnitConnection {
    pub sourceAudioUnit: AudioUnit,
    pub sourceOutputNumber: u32,
    pub destInputNumber: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AUChannelInfo {
    pub inChannels: i16,
    pub outChannels: i16,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitParameterInfo {
    pub name: [c_char; 52],
    pub unitName: CFStringRef,
    pub clumpID: u32,
    pub cfNameString: CFStringRef,
    pub unit: u32,
    pub minValue: f32,
    pub maxValue: f32,
    pub defaultValue: f32,
    pub flags: u32,
}

pub const kAudioUnitParameterUnit_Generic: u32 = 0;
pub const kAudioUnitParameterUnit_Indexed: u32 = 1;
pub const kAudioUnitParameterUnit_Boolean: u32 = 2;
pub const kAudioUnitParameterUnit_Percent: u32 = 3;
pub const kAudioUnitParameterUnit_Seconds: u32 = 4;
pub const kAudioUnitParameterUnit_Hertz: u32 = 8;
pub const kAudioUnitParameterUnit_Decibels: u32 = 13;
pub const kAudioUnitParameterUnit_Milliseconds: u32 = 24;
pub const kAudioUnitParameterUnit_Ratio: u32 = 25;
pub const kAudioUnitParameterUnit_CustomUnit: u32 = 26;

pub const kAudioUnitParameterFlag_CFNameRelease: u32 = 1 << 4;
pub const kAudioUnitParameterFlag_ValuesHaveStrings: u32 = 1 << 21;
pub const kAudioUnitParameterFlag_DisplayLogarithmic: u32 = 1 << 22;
pub const kAudioUnitParameterFlag_IsHighResolution: u32 = 1 << 23;
pub const kAudioUnitParameterFlag_NonRealTime: u32 = 1 << 24;
pub const kAudioUnitParameterFlag_CanRamp: u32 = 1 << 25;
pub const kAudioUnitParameterFlag_ExpertMode: u32 = 1 << 26;
pub const kAudioUnitParameterFlag_HasCFNameString: u32 = 1 << 27;
pub const kAudioUnitParameterFlag_IsGlobalMeta: u32 = 1 << 28;
pub const kAudioUnitParameterFlag_IsElementMeta: u32 = 1 << 29;
pub const kAudioUnitParameterFlag_IsReadable: u32 = 1 << 30;
pub const kAudioUnitParameterFlag_IsWritable: u32 = 1 << 31;

/// `kAudioUnitProperty_ParameterStringFromValue`'s payload: `inValue`
/// null asks for the current value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitParameterStringFromValue {
    pub inParamID: AudioUnitParameterID,
    pub inValue: *const AudioUnitParameterValue,
    pub outString: CFStringRef,
}

/// `kAudioUnitProperty_ParameterValueFromString`'s payload.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitParameterValueFromString {
    pub inParamID: AudioUnitParameterID,
    pub inString: CFStringRef,
    pub outValue: AudioUnitParameterValue,
}

/// `kAudioUnitProperty_CocoaUI`'s payload, for one view class.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitCocoaViewInfo {
    pub mCocoaAUViewBundleLocation: CFURLRef,
    pub mCocoaAUViewClass: [CFStringRef; 1],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AUPreset {
    pub presetNumber: i32,
    pub presetName: CFStringRef,
}

pub type HostCallback_GetBeatAndTempo =
    unsafe extern "C" fn(user: *mut c_void, beat: *mut f64, tempo: *mut f64) -> OSStatus;
pub type HostCallback_GetMusicalTimeLocation = unsafe extern "C" fn(
    user: *mut c_void,
    delta_to_next_beat: *mut u32,
    numerator: *mut f32,
    denominator: *mut u32,
    measure_downbeat: *mut f64,
) -> OSStatus;
pub type HostCallback_GetTransportState = unsafe extern "C" fn(
    user: *mut c_void,
    playing: *mut Boolean,
    changed: *mut Boolean,
    sample_in_timeline: *mut f64,
    cycling: *mut Boolean,
    cycle_start_beat: *mut f64,
    cycle_end_beat: *mut f64,
) -> OSStatus;
pub type HostCallback_GetTransportState2 = unsafe extern "C" fn(
    user: *mut c_void,
    playing: *mut Boolean,
    recording: *mut Boolean,
    changed: *mut Boolean,
    sample_in_timeline: *mut f64,
    cycling: *mut Boolean,
    cycle_start_beat: *mut f64,
    cycle_end_beat: *mut f64,
) -> OSStatus;

/// `kAudioUnitProperty_HostCallbacks`' payload (a host may pass a
/// shorter, older struct: only the fields its size covers are there).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HostCallbackInfo {
    pub hostUserData: *mut c_void,
    pub beatAndTempoProc: Option<HostCallback_GetBeatAndTempo>,
    pub musicalTimeLocationProc: Option<HostCallback_GetMusicalTimeLocation>,
    pub transportStateProc: Option<HostCallback_GetTransportState>,
    pub transportStateProc2: Option<HostCallback_GetTransportState2>,
}

/// A property listener (`AudioUnitAddPropertyListener`).
pub type AudioUnitPropertyListenerProc = unsafe extern "C" fn(
    refcon: *mut c_void,
    unit: AudioUnit,
    id: AudioUnitPropertyID,
    scope: AudioUnitScope,
    element: AudioUnitElement,
);

pub const kParameterEvent_Immediate: u32 = 1;
pub const kParameterEvent_Ramped: u32 = 2;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ParameterEventRamp {
    pub startBufferOffset: i32,
    pub durationInFrames: u32,
    pub startValue: f32,
    pub endValue: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ParameterEventImmediate {
    pub bufferOffset: u32,
    pub value: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union ParameterEventValue {
    pub ramp: ParameterEventRamp,
    pub immediate: ParameterEventImmediate,
}

/// One of `AudioUnitScheduleParameters`' events.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitParameterEvent {
    pub scope: AudioUnitScope,
    pub element: AudioUnitElement,
    pub parameter: AudioUnitParameterID,
    pub eventType: u32,
    pub eventValues: ParameterEventValue,
}

// ===========================================================================
// Change notifications (AudioUnitUtilities.h)
// ===========================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AudioUnitParameter {
    pub mAudioUnit: AudioUnit,
    pub mParameterID: AudioUnitParameterID,
    pub mScope: AudioUnitScope,
    pub mElement: AudioUnitElement,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AudioUnitProperty {
    pub mAudioUnit: AudioUnit,
    pub mPropertyID: AudioUnitPropertyID,
    pub mScope: AudioUnitScope,
    pub mElement: AudioUnitElement,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union AudioUnitEventArgument {
    pub mParameter: AudioUnitParameter,
    pub mProperty: AudioUnitProperty,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AudioUnitEvent {
    pub mEventType: u32,
    pub mArgument: AudioUnitEventArgument,
}

pub const kAudioUnitEvent_ParameterValueChange: u32 = 0;
pub const kAudioUnitEvent_BeginParameterChangeGesture: u32 = 1;
pub const kAudioUnitEvent_EndParameterChangeGesture: u32 = 2;
pub const kAudioUnitEvent_PropertyChange: u32 = 3;

/// "Every parameter" in a parameter notification: hosts re-read them all.
pub const kAUParameterListener_AnyParameter: AudioUnitParameterID = 0xFFFF_FFFF;

// ===========================================================================
// CoreFoundation constants
// ===========================================================================

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kCFNumberSInt32Type: CFIndex = 3;
pub const kCFNumberFloat64Type: CFIndex = 6;
pub const kCFPropertyListXMLFormat_v1_0: CFIndex = 100;
pub const kCFPropertyListBinaryFormat_v1_0: CFIndex = 200;

/// `CFDictionaryKeyCallBacks` / `CFDictionaryValueCallBacks` /
/// `CFArrayCallBacks`: opaque here, only their addresses are passed.
#[repr(C)]
pub struct CFCallBacks {
    _opaque: [u8; 0],
}

// ===========================================================================
// Declarations (macOS)
// ===========================================================================

#[cfg(target_os = "macos")]
#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    pub fn AudioComponentFindNext(component: AudioComponent, desc: *const AudioComponentDescription) -> AudioComponent;
    pub fn AudioComponentCopyName(component: AudioComponent, name: *mut CFStringRef) -> OSStatus;
    pub fn AudioComponentGetDescription(component: AudioComponent, desc: *mut AudioComponentDescription) -> OSStatus;
    pub fn AudioComponentRegister(
        desc: *const AudioComponentDescription,
        name: CFStringRef,
        version: u32,
        factory: AudioComponentFactoryFunction,
    ) -> AudioComponent;
    pub fn AudioComponentInstanceNew(component: AudioComponent, instance: *mut AudioUnit) -> OSStatus;
    pub fn AudioComponentInstanceDispose(instance: AudioUnit) -> OSStatus;
    pub fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    pub fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    pub fn AudioUnitReset(unit: AudioUnit, scope: AudioUnitScope, element: AudioUnitElement) -> OSStatus;
    pub fn AudioUnitGetPropertyInfo(
        unit: AudioUnit,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        size: *mut u32,
        writable: *mut Boolean,
    ) -> OSStatus;
    pub fn AudioUnitGetProperty(
        unit: AudioUnit,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        data: *mut c_void,
        size: *mut u32,
    ) -> OSStatus;
    pub fn AudioUnitSetProperty(
        unit: AudioUnit,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        data: *const c_void,
        size: u32,
    ) -> OSStatus;
    pub fn AudioUnitGetParameter(
        unit: AudioUnit,
        id: AudioUnitParameterID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        value: *mut f32,
    ) -> OSStatus;
    pub fn AudioUnitSetParameter(
        unit: AudioUnit,
        id: AudioUnitParameterID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        value: f32,
        offset: u32,
    ) -> OSStatus;
    pub fn AudioUnitRender(
        unit: AudioUnit,
        action_flags: *mut u32,
        time: *const AudioTimeStamp,
        bus: u32,
        frames: u32,
        data: *mut AudioBufferList,
    ) -> OSStatus;
    pub fn MusicDeviceMIDIEvent(unit: AudioUnit, status: u32, data1: u32, data2: u32, offset: u32) -> OSStatus;
    pub fn AUEventListenerNotify(listener: *mut c_void, object: *mut c_void, event: *const AudioUnitEvent) -> OSStatus;
    pub fn AUParameterListenerNotify(
        listener: *mut c_void,
        object: *mut c_void,
        parameter: *const AudioUnitParameter,
    ) -> OSStatus;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFTypeDictionaryKeyCallBacks: CFCallBacks;
    pub static kCFTypeDictionaryValueCallBacks: CFCallBacks;
    pub static kCFTypeArrayCallBacks: CFCallBacks;
    pub static kCFRunLoopDefaultMode: CFStringRef;

    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFNumberGetTypeID() -> CFTypeID;
    pub fn CFDataGetTypeID() -> CFTypeID;
    pub fn CFDictionaryGetTypeID() -> CFTypeID;
    pub fn CFStringCreateWithBytes(
        alloc: CFAllocatorRef,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        external: Boolean,
    ) -> CFStringRef;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFStringGetCString(s: CFStringRef, buffer: *mut c_char, size: CFIndex, encoding: u32) -> Boolean;
    pub fn CFNumberCreate(alloc: CFAllocatorRef, kind: CFIndex, value: *const c_void) -> CFNumberRef;
    pub fn CFNumberGetValue(number: CFNumberRef, kind: CFIndex, value: *mut c_void) -> Boolean;
    pub fn CFDataCreate(alloc: CFAllocatorRef, bytes: *const u8, len: CFIndex) -> CFDataRef;
    pub fn CFDataGetLength(data: CFDataRef) -> CFIndex;
    pub fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    pub fn CFDictionaryCreateMutable(
        alloc: CFAllocatorRef,
        capacity: CFIndex,
        keys: *const CFCallBacks,
        values: *const CFCallBacks,
    ) -> CFMutableDictionaryRef;
    pub fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: *const c_void, value: *const c_void);
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    pub fn CFArrayCreate(
        alloc: CFAllocatorRef,
        values: *const *const c_void,
        count: CFIndex,
        callbacks: *const CFCallBacks,
    ) -> CFArrayRef;
    pub fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
    pub fn CFURLCopyFileSystemPath(url: CFURLRef, style: CFIndex) -> CFStringRef;
    pub fn CFURLCreateFromFileSystemRepresentation(
        alloc: CFAllocatorRef,
        path: *const u8,
        len: CFIndex,
        directory: Boolean,
    ) -> CFURLRef;
    pub fn CFPropertyListCreateData(
        alloc: CFAllocatorRef,
        list: CFPropertyListRef,
        format: CFIndex,
        options: usize,
        error: *mut *const c_void,
    ) -> CFDataRef;
    pub fn CFPropertyListCreateWithData(
        alloc: CFAllocatorRef,
        data: CFDataRef,
        options: usize,
        format: *mut CFIndex,
        error: *mut *const c_void,
    ) -> CFPropertyListRef;
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: Boolean) -> i32;
}

// ===========================================================================
// Layout assertions (64-bit: every Mac)
// ===========================================================================

#[cfg(target_pointer_width = "64")]
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    macro_rules! layout {
        ($($t:ty => $size:expr, $align:expr;)*) => {$(
            assert!(size_of::<$t>() == $size, concat!("size of ", stringify!($t)));
            assert!(align_of::<$t>() == $align, concat!("align of ", stringify!($t)));
        )*};
    }
    layout! {
        AudioComponentDescription => 20, 4;
        AudioComponentPlugInInterface => 32, 8;
        AudioStreamBasicDescription => 40, 8;
        AuBuffer => 16, 8;
        AudioBufferList => 24, 8;
        SMPTETime => 24, 4;
        AudioTimeStamp => 64, 8;
        AURenderCallbackStruct => 16, 8;
        AudioUnitConnection => 16, 8;
        AUChannelInfo => 4, 2;
        AudioUnitParameterInfo => 104, 8;
        AudioUnitParameterStringFromValue => 24, 8;
        AudioUnitParameterValueFromString => 24, 8;
        AudioUnitCocoaViewInfo => 16, 8;
        AUPreset => 16, 8;
        HostCallbackInfo => 40, 8;
        AudioUnitParameterEvent => 32, 4;
        AudioUnitParameter => 24, 8;
        AudioUnitProperty => 24, 8;
        AudioUnitEvent => 32, 8;
    }
    assert!(offset_of!(AudioBufferList, mBuffers) == 8);
    assert!(offset_of!(AudioUnitEvent, mArgument) == 8);
    assert!(offset_of!(AudioUnitParameterEvent, eventValues) == 16);
    assert!(fourcc(b"aufx") == 0x6175_6678);
};
