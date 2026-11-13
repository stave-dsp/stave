//! The slice of AudioToolbox and CoreFoundation an offline Audio Unit
//! (v2) host needs, as plain C declarations. Names and values follow
//! Apple's headers (`AudioUnit/AUComponent.h`, `AudioUnitProperties.h`,
//! `CoreAudioTypes.h`); nothing here is called outside `au`.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use core::ffi::{c_char, c_void};

pub type OSStatus = i32;
pub type AudioComponent = *mut c_void;
pub type AudioUnit = *mut c_void;
pub type CFStringRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioComponentDescription {
    pub componentType: u32,
    pub componentSubType: u32,
    pub componentManufacturer: u32,
    pub componentFlags: u32,
    pub componentFlagsMask: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
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

/// One channel of a buffer list (CoreAudioTypes.h).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AuBuffer {
    pub mNumberChannels: u32,
    pub mDataByteSize: u32,
    pub mData: *mut c_void,
}

/// `AudioBufferList` with room for [`MAX_CHANNELS`] buffers (the C type
/// is a one-element array the caller over-allocates; this is the same
/// layout with the room spelled out).
#[repr(C)]
pub struct AudioBufferList {
    pub mNumberBuffers: u32,
    pub mBuffers: [AuBuffer; MAX_CHANNELS],
}

/// The most channels a hosted bus carries here.
pub const MAX_CHANNELS: usize = 8;

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

pub type AURenderCallback = unsafe extern "C" fn(
    refcon: *mut c_void,
    action_flags: *mut u32,
    time: *const AudioTimeStamp,
    bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> OSStatus;

#[repr(C)]
pub struct AURenderCallbackStruct {
    pub inputProc: AURenderCallback,
    pub inputProcRefCon: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
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

/// A four-character code.
pub const fn fourcc(s: &[u8; 4]) -> u32 {
    (s[0] as u32) << 24 | (s[1] as u32) << 16 | (s[2] as u32) << 8 | s[3] as u32
}

pub const kAudioUnitType_Effect: u32 = fourcc(b"aufx");
pub const kAudioUnitType_MusicEffect: u32 = fourcc(b"aumf");
pub const kAudioUnitType_MusicDevice: u32 = fourcc(b"aumu");
pub const kAudioFormatLinearPCM: u32 = fourcc(b"lpcm");
pub const kAudioFormatFlagIsFloat: u32 = 1;
pub const kAudioFormatFlagIsPacked: u32 = 1 << 3;
pub const kAudioFormatFlagIsNonInterleaved: u32 = 1 << 5;

pub const kAudioUnitScope_Global: u32 = 0;
pub const kAudioUnitScope_Input: u32 = 1;
pub const kAudioUnitScope_Output: u32 = 2;

pub const kAudioUnitProperty_ClassInfo: u32 = 0;
pub const kAudioUnitProperty_ParameterList: u32 = 3;
pub const kAudioUnitProperty_ParameterInfo: u32 = 4;
pub const kAudioUnitProperty_StreamFormat: u32 = 8;
pub const kAudioUnitProperty_Latency: u32 = 12;
pub const kAudioUnitProperty_SupportedNumChannels: u32 = 13;
pub const kAudioUnitProperty_MaximumFramesPerSlice: u32 = 14;
pub const kAudioUnitProperty_ParameterValueStrings: u32 = 16;
pub const kAudioUnitProperty_TailTime: u32 = 20;
pub const kAudioUnitProperty_BypassEffect: u32 = 21;
pub const kAudioUnitProperty_SetRenderCallback: u32 = 23;
pub const kAudioUnitProperty_OfflineRender: u32 = 37;

pub const kAudioUnitParameterUnit_Indexed: u32 = 1;
pub const kAudioUnitParameterUnit_Boolean: u32 = 2;
pub const kAudioUnitParameterUnit_CustomUnit: u32 = 26;

pub const kAudioUnitParameterFlag_CFNameRelease: u32 = 1 << 4;
pub const kAudioUnitParameterFlag_ValuesHaveStrings: u32 = 1 << 21;
pub const kAudioUnitParameterFlag_HasCFNameString: u32 = 1 << 27;
pub const kAudioUnitParameterFlag_IsReadable: u32 = 1 << 30;
pub const kAudioUnitParameterFlag_IsWritable: u32 = 1 << 31;

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    pub fn AudioComponentFindNext(
        component: AudioComponent,
        desc: *const AudioComponentDescription,
    ) -> AudioComponent;
    pub fn AudioComponentCopyName(component: AudioComponent, name: *mut CFStringRef) -> OSStatus;
    pub fn AudioComponentGetDescription(
        component: AudioComponent,
        desc: *mut AudioComponentDescription,
    ) -> OSStatus;
    pub fn AudioComponentInstanceNew(component: AudioComponent, instance: *mut AudioUnit) -> OSStatus;
    pub fn AudioComponentInstanceDispose(instance: AudioUnit) -> OSStatus;
    pub fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    pub fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    pub fn AudioUnitReset(unit: AudioUnit, scope: u32, element: u32) -> OSStatus;
    pub fn AudioUnitGetPropertyInfo(
        unit: AudioUnit,
        id: u32,
        scope: u32,
        element: u32,
        size: *mut u32,
        writable: *mut u8,
    ) -> OSStatus;
    pub fn AudioUnitGetProperty(
        unit: AudioUnit,
        id: u32,
        scope: u32,
        element: u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> OSStatus;
    pub fn AudioUnitSetProperty(
        unit: AudioUnit,
        id: u32,
        scope: u32,
        element: u32,
        data: *const c_void,
        size: u32,
    ) -> OSStatus;
    pub fn AudioUnitGetParameter(unit: AudioUnit, id: u32, scope: u32, element: u32, value: *mut f32) -> OSStatus;
    pub fn AudioUnitSetParameter(
        unit: AudioUnit,
        id: u32,
        scope: u32,
        element: u32,
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
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFStringGetLength(s: CFStringRef) -> isize;
    pub fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    pub fn CFStringGetCString(s: CFStringRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    pub fn CFArrayGetCount(a: CFArrayRef) -> isize;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, i: isize) -> *const c_void;
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: u8) -> i32;
    pub static kCFRunLoopDefaultMode: CFStringRef;
}

/// `s` as UTF-8 (empty for null).
///
/// # Safety
/// `s` is null or a live `CFString`.
pub unsafe fn string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: `s` is a live CFString (the caller's contract); the buffer
    // is sized by CF's own maximum for the encoding, plus the NUL.
    unsafe {
        let n = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0 as c_char; n.max(1) as usize];
        if CFStringGetCString(s, buf.as_mut_ptr(), n, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

/// A unit's label for an AudioUnit parameter unit (`custom` for
/// `kAudioUnitParameterUnit_CustomUnit`).
pub fn unit_label(unit: u32, custom: &str) -> String {
    match unit {
        3 => "%",
        4 => "s",
        5 => "samples",
        6 | 15 => "°",
        8 => "Hz",
        9 | 20 => "cents",
        10 => "semitones",
        13 => "dB",
        21 => "octaves",
        22 => "BPM",
        23 => "beats",
        24 => "ms",
        25 => "ratio",
        u if u == kAudioUnitParameterUnit_CustomUnit => return custom.to_string(),
        _ => "",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The structs match the C headers' sizes (arm64 and x86_64 alike).
    #[test]
    fn layouts_match_the_headers() {
        assert_eq!(std::mem::size_of::<AudioStreamBasicDescription>(), 40);
        assert_eq!(std::mem::size_of::<AudioTimeStamp>(), 64);
        assert_eq!(std::mem::size_of::<AuBuffer>(), 16);
        assert_eq!(std::mem::offset_of!(AudioBufferList, mBuffers), 8);
        assert_eq!(std::mem::size_of::<AudioUnitParameterInfo>(), 104);
        assert_eq!(std::mem::size_of::<AudioComponentDescription>(), 20);
        assert_eq!(fourcc(b"aufx"), 0x6175_6678);
    }
}
