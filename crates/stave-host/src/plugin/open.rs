//! The load sequence: module → factory → audio-module class →
//! component (+ controller) → setup → buses → activate. Every step is
//! traced, and every acquired object is a guard, so a failure at any
//! step unwinds whatever came before it.

use std::path::{Path, PathBuf};

use stave_vst3::sys::{
    BusInfo, IComponentVtbl, IEditControllerVtbl, ProcessSetup, Tuid, BUS_DIR_INPUT,
    BUS_DIR_OUTPUT, IID_IAUDIO_PROCESSOR, IID_ICOMPONENT, IID_IEDIT_CONTROLLER, K_RESULT_OK,
    MEDIA_TYPE_AUDIO, MEDIA_TYPE_EVENT, PROCESS_MODE_REALTIME, SAMPLE_SIZE_32,
};

use super::com::connection::Connection;
use super::com::event_list::EventList;
use super::com::host_app::host_context;
use super::com::param_changes::ParamChanges;
use super::com::stream::StreamHandle;
use super::com::{Com, Initialized};
use super::controller::{Controller, Ownership};
use super::factory::{choose_audio_module, Factory};
use super::loaded::{ControllerKind, HostStats, Io, LoadedPlugin, PluginInfo};
use super::module::{EntryPoints, Module};
use super::trace::LoadTrace;
use super::transport::Transport;
use super::HostError;
use crate::config::HostConfig;
use crate::instance::Layout;

impl LoadedPlugin {
    /// Load a `.vst3` bundle from disk.
    pub fn open(bundle: &Path, config: HostConfig) -> Result<Self, HostError> {
        let mut trace = LoadTrace::new();
        let module = trace.step(
            "module",
            || Module::load(bundle),
            |_| bundle.display().to_string(),
        )?;
        build(module, Some(bundle.to_path_buf()), config, trace)
    }

    /// Host statically linked entry points (tests, embedders).
    pub fn from_entry_points(entry: EntryPoints, config: HostConfig) -> Result<Self, HostError> {
        let mut trace = LoadTrace::new();
        let module = trace.step(
            "module",
            || Module::from_entry_points(entry),
            |_| "static entry points".into(),
        )?;
        build(module, None, config, trace)
    }
}

fn build(
    module: Module,
    bundle: Option<PathBuf>,
    config: HostConfig,
    mut trace: LoadTrace,
) -> Result<LoadedPlugin, HostError> {
    let factory = trace.step(
        "factory",
        || Factory::new(module.entry()),
        |_| String::new(),
    )?;
    let factory_vendor = factory.vendor();
    let classes = factory.classes();
    let class = trace.step(
        "class",
        || {
            choose_audio_module(&classes)
                .cloned()
                .ok_or_else(|| HostError::NoAudioModule {
                    classes: classes
                        .iter()
                        .map(|c| format!("{} ({})", c.name, c.category))
                        .collect(),
                })
        },
        |c| format!("#{} \"{}\" of {}", c.index, c.name, classes.len()),
    )?;

    let component: Com<IComponentVtbl> = trace.step(
        "create component",
        || {
            // SAFETY: the vtable type matches the iid (here and in the
            // queries below).
            unsafe { factory.create(&class.cid, &IID_ICOMPONENT) }
                .map_err(HostError::call("createInstance"))
        },
        |_| String::new(),
    )?;
    let processor = trace.step(
        "audio processor",
        || {
            // SAFETY: as above.
            unsafe { component.query(&IID_IAUDIO_PROCESSOR) }.ok_or(HostError::call(
                "queryInterface(IAudioProcessor)",
            )(
                stave_vst3::sys::K_NO_INTERFACE,
            ))
        },
        |_| String::new(),
    )?;
    // SAFETY: as above.
    let single_object: Option<Com<IEditControllerVtbl>> =
        unsafe { component.query(&IID_IEDIT_CONTROLLER) };
    let component = trace.step(
        "initialize component",
        || {
            // SAFETY: IComponent's vtable starts with IPluginBase.
            unsafe { Initialized::new(component, host_context()) }
                .map_err(HostError::call("IComponent::initialize"))
        },
        |_| String::new(),
    )?;

    let (controller, connection) = match single_object {
        Some(c) => (Some(Controller::new(Ownership::Shared(c))), None),
        None => separate_controller(&factory, &component, &mut trace),
    };
    let controller_kind = match &controller {
        None => ControllerKind::None,
        Some(c) if c.is_separate() => ControllerKind::Separate,
        Some(_) => ControllerKind::SingleObject,
    };

    let (inputs, outputs, event_inputs) = activate_buses(&component);
    trace.note(
        "buses",
        true,
        format!("audio {inputs} in / {outputs} out, events: {event_inputs}"),
    );

    trace.step(
        "setup processing",
        || setup(&processor, config),
        |_| {
            format!(
                "{} Hz, max block {}",
                config.sample_rate.hz(),
                config.max_block
            )
        },
    )?;

    let debug_bus = module.entry().debug_bus_for.map_or(std::ptr::null(), |f| {
        // SAFETY: stave's exported lookup takes any interface pointer
        // of a live instance; `component` is one.
        unsafe { f(component.as_ptr().cast()) }
    });
    // SAFETY: `abi_version` is the bus's first field in every layout; a
    // bus from another layout is not read past it.
    let debug_bus = match unsafe { debug_bus.as_ref() } {
        Some(b) if b.abi_version != stave_debug::ABI_VERSION => {
            trace.note(
                "debug bus",
                false,
                format!(
                    "ABI {} (host speaks {})",
                    b.abi_version,
                    stave_debug::ABI_VERSION
                ),
            );
            std::ptr::null()
        }
        _ => debug_bus,
    };
    let param_count = controller
        .as_ref()
        .map_or(0, |c| c.handle().controls().len());
    let layout = Layout {
        inputs,
        outputs,
        sample_rate: config.sample_rate.hz(),
        max_block: config.max_block.get(),
    };
    let mut plugin = LoadedPlugin {
        io: Io::new(&layout),
        layout,
        info: PluginInfo {
            bundle,
            class,
            factory_vendor,
            controller: controller_kind,
            connected: connection.is_some(),
            event_inputs,
            stave_debug_bus: !debug_bus.is_null(),
            load: LoadTrace::default(),
        },
        events: EventList::new(),
        changes: ParamChanges::with_capacity(param_count.max(16)),
        transport: Transport::new(config.transport, config.sample_rate.hz()),
        chunk_notes: Vec::with_capacity(super::com::event_list::CAPACITY),
        stats: HostStats::default(),
        debug_bus,
        active: false,
        processing: false,
        _connection: connection,
        controller,
        processor,
        component,
        _factory: factory,
        _module: module,
    };
    trace.step("activate", || plugin.activate(), |_| String::new())?;
    plugin.info.load = trace;
    Ok(plugin)
}

/// Split-architecture plugins: create the controller class the
/// component names, initialize it, wire the connection points, and hand
/// it the component's state. Failure is not fatal — the plugin still
/// processes, it just has no parameters.
fn separate_controller(
    factory: &Factory,
    component: &Initialized<IComponentVtbl>,
    trace: &mut LoadTrace,
) -> (Option<Controller>, Option<Connection>) {
    let mut cid: Tuid = [0; 16];
    // SAFETY: the object's own method, called on the live object `Com` holds.
    let r = unsafe { (component.vtbl().get_controller_class_id)(component.as_ptr(), &mut cid) };
    if r != K_RESULT_OK {
        trace.note("controller", true, "none (no controller class id)");
        return (None, None);
    }
    let created = trace.step(
        "create controller",
        || -> Result<_, HostError> {
            // SAFETY: the vtable type matches the iid.
            let com: Com<IEditControllerVtbl> =
                unsafe { factory.create(&cid, &IID_IEDIT_CONTROLLER) }
                    .map_err(HostError::call("createInstance(controller)"))?;
            // SAFETY: IEditController's vtable starts with IPluginBase.
            unsafe { Initialized::new(com, host_context()) }
                .map_err(HostError::call("IEditController::initialize"))
        },
        |_| "separate controller".into(),
    );
    let Ok(ctrl) = created else {
        return (None, None);
    };
    let connection = Connection::wire(component.com(), ctrl.com());
    trace.note(
        "connect",
        connection.is_some(),
        if connection.is_some() {
            "wired"
        } else {
            "not offered"
        },
    );

    // Component state → controller, so the controller's values reflect
    // the DSP's (the spec's setComponentState handshake).
    let mut s = StreamHandle::new(Vec::new());
    // SAFETY: the object's own method, called on the live object `Com` holds.
    let got = unsafe { (component.vtbl().get_state)(component.as_ptr(), s.as_funknown()) };
    let synced = got == K_RESULT_OK && {
        s.rewind();
        // SAFETY: the object's own method, called on the live object `Com` holds.
        unsafe { (ctrl.vtbl().set_component_state)(ctrl.as_ptr(), s.as_funknown()) == K_RESULT_OK }
    };
    trace.note(
        "component state",
        synced,
        format!("{} bytes", s.bytes().len()),
    );
    (Some(Controller::new(Ownership::Separate(ctrl))), connection)
}

fn setup(
    processor: &Com<stave_vst3::sys::IAudioProcessorVtbl>,
    config: HostConfig,
) -> Result<(), HostError> {
    let mut setup = ProcessSetup {
        process_mode: PROCESS_MODE_REALTIME,
        symbolic_sample_size: SAMPLE_SIZE_32,
        max_samples_per_block: i32::try_from(config.max_block.get())
            .map_err(|_| HostError::BlockTooLarge(config.max_block.get()))?,
        sample_rate: config.sample_rate.hz(),
    };
    // SAFETY: the object's own method, called on the live object `Com` holds.
    match unsafe { (processor.vtbl().setup_processing)(processor.as_ptr(), &mut setup) } {
        K_RESULT_OK => Ok(()),
        r => Err(HostError::call("setupProcessing")(r)),
    }
}

/// Activate the MAIN audio buses and the first event input; every
/// auxiliary bus (sidechains) is explicitly deactivated — the host only
/// ever supplies buffers for the main pair. Returns (in ch, out ch,
/// has event input).
fn activate_buses(component: &Initialized<IComponentVtbl>) -> (usize, usize, bool) {
    // Each call below: the component's own methods, called on
    // the live component `Initialized` holds.
    let vt = component.vtbl();
    let obj = component.as_ptr();
    let count = |media, dir| {
        // SAFETY: see above.
        unsafe { (vt.get_bus_count)(obj, media, dir) }.max(0)
    };
    let set_all = |media, dir, main_on: bool| {
        for i in 0..count(media, dir) {
            // SAFETY: see above.
            unsafe { (vt.activate_bus)(obj, media, dir, i, u8::from(i == 0 && main_on)) };
        }
    };
    let channels = |dir| {
        if count(MEDIA_TYPE_AUDIO, dir) == 0 {
            return 0;
        }
        // SAFETY: all-zero is a valid `BusInfo` (integers, a UTF-16 array).
        let mut bus = unsafe { std::mem::zeroed::<BusInfo>() };
        // SAFETY: see above.
        let r = unsafe { (vt.get_bus_info)(obj, MEDIA_TYPE_AUDIO, dir, 0, &mut bus) };
        if r == K_RESULT_OK {
            bus.channel_count.max(0) as usize
        } else {
            0
        }
    };
    let (ins, outs) = (channels(BUS_DIR_INPUT), channels(BUS_DIR_OUTPUT));
    set_all(MEDIA_TYPE_AUDIO, BUS_DIR_INPUT, true);
    set_all(MEDIA_TYPE_AUDIO, BUS_DIR_OUTPUT, true);
    set_all(MEDIA_TYPE_EVENT, BUS_DIR_INPUT, true);
    (ins, outs, count(MEDIA_TYPE_EVENT, BUS_DIR_INPUT) > 0)
}
