//! Buses: the input and output elements, the formats the host sets on
//! them, which of the plugin's [`Layout`](stave_core::Layout)s those
//! select, and `Initialize`/`Uninitialize`, which prepare the plugin for
//! them.
//!
//! The elements: input element 0 is the main input (absent when no
//! layout has one), then one per sidechain (`Layout::aux`, in order);
//! output element 0 is the main output. Every format is the canonical
//! one — 32-bit float, non-interleaved — at one sample rate for all;
//! channel counts are what the host negotiates, and `Initialize` refuses
//! counts no layout declares (`kAudioUnitErr_FormatNotSupported`).

use core::ffi::c_void;

use stave_core::runtime::MAX_AUX_PORTS;
use stave_core::Environment;

use super::*;

/// What a fresh instance runs at until the host says otherwise
/// (AudioToolbox's defaults).
pub(crate) const DEFAULT_RATE: f64 = 44_100.0;
pub(crate) const DEFAULT_MAX_FRAMES: u32 = 1156;
/// The most channels one element carries.
pub(crate) const MAX_CHANNELS: usize = 8;

/// A property listener the host registered.
#[derive(Copy, Clone)]
pub(crate) struct Listener {
    pub(crate) id: sys::AudioUnitPropertyID,
    pub(crate) proc_: sys::AudioUnitPropertyListenerProc,
    pub(crate) refcon: *mut c_void,
}

/// The host's configuration.
pub(crate) struct Props {
    pub(crate) sample_rate: f64,
    pub(crate) max_frames: u32,
    /// Channels of each input element.
    pub(crate) inputs: Vec<u32>,
    pub(crate) output: u32,
    pub(crate) initialized: bool,
    pub(crate) listeners: Vec<Listener>,
    pub(crate) offline: u32,
}

impl Props {
    pub(crate) fn new<P: AuExport>() -> Self {
        let first = P::LAYOUTS[0];
        let mut inputs = Vec::new();
        if has_main_input::<P>() {
            inputs.push(first.input_channels().max(1) as u32);
        }
        for a in 0..aux_count::<P>() {
            inputs.push(first.aux.get(a).map_or(1, |x| x.channels.count()) as u32);
        }
        Props {
            sample_rate: DEFAULT_RATE,
            max_frames: DEFAULT_MAX_FRAMES,
            inputs,
            output: first.output_channels() as u32,
            initialized: false,
            listeners: Vec::new(),
            offline: 0,
        }
    }
}

/// Where an input element's audio comes from.
#[derive(Copy, Clone)]
pub(crate) enum Source {
    None,
    Callback(sys::AURenderCallbackStruct),
    Connection(sys::AudioUnitConnection),
}

/// What the audio callback works with: the host's sources and
/// callbacks, and buffers sized at `Initialize` (no allocation while
/// processing).
pub(crate) struct Io {
    pub(crate) sources: Vec<Source>,
    pub(crate) host: Option<sys::HostCallbackInfo>,
    pub(crate) notifies: Vec<(sys::AURenderCallback, *mut c_void)>,
    /// Per input element, its channels' samples end to end.
    pub(crate) input: Vec<Vec<f32>>,
    pub(crate) output: Vec<f32>,
    /// Frames each channel of the buffers holds.
    pub(crate) frames: usize,
    /// The formats the buffers were made for.
    pub(crate) inputs: Vec<u32>,
    pub(crate) output_channels: u32,
    /// The index into `LAYOUTS` the formats selected.
    pub(crate) layout: usize,
}

impl Io {
    pub(crate) fn new<P: AuExport>() -> Self {
        Io {
            sources: vec![Source::None; input_elements::<P>()],
            host: None,
            notifies: Vec::new(),
            input: Vec::new(),
            output: Vec::new(),
            frames: 0,
            inputs: Vec::new(),
            output_channels: 0,
            layout: 0,
        }
    }

    fn allocate(&mut self, props: &Props, layout: usize) {
        let frames = props.max_frames as usize;
        self.input = props.inputs.iter().map(|&c| vec![0.0; c as usize * frames]).collect();
        self.output = vec![0.0; props.output as usize * frames];
        self.frames = frames;
        self.inputs = props.inputs.clone();
        self.output_channels = props.output;
        self.layout = layout;
    }
}

/// Some layout has a main input.
pub(crate) fn has_main_input<P: AuExport>() -> bool {
    P::LAYOUTS.iter().any(|l| l.input.is_some())
}

/// The most sidechains any layout declares.
pub(crate) fn aux_count<P: AuExport>() -> usize {
    P::LAYOUTS.iter().map(|l| l.aux.len()).max().unwrap_or(0).min(MAX_AUX_PORTS)
}

pub(crate) fn input_elements<P: AuExport>() -> usize {
    usize::from(has_main_input::<P>()) + aux_count::<P>()
}

/// The widest main input any layout declares (the bypass gate and the
/// scratch are sized for it).
pub(crate) fn max_in_channels<P: AuExport>() -> usize {
    P::LAYOUTS.iter().map(|c| c.input_channels()).max().unwrap_or(0)
}

/// `kAudioUnitProperty_SupportedNumChannels`: each layout's main
/// (input, output), once each, in preference order.
pub(crate) fn supported<P: AuExport>() -> Vec<sys::AUChannelInfo> {
    let mut out: Vec<sys::AUChannelInfo> = Vec::new();
    for l in P::LAYOUTS {
        let info = sys::AUChannelInfo {
            inChannels: l.input_channels() as i16,
            outChannels: l.output_channels() as i16,
        };
        if !out.contains(&info) {
            out.push(info);
        }
    }
    out
}

/// The first layout the formats match: main input and output, and each
/// sidechain the layout declares.
pub(crate) fn select<P: AuExport>(inputs: &[u32], output: u32) -> Option<usize> {
    let main = usize::from(has_main_input::<P>());
    P::LAYOUTS.iter().position(|l| {
        let main_ok = main == 0 || inputs.first().copied() == Some(l.input_channels() as u32);
        let aux_ok = l
            .aux
            .iter()
            .take(MAX_AUX_PORTS)
            .enumerate()
            .all(|(a, x)| inputs.get(main + a).copied() == Some(x.channels.count() as u32));
        l.input.is_some() == (main == 1) && main_ok && aux_ok && l.output_channels() as u32 == output
    })
}

/// Whether `channels` is a count element `element` of `scope` takes in
/// some layout.
pub(crate) fn channels_allowed<P: AuExport>(scope: sys::AudioUnitScope, element: u32, channels: u32) -> bool {
    let main = usize::from(has_main_input::<P>());
    P::LAYOUTS.iter().any(|l| match scope {
        sys::kAudioUnitScope_Output => l.output_channels() as u32 == channels,
        _ if (element as usize) < main => l.input_channels() as u32 == channels,
        _ => l.aux.get(element as usize - main).is_some_and(|x| x.channels.count() as u32 == channels),
    })
}

/// The canonical format with `channels` at `rate`.
pub(crate) fn canonical(rate: f64, channels: u32) -> sys::AudioStreamBasicDescription {
    sys::AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: sys::kAudioFormatLinearPCM,
        mFormatFlags: sys::kAudioFormatFlagIsFloat | sys::kAudioFormatFlagIsPacked | sys::kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

/// `f` is the canonical format (any rate, 1 to [`MAX_CHANNELS`]
/// channels); the packed flag is optional, as AudioToolbox treats it.
pub(crate) fn is_canonical(f: &sys::AudioStreamBasicDescription) -> bool {
    let flags = f.mFormatFlags & !sys::kAudioFormatFlagIsPacked;
    f.mFormatID == sys::kAudioFormatLinearPCM
        && flags == sys::kAudioFormatFlagIsFloat | sys::kAudioFormatFlagIsNonInterleaved
        && f.mBitsPerChannel == 32
        && f.mBytesPerFrame == 4
        && f.mBytesPerPacket == 4
        && f.mFramesPerPacket == 1
        && (1..=MAX_CHANNELS as u32).contains(&f.mChannelsPerFrame)
        && f.mSampleRate > 0.0
        && f.mSampleRate.is_finite()
}

/// Prepare the plugin for `props` (the layout they select, the rate,
/// the largest block) and size the buffers. [not audio]
pub(crate) fn prepare<P: AuExport>(inst: &Instance<P>, props: &mut Props) -> OSStatus {
    let Some(layout) = select::<P>(&props.inputs, props.output) else {
        return sys::kAudioUnitErr_FormatNotSupported;
    };
    let Some(mut proc) = inst.plugin.claim() else {
        inst.report("Initialize", &CallError::ProcessorBusy);
        return sys::kAudioUnitErr_CannotDoInCurrentContext;
    };
    let rate = props.sample_rate as f32;
    let env = Environment::new(rate, props.max_frames as usize).with_layout(P::LAYOUTS[layout]);
    let latency = proc.prepare(&env, max_in_channels::<P>());
    inst.diagnostics.event(stave_core::telemetry::Event::Prepare {
        sample_rate: rate,
        max_block: props.max_frames,
    });
    inst.shared.set_sample_rate(rate);
    inst.shared.set_max_frames(props.max_frames);
    inst.shared.set_latency(latency);
    inst.tail.store(stave_core::runtime::tail_wire(proc.plugin.tail()), Ordering::Release);
    drop(proc);
    lock(&inst.io).allocate(props, layout);
    props.initialized = true;
    inst.shared.set_activated(true);
    sys::noErr
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn initialize<P: AuExport>(this: *mut c_void) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    let status = inst.guarded(
        "Initialize",
        || sys::kAudioUnitErr_FailedInitialization,
        || {
            let mut props = lock(&inst.props);
            if props.initialized {
                return sys::noErr;
            }
            prepare(inst, &mut props)
        },
    );
    if status == sys::noErr {
        inst.changed(sys::kAudioUnitProperty_Latency, sys::kAudioUnitScope_Global, 0);
        inst.changed(sys::kAudioUnitProperty_TailTime, sys::kAudioUnitScope_Global, 0);
    }
    status
}

/// # Safety
/// AudioToolbox's contract: `this` came from `factory`.
pub(crate) unsafe extern "C" fn uninitialize<P: AuExport>(this: *mut c_void) -> OSStatus {
    // SAFETY: this function's contract.
    let inst = unsafe { Instance::<P>::from_this(this) };
    inst.diagnostics.report_faults();
    inst.guarded(
        "Uninitialize",
        || sys::noErr,
        || {
            let mut props = lock(&inst.props);
            if !props.initialized {
                return sys::noErr;
            }
            match inst.plugin.claim() {
                Some(mut proc) => proc.release(),
                None => inst.report("Uninitialize", &CallError::ProcessorBusy),
            }
            inst.diagnostics.event(stave_core::telemetry::Event::Release);
            inst.shared.set_activated(false);
            props.initialized = false;
            sys::noErr
        },
    )
}
