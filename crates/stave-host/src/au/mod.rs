//! Audio Unit (v2) hosting on macOS, for offline measurement: find a
//! component by name, open it at the host's rate and block size, and
//! drive it as an [`Instance`], so the bench,
//! the response sweep, the validation suite, the voicing tools and the
//! characterisation (DSP.md R17–R19) measure an Audio Unit exactly as they
//! measure a VST3 or a stave stage.
//!
//! A tool opens an Audio Unit as `au:<name>` (`au:<manufacturer>:<name>`
//! when two vendors share a name) or by its `.component` bundle path:
//!
//! ```text
//! au:AUBandpass
//! au:Apple:AUDelay
//! /Library/Audio/Plug-Ins/Components/Some Effect.component
//! ```
//!
//! What it is for: a third-party plugin as a *benchmark* and a listening
//! contender. It is not a reference — circuits are held to ngspice and
//! datasheets — and its output is the vendor's, kept out of the
//! repository. A plugin that needs a licence or DSP hardware it cannot
//! find may open and output silence or its input: the host reports what it got and
//! does nothing to get around it.
//!
//! ```text
//! ffi.rs       AudioToolbox / CoreFoundation declarations
//! mod.rs       finding components; AuInstance: open, process, params
//! ```

mod ffi;

use std::ffi::c_void;
use std::ptr;

use stave_core::Event;

use crate::config::HostConfig;
use crate::instance::{Instance, Layout, Tail};
use crate::param::{Param, ParamError, ParamFlags, ParamId};

use ffi::*;

/// What `au:` names.
pub const PREFIX: &str = "au:";

/// An installed Audio Unit effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub manufacturer: String,
    pub name: String,
    /// `type subtype manufacturer` as four-character codes.
    pub codes: [String; 3],
    desc: AudioComponentDescription,
    handle: usize,
}

impl Component {
    /// "Manufacturer: Name".
    pub fn full_name(&self) -> String {
        format!("{}: {}", self.manufacturer, self.name)
    }
}

/// Why an Audio Unit would not open or run.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AuError {
    #[error("no Audio Unit named '{query}'{}", near(similar))]
    NotFound { query: String, similar: Vec<String> },
    #[error("'{query}' names {} Audio Units: {}; add the manufacturer (au:<manufacturer>:<name>)", matches.len(), matches.join(", "))]
    Ambiguous { query: String, matches: Vec<String> },
    #[error("{call} failed: OSStatus {status} ({})", fourcc_text(*status))]
    Status { call: &'static str, status: i32 },
    #[error("the Audio Unit accepts neither mono nor stereo float audio at {rate} Hz")]
    NoFormat { rate: f64 },
}

fn near(similar: &[String]) -> String {
    if similar.is_empty() {
        String::new()
    } else {
        format!("; similar: {}", similar.join(", "))
    }
}

/// An OSStatus that is a four-character code, as text ("'!dat'").
fn fourcc_text(status: i32) -> String {
    let b = status.to_be_bytes();
    if b.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
        format!("'{}'", String::from_utf8_lossy(&b))
    } else {
        "see MacErrors.h / AUComponent.h".into()
    }
}

fn code(c: u32) -> String {
    String::from_utf8_lossy(&c.to_be_bytes()).into_owned()
}

fn check(call: &'static str, status: OSStatus) -> Result<(), AuError> {
    if status == 0 {
        Ok(())
    } else {
        Err(AuError::Status { call, status })
    }
}

/// Every installed effect Audio Unit (`aufx`, `aumf`), sorted by name.
pub fn components() -> Vec<Component> {
    let mut out = Vec::new();
    for kind in [kAudioUnitType_Effect, kAudioUnitType_MusicEffect] {
        let wanted = AudioComponentDescription {
            componentType: kind,
            ..Default::default()
        };
        let mut c: AudioComponent = ptr::null_mut();
        loop {
            // SAFETY: `c` is null or a component the previous call
            // returned; `wanted` outlives the call.
            c = unsafe { AudioComponentFindNext(c, &wanted) };
            if c.is_null() {
                break;
            }
            let mut name: CFStringRef = ptr::null();
            let mut desc = AudioComponentDescription::default();
            // SAFETY: `c` is a live component; both out-pointers are
            // valid; the copied name is ours to release.
            let full = unsafe {
                if AudioComponentGetDescription(c, &mut desc) != 0 || AudioComponentCopyName(c, &mut name) != 0 {
                    continue;
                }
                let s = string(name);
                if !name.is_null() {
                    CFRelease(name);
                }
                s
            };
            let (manufacturer, name) = full.split_once(": ").unwrap_or(("", &full));
            out.push(Component {
                manufacturer: manufacturer.to_string(),
                name: name.to_string(),
                codes: [code(desc.componentType), code(desc.componentSubType), code(desc.componentManufacturer)],
                desc,
                handle: c as usize,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.manufacturer.cmp(&b.manufacturer)));
    out
}

/// Find one component by `query`: an exact name
/// (case-insensitive), optionally `manufacturer:name`.
pub fn find(query: &str) -> Result<Component, AuError> {
    find_in(components(), query)
}

fn find_in(all: Vec<Component>, query: &str) -> Result<Component, AuError> {
    let eq = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let mut matches: Vec<&Component> = all.iter().filter(|c| eq(&c.name, query) || eq(&c.full_name(), query)).collect();
    if matches.is_empty() {
        if let Some((maker, name)) = query.split_once(':') {
            matches = all
                .iter()
                .filter(|c| eq(&c.name, name) && (eq(&c.manufacturer, maker) || eq(&c.codes[2], maker)))
                .collect();
        }
    }
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => {
            let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
            let mut similar: Vec<String> = all
                .iter()
                .filter(|c| {
                    let n = c.name.to_lowercase();
                    words.iter().filter(|w| w.len() > 2).any(|w| n.contains(w.as_str()))
                })
                .map(Component::full_name)
                .collect();
            similar.truncate(12);
            Err(AuError::NotFound {
                query: query.into(),
                similar,
            })
        }
        many => Err(AuError::Ambiguous {
            query: query.into(),
            matches: many.iter().map(|c| c.full_name()).collect(),
        }),
    }
}

/// What the input callback reads: the block's input, one buffer per
/// channel. Boxed, so its address is stable for the callback's refcon.
struct Feed {
    channels: Vec<Vec<f32>>,
    frames: usize,
}

/// The input callback (`kAudioUnitProperty_SetRenderCallback`): hand the unit the current block's input.
///
/// # Safety
/// `refcon` is the `Feed` [`AuInstance`] registered, alive for the
/// unit's life; `data` is the unit's buffer list for `frames` frames,
/// holding `mNumberBuffers` buffers (the C type is over-allocated from a
/// one-element array, so the list may be smaller than [`AudioBufferList`]).
///
/// A panic here would abort the host through `extern "C"`: it is caught
/// and answered with an error status instead.
unsafe extern "C" fn feed(
    refcon: *mut c_void,
    _flags: *mut u32,
    _time: *const AudioTimeStamp,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> OSStatus {
    // `kAudio_ParamError`: the unit's input pull fails; the host lives.
    const PARAM_ERROR: OSStatus = -50;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if refcon.is_null() || data.is_null() {
            return PARAM_ERROR;
        }
        // SAFETY: the function's contract.
        let feed = unsafe { &mut *refcon.cast::<Feed>() };
        // Only the buffers the unit allocated: never a reference to the
        // whole `[AuBuffer; MAX_CHANNELS]`, which may run past the list.
        // SAFETY: `data` points at a live list (the contract); the count
        // is its first field, and `mBuffers` holds that many buffers.
        let buffers = unsafe {
            let count = (ptr::addr_of!((*data).mNumberBuffers).read() as usize).min(MAX_CHANNELS);
            let first = ptr::addr_of_mut!((*data).mBuffers).cast::<AuBuffer>();
            std::slice::from_raw_parts_mut(first, count)
        };
        let frames = (frames as usize).min(feed.frames);
        for (i, b) in buffers.iter_mut().enumerate() {
            let Some(src) = feed.channels.len().checked_sub(1).map(|last| &mut feed.channels[i.min(last)]) else {
                return PARAM_ERROR; // no input channels to feed from
            };
            const F32: usize = std::mem::size_of::<f32>();
            let frames = frames.min(src.len());
            let n = if b.mData.is_null() {
                b.mData = src.as_mut_ptr().cast();
                frames
            } else {
                // The unit's buffer holds `mDataByteSize` bytes: never
                // write past that, whatever `frames` says.
                let n = frames.min(b.mDataByteSize as usize / F32);
                // SAFETY: `mData` is writable for `mDataByteSize` bytes,
                // ≥ `n` floats; `src` holds ≥ `n` floats.
                unsafe { ptr::copy_nonoverlapping(src.as_ptr(), b.mData.cast::<f32>(), n) };
                n
            };
            // `n` ≤ `max_block` floats, far below 4 GiB.
            b.mDataByteSize = (n * F32) as u32;
        }
        0
    }))
    .unwrap_or(PARAM_ERROR)
}

#[cfg(test)]
mod feed_tests;

/// An Audio Unit, initialised and processing offline (see the module).
pub struct AuInstance {
    unit: AudioUnit,
    component: Component,
    layout: Layout,
    feed: Box<Feed>,
    out: Vec<Vec<f32>>,
    sample_time: f64,
    /// Every param's id and static info, read once at open.
    infos: Vec<(u32, Param)>,
}

// SAFETY: an AU v2 instance has no thread affinity for processing and
// parameter calls; `AuInstance` is used by one thread at a time (`&mut`
// for every call that processes or writes).
unsafe impl Send for AuInstance {}

impl AuInstance {
    /// Open the Audio Unit `query` names at `config`'s
    /// rate and block size, mono in and out if it takes that, else
    /// stereo. `query` is a name, "manufacturer:name" or
    /// "Manufacturer: Name" (see [`crate::opened::au_query`]).
    pub fn open(query: &str, config: HostConfig) -> Result<Self, AuError> {
        Self::open_component(find(query)?, config)
    }

    pub fn open_component(component: Component, config: HostConfig) -> Result<Self, AuError> {
        let mut unit: AudioUnit = ptr::null_mut();
        // SAFETY: `handle` is a component `components()` found (components
        // live as long as the process); `unit` is a valid out-pointer.
        check("AudioComponentInstanceNew", unsafe {
            AudioComponentInstanceNew(component.handle as AudioComponent, &mut unit)
        })?;
        let rate = config.sample_rate.hz();
        let max_block = config.max_block.get();
        let mut this = Self {
            unit,
            component,
            layout: Layout {
                inputs: 0,
                outputs: 0,
                sample_rate: rate,
                max_block,
            },
            feed: Box::new(Feed {
                channels: Vec::new(),
                frames: 0,
            }),
            out: Vec::new(),
            sample_time: 0.0,
            infos: Vec::new(),
        };
        let channels = this.configure_format(rate)?;
        this.layout.inputs = channels;
        this.layout.outputs = channels;
        this.feed.channels = vec![vec![0.0; max_block]; channels];
        this.out = vec![vec![0.0; max_block]; channels];
        let frames = max_block as u32;
        this.set(kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, &frames, "MaximumFramesPerSlice")?;
        let offline: u32 = 1;
        // Offline processing is a hint; a unit that has no such mode is fine.
        let _ = this.set(kAudioUnitProperty_OfflineRender, kAudioUnitScope_Global, &offline, "OfflineRender");
        let callback = AURenderCallbackStruct {
            inputProc: feed,
            inputProcRefCon: (&mut *this.feed as *mut Feed).cast(),
        };
        this.set(kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, &callback, "SetRenderCallback")?;
        // SAFETY: a live, configured unit.
        check("AudioUnitInitialize", unsafe { AudioUnitInitialize(this.unit) })?;
        this.infos = this.read_params();
        Ok(this)
    }

    pub fn component(&self) -> &Component {
        &self.component
    }

    /// Clear the unit's state (delay lines, envelopes) without
    /// re-opening it.
    pub fn reset(&mut self) {
        // SAFETY: a live unit.
        unsafe { AudioUnitReset(self.unit, kAudioUnitScope_Global, 0) };
    }

    /// Mono if the unit takes it, else stereo: the channel count set on
    /// both buses.
    fn configure_format(&mut self, rate: f64) -> Result<usize, AuError> {
        for channels in [1u32, 2] {
            let f = AudioStreamBasicDescription {
                mSampleRate: rate,
                mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked | kAudioFormatFlagIsNonInterleaved,
                mBytesPerPacket: 4,
                mFramesPerPacket: 1,
                mBytesPerFrame: 4,
                mChannelsPerFrame: channels,
                mBitsPerChannel: 32,
                mReserved: 0,
            };
            let ok = self.set(kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, &f, "StreamFormat").is_ok()
                && self.set(kAudioUnitProperty_StreamFormat, kAudioUnitScope_Output, &f, "StreamFormat").is_ok();
            if ok {
                return Ok(channels as usize);
            }
        }
        Err(AuError::NoFormat { rate })
    }

    fn set<T>(&self, id: u32, scope: u32, value: &T, call: &'static str) -> Result<(), AuError> {
        // SAFETY: `value` is a `T` of `size_of::<T>()` bytes, the type
        // the property `id` takes (each call site passes the header's).
        check(call, unsafe {
            AudioUnitSetProperty(self.unit, id, scope, 0, (value as *const T).cast(), std::mem::size_of::<T>() as u32)
        })
    }

    fn get_f64(&self, id: u32) -> Option<f64> {
        let mut v = 0.0f64;
        let mut size = std::mem::size_of::<f64>() as u32;
        // SAFETY: an f64 out-pointer of the stated size.
        let s = unsafe { AudioUnitGetProperty(self.unit, id, kAudioUnitScope_Global, 0, (&mut v as *mut f64).cast(), &mut size) };
        (s == 0).then_some(v)
    }

    /// Every parameter's static info (global scope).
    fn read_params(&self) -> Vec<(u32, Param)> {
        let mut size = 0u32;
        // SAFETY: size query on a live unit.
        let s = unsafe {
            AudioUnitGetPropertyInfo(self.unit, kAudioUnitProperty_ParameterList, kAudioUnitScope_Global, 0, &mut size, ptr::null_mut())
        };
        if s != 0 || size == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / 4];
        // SAFETY: `ids` holds `size` bytes.
        let s = unsafe {
            AudioUnitGetProperty(self.unit, kAudioUnitProperty_ParameterList, kAudioUnitScope_Global, 0, ids.as_mut_ptr().cast(), &mut size)
        };
        if s != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / 4);
        ids.into_iter().filter_map(|id| self.param_info(id).map(|p| (id, p))).collect()
    }

    fn param_info(&self, id: u32) -> Option<Param> {
        // SAFETY: zeroed is a valid AudioUnitParameterInfo (plain data,
        // null CF references).
        let mut info: AudioUnitParameterInfo = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<AudioUnitParameterInfo>() as u32;
        // SAFETY: `info` is the property's type and size; the CF strings
        // it returns are read, then released when the unit says so.
        let (name, custom) = unsafe {
            if AudioUnitGetProperty(self.unit, kAudioUnitProperty_ParameterInfo, kAudioUnitScope_Global, id, (&mut info as *mut AudioUnitParameterInfo).cast(), &mut size) != 0 {
                return None;
            }
            let name = if info.flags & kAudioUnitParameterFlag_HasCFNameString != 0 && !info.cfNameString.is_null() {
                let n = string(info.cfNameString);
                if info.flags & kAudioUnitParameterFlag_CFNameRelease != 0 {
                    CFRelease(info.cfNameString);
                }
                n
            } else {
                std::ffi::CStr::from_ptr(info.name.as_ptr()).to_string_lossy().into_owned()
            };
            let custom = if info.unit == kAudioUnitParameterUnit_CustomUnit { string(info.unitName) } else { String::new() };
            (name, custom)
        };
        let (min, max) = (f64::from(info.minValue), f64::from(info.maxValue));
        let step_names = if info.flags & kAudioUnitParameterFlag_ValuesHaveStrings != 0 { self.value_strings(id) } else { Vec::new() };
        let step_count = match info.unit {
            u if u == kAudioUnitParameterUnit_Boolean => 2,
            u if u == kAudioUnitParameterUnit_Indexed => (max - min).round().max(0.0) as u32 + 1,
            _ => 0,
        };
        Some(Param {
            id: ParamId(id),
            name: name.trim().to_string(),
            group: String::new(),
            unit: unit_label(info.unit, &custom),
            min,
            max,
            default: f64::from(info.defaultValue),
            value: f64::from(info.defaultValue),
            normalized: 0.0,
            step_count,
            step_names,
            flags: ParamFlags::default(),
            read_only: info.flags & kAudioUnitParameterFlag_IsWritable == 0,
            program: false,
        })
    }

    fn value_strings(&self, id: u32) -> Vec<String> {
        let mut array: CFArrayRef = ptr::null();
        let mut size = std::mem::size_of::<CFArrayRef>() as u32;
        // SAFETY: the property is a CFArrayRef of CFStrings we own.
        unsafe {
            if AudioUnitGetProperty(self.unit, kAudioUnitProperty_ParameterValueStrings, kAudioUnitScope_Global, id, (&mut array as *mut CFArrayRef).cast(), &mut size) != 0
                || array.is_null()
            {
                return Vec::new();
            }
            let names = (0..CFArrayGetCount(array)).map(|i| string(CFArrayGetValueAtIndex(array, i))).collect();
            CFRelease(array);
            names
        }
    }

    fn value(&self, id: u32) -> f64 {
        let mut v = 0.0f32;
        // SAFETY: an f32 out-pointer.
        unsafe { AudioUnitGetParameter(self.unit, id, kAudioUnitScope_Global, 0, &mut v) };
        f64::from(v)
    }
}

impl Drop for AuInstance {
    fn drop(&mut self) {
        // SAFETY: the unit this instance opened, disposed once; the feed
        // it calls back into outlives it (dropped after this body).
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

impl Instance for AuInstance {
    fn layout(&self) -> Layout {
        self.layout
    }

    fn latency(&self) -> u32 {
        self.get_f64(kAudioUnitProperty_Latency).map_or(0, |s| (s * self.layout.sample_rate).round().max(0.0) as u32)
    }

    fn tail(&self) -> Tail {
        match self.get_f64(kAudioUnitProperty_TailTime) {
            Some(s) if (0.0..3600.0).contains(&s) => Tail::Samples((s * self.layout.sample_rate).ceil() as u32),
            Some(_) => Tail::Infinite,
            None => Tail::Samples(0),
        }
    }

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _events: &[Event]) {
        let n = outputs.first().map_or(0, |o| o.len()).min(self.layout.max_block);
        if n == 0 {
            return;
        }
        for (c, ch) in self.feed.channels.iter_mut().enumerate() {
            match inputs.get(c).or(inputs.first()) {
                Some(src) => ch[..n].copy_from_slice(&src[..n]),
                None => ch[..n].fill(0.0),
            }
        }
        self.feed.frames = n;
        let mut list = AudioBufferList {
            mNumberBuffers: self.out.len() as u32,
            mBuffers: [AuBuffer {
                mNumberChannels: 1,
                mDataByteSize: 0,
                mData: ptr::null_mut(),
            }; MAX_CHANNELS],
        };
        for (b, o) in list.mBuffers.iter_mut().zip(&mut self.out) {
            b.mDataByteSize = (n * 4) as u32;
            b.mData = o.as_mut_ptr().cast();
        }
        let time = AudioTimeStamp {
            mSampleTime: self.sample_time,
            mFlags: kAudioTimeStampSampleTimeValid,
            ..Default::default()
        };
        let mut flags = 0u32;
        // SAFETY: a live, initialised unit; `list` points at `n` floats
        // per channel; the feed the callback reads holds `n` frames.
        let status = unsafe { AudioUnitRender(self.unit, &mut flags, &time, 0, n as u32, &mut list) };
        self.sample_time += n as f64;
        for (c, out) in outputs.iter_mut().enumerate() {
            let b = &list.mBuffers[c.min(self.out.len() - 1)];
            if status != 0 || b.mData.is_null() {
                out[..n].fill(0.0);
            } else {
                // SAFETY: after a successful call each buffer holds `n`
                // floats (ours, or the unit's own for this call).
                let src = unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), n) };
                out[..n].copy_from_slice(src);
            }
        }
    }

    fn params(&self) -> Vec<Param> {
        self.infos
            .iter()
            .map(|(id, p)| {
                let value = self.value(*id);
                let span = p.max - p.min;
                Param {
                    value,
                    normalized: if span > 0.0 { ((value - p.min) / span).clamp(0.0, 1.0) } else { 0.0 },
                    ..p.clone()
                }
            })
            .collect()
    }

    fn set_param(&mut self, name: &str, value: f64) -> Result<(), ParamError> {
        let params = self.params();
        let p = crate::param::find(&params, name).ok_or_else(|| ParamError::NotFound(name.to_string()))?;
        if p.read_only {
            return Err(ParamError::ReadOnly(p.name.clone()));
        }
        if !(p.min..=p.max).contains(&value) {
            return Err(ParamError::OutOfRange {
                name: p.name.clone(),
                min: p.min,
                max: p.max,
            });
        }
        // SAFETY: a live unit; a parameter id it listed.
        unsafe { AudioUnitSetParameter(self.unit, p.id.0, kAudioUnitScope_Global, 0, value as f32, 0) };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn c(maker: &str, name: &str) -> Component {
        Component {
            manufacturer: maker.into(),
            name: name.into(),
            codes: ["aufx".into(), "test".into(), maker.chars().take(4).collect()],
            desc: AudioComponentDescription::default(),
            handle: 0,
        }
    }

    #[test]
    fn finds_by_name_or_manufacturer_and_name() {
        let all = || vec![c("Apple", "AUDelay"), c("Acme", "Delay"), c("Other", "Delay"), c("Acme", "Delay SE")];
        assert_eq!(find_in(all(), "audelay").unwrap().manufacturer, "Apple");
        assert_eq!(find_in(all(), "Acme:Delay").unwrap().manufacturer, "Acme");
        assert_eq!(find_in(all(), "Other: Delay").unwrap().manufacturer, "Other");
        assert!(matches!(find_in(all(), "Delay"), Err(AuError::Ambiguous { matches, .. }) if matches.len() == 2));
        let Err(AuError::NotFound { similar, .. }) = find_in(all(), "Delays") else { panic!() };
        assert!(similar.is_empty());
        let Err(AuError::NotFound { similar, .. }) = find_in(all(), "big delay") else { panic!() };
        assert_eq!(similar.len(), 4);
    }

    #[test]
    fn paths_name_audio_units() {
        use crate::opened::au_query;
        assert_eq!(au_query(Path::new("au:Apple:AUDelay")).as_deref(), Some("Apple:AUDelay"));
        assert_eq!(au_query(Path::new("/x/Some Effect.component")).as_deref(), Some("Some Effect"));
        assert_eq!(au_query(Path::new("/x/A.vst3")), None);
    }

    /// Apple's own AUDelay ships with every macOS: open it, read its
    /// params, set one, process an impulse and see it come back delayed.
    #[test]
    fn hosts_apples_delay() {
        let config = HostConfig {
            max_block: std::num::NonZeroUsize::new(64).unwrap(),
            ..HostConfig::default()
        };
        let mut d = AuInstance::open("Apple:AUDelay", config).unwrap();
        let params = d.params();
        assert!(params.iter().any(|p| p.name.to_lowercase().contains("delay")), "{params:?}");
        let dry = crate::param::find(&params, "dry/wet mix").map(|p| p.name.clone());
        if let Some(n) = dry {
            d.set_param(&n, 100.0).unwrap();
        }
        assert!(d.set_param("no such param", 0.0).is_err());
        let mut energy = 0.0f64;
        for block in 0..1000 {
            let mut x = vec![0.0f32; 64];
            if block == 0 {
                x[0] = 1.0;
            }
            let mut l = vec![0.0f32; 64];
            let mut r = vec![0.0f32; 64];
            let ch = d.layout().outputs;
            let ins: Vec<&[f32]> = (0..ch).map(|_| &x[..]).collect();
            let mut outs: Vec<&mut [f32]> = vec![&mut l, &mut r];
            outs.truncate(ch);
            d.process(&ins, &mut outs, &[]);
            energy += l.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        }
        assert!(energy > 0.01, "the impulse came back: {energy}");
    }
}
