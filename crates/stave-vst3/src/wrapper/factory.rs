//! IPluginFactory: class info and instance creation.

use super::*;

/// The static factory object the host receives from `GetPluginFactory`.
/// Just a vtable pointer — no per-instance state.
#[repr(C)]
pub struct FactoryObj {
    pub vtbl: *const IPluginFactoryVtbl,
}

// SAFETY: FactoryObj is a static read by hosts (which may call across
// threads); it has no interior mutability. The vtable pointer points into
// another static and the methods themselves take no shared state.
unsafe impl Sync for FactoryObj {}

pub(super) unsafe extern "C" fn factory_query_interface(
    this: *mut FUnknown,
    iid: *const Tuid,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: `iid` and `obj` are null or valid.
    let (iid, obj) = unsafe { (iid.as_ref(), out_param(obj, "obj")) };
    let (Some(&requested), Ok(obj)) = (iid, obj) else {
        return K_INVALID_ARGUMENT;
    };
    let known = [
        IID_FUNKNOWN,
        IID_IPLUGIN_FACTORY,
        IID_IPLUGIN_FACTORY2,
        IID_IPLUGIN_FACTORY3,
    ];
    // Array comparisons and pointer writes only: nothing here can panic.
    if known.contains(&requested) {
        *obj = this.cast();
        K_RESULT_OK
    } else {
        *obj = ptr::null_mut();
        K_NO_INTERFACE
    }
}

pub(super) unsafe extern "C" fn factory_add_ref(_this: *mut FUnknown) -> u32 {
    1
}

pub(super) unsafe extern "C" fn factory_release(_this: *mut FUnknown) -> u32 {
    1
}

pub(super) unsafe extern "C" fn factory_get_factory_info<P: Vst3Export>(
    _this: *mut FUnknown,
    info: *mut PFactoryInfo,
) -> Tresult {
    // SAFETY: SDK: null or writable.
    let Ok(dst) = (unsafe { out_param(info, "info") }) else {
        return K_INVALID_ARGUMENT;
    };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            // SAFETY: all-zero is a valid `PFactoryInfo` (char arrays, an
            // integer).
            *dst = unsafe { mem::zeroed() };
            let desc = <P as Plugin>::DESCRIPTOR;
            write_cstr_into(&mut dst.vendor, desc.vendor);
            write_cstr_into(&mut dst.url, desc.url);
            // Email — we don't have one in the descriptor; leave empty.
            dst.flags = FACTORY_FLAG_UNICODE;
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn factory_count_classes(_this: *mut FUnknown) -> i32 {
    // One class: we expose a fused component+controller as a single class.
    // (VST3 also supports declaring them separately; for now we register only
    // the component class, since IComponent::getControllerClassId returns the
    // same CID — a host will use that to find the controller.)
    1
}

pub(super) unsafe extern "C" fn factory_get_class_info<P: Vst3Export>(
    _this: *mut FUnknown,
    index: i32,
    info: *mut PClassInfo,
) -> Tresult {
    // SAFETY: SDK: null or writable.
    let (Ok(dst), 0) = (unsafe { out_param(info, "info") }, index) else {
        return K_INVALID_ARGUMENT;
    };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            let desc = <P as Plugin>::DESCRIPTOR;
            // SAFETY: all-zero is a valid `PClassInfo` (a byte id, integers, char
            // arrays).
            *dst = unsafe { mem::zeroed() };
            dst.cid = P::COMPONENT_CID;
            dst.cardinality = K_MANY_INSTANCES;
            write_cstr_into(&mut dst.category, CATEGORY_EFFECT);
            write_cstr_into(&mut dst.name, desc.name);
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn factory_get_class_info_2<P: Vst3Export>(
    _this: *mut FUnknown,
    index: i32,
    info: *mut PClassInfo2,
) -> Tresult {
    // SAFETY: SDK: null or writable.
    let (Ok(dst), 0) = (unsafe { out_param(info, "info") }, index) else {
        return K_INVALID_ARGUMENT;
    };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            let desc = <P as Plugin>::DESCRIPTOR;
            // SAFETY: all-zero is a valid `PClassInfo2` (a byte id, integers, char
            // arrays).
            *dst = unsafe { mem::zeroed() };
            dst.cid = P::COMPONENT_CID;
            dst.cardinality = K_MANY_INSTANCES;
            write_cstr_into(&mut dst.category, CATEGORY_EFFECT);
            write_cstr_into(&mut dst.name, desc.name);
            dst.class_flags = 0;
            write_subcategories::<P>(&mut dst.sub_categories);
            write_cstr_into(&mut dst.vendor, desc.vendor);
            write_cstr_into(&mut dst.version, desc.version);
            write_cstr_into(&mut dst.sdk_version, c"VST 3.7.0");
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn factory_get_class_info_unicode<P: Vst3Export>(
    _this: *mut FUnknown,
    index: i32,
    info: *mut PClassInfoW,
) -> Tresult {
    // SAFETY: SDK: null or writable.
    let (Ok(dst), 0) = (unsafe { out_param(info, "info") }, index) else {
        return K_INVALID_ARGUMENT;
    };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            let desc = <P as Plugin>::DESCRIPTOR;
            // SAFETY: all-zero is a valid `PClassInfoW` (a byte id, integers, char
            // arrays).
            *dst = unsafe { mem::zeroed() };
            dst.cid = P::COMPONENT_CID;
            dst.cardinality = K_MANY_INSTANCES;
            write_cstr_into(&mut dst.category, CATEGORY_EFFECT);
            write_utf16_name(&mut dst.name, cstr_to_str(desc.name));
            dst.class_flags = 0;
            write_subcategories::<P>(&mut dst.sub_categories);
            write_utf16_name(&mut dst.vendor, cstr_to_str(desc.vendor));
            write_utf16_name(&mut dst.version, cstr_to_str(desc.version));
            write_utf16_name(&mut dst.sdk_version, "VST 3.7.0");
            K_RESULT_OK
        },
    )
}

pub(super) unsafe extern "C" fn factory_set_host_context(
    _this: *mut FUnknown,
    _context: *mut FUnknown,
) -> Tresult {
    K_RESULT_OK
}

pub(super) unsafe extern "C" fn factory_create_instance<P: Vst3Export>(
    _this: *mut FUnknown,
    cid: FidString,
    iid: FidString,
    obj: *mut *mut c_void,
) -> Tresult {
    // SAFETY: SDK: `cid` and `iid` are null or 16-byte TUIDs passed as
    // `const char*`; `obj` is null or writable.
    let (cid, obj_out) = unsafe { ((cid as *const Tuid).as_ref(), out_param(obj, "obj")) };
    let Ok(obj_out) = obj_out else {
        return K_INVALID_ARGUMENT;
    };
    *obj_out = ptr::null_mut();
    let Some(&requested_cid) = cid.filter(|_| !iid.is_null()) else {
        return K_INVALID_ARGUMENT;
    };
    if requested_cid != P::COMPONENT_CID && requested_cid != P::CONTROLLER_CID {
        return K_NO_INTERFACE;
    }
    // `P::new()` and every author hook below are plugin code. On a panic
    // the partly built values drop during the unwind and the host gets
    // an error with `*obj` still null.
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        // SAFETY: `iid` is a non-null TUID and `obj` writable (checked).
        || unsafe { create_instance::<P>(iid, obj) },
    )
}

/// # Safety
/// `iid` is a 16-byte TUID; `obj` is writable.
unsafe fn create_instance<P: Vst3Export>(iid: FidString, obj: *mut *mut c_void) -> Tresult {
    // Telemetry exists before the plugin so it can be attached right
    // after construction. The wrapper's I/O taps are registered (and
    // read the floor) before audio runs, so the editor's meter rails
    // draw from the first frame.
    let telemetry = Telemetry::new();
    let taps = WrapperTaps::new(&telemetry);
    let floor = [WrapperTaps::FLOOR_DB; 3];
    taps.publish_peaks(floor, floor);

    let mut plugin_obj = P::new();
    plugin_obj.attach_telemetry(telemetry.clone());
    let diagnostics = Diagnostics::new(&P::DESCRIPTOR);
    diagnostics.log.add(Box::new(TelemetrySink(telemetry.clone())));
    let (param_table, face) = (plugin_obj.param_table(), plugin_obj.face());
    if let Err(e) = param_table.validate() {
        // Author bug (duplicate or reserved ids): refuse the instance
        // loudly rather than corrupt automation.
        Diagnostics::new(&P::DESCRIPTOR).log.write(
            Severity::PluginMisbehaving,
            &format_args!("param table invalid: {e}"),
        );
        debug_assert!(false, "param table invalid: {e}");
        return K_INTERNAL_ERROR;
    }
    // Seed the latency mirror while we still own the plugin exclusively;
    // hosts may query it before setActive.
    let programs = crate::programs::Programs::new(plugin_obj.presets());
    let shared = SharedState::new(44_100.0);
    shared.set_latency(plugin_obj.latency_samples());
    let inst = Box::into_raw(Box::new(Instance::<P> {
        component: ComponentObj {
            vtbl: ptr::null(),
            parent: ptr::null_mut(),
        },
        processor: ProcessorObj {
            vtbl: ptr::null(),
            parent: ptr::null_mut(),
        },
        controller: ControllerObj {
            vtbl: ptr::null(),
            parent: ptr::null_mut(),
        },
        pcr: PcrObj {
            vtbl: ptr::null(),
            parent: ptr::null_mut(),
        },
        pcr_vtbl: build_pcr_vtbl::<P>(),
        unit_info: UnitInfoObj {
            vtbl: ptr::null(),
            parent: ptr::null_mut(),
        },
        unit_info_vtbl: build_unit_info_vtbl::<P>(),
        #[cfg(target_os = "macos")]
        view_vtbl: build_view_vtbl::<P>(),
        ref_count: AtomicU32::new(0),
        shared,
        plugin: PluginCell::new(plugin_obj),
        bypass_id: (0..param_table.count())
            .filter_map(|i| param_table.by_index(i))
            .find(|p| p.meta().flags.bypass)
            .map(|p| p.meta().id.0),
        units: crate::units::Units::new(param_table.as_ref()),
        param_table,
        programs,
        face,
        state_store: std::sync::Mutex::new(StateStore::default()),
        diagnostics,
        guard: PanicGuard::default(),
        handler: Arc::new(ComponentHandler::none()),
        tail: AtomicU32::new(0),
        telemetry,
        taps,
        config: core::sync::atomic::AtomicUsize::new(0),
    }));

    // Wire up vtables + back-pointers. Has to happen after Box::into_raw
    // because each sub-object needs the parent's heap address.
    // SAFETY: `inst` is the fresh, unshared allocation.
    unsafe {
        (*inst).component.vtbl = P::component_vtbl();
        (*inst).component.parent = inst;
        (*inst).processor.vtbl = P::processor_vtbl();
        (*inst).processor.parent = inst;
        (*inst).controller.vtbl = P::controller_vtbl();
        (*inst).controller.parent = inst;
        (*inst).pcr.vtbl = ptr::addr_of!((*inst).pcr_vtbl);
        (*inst).pcr.parent = inst;
        (*inst).unit_info.vtbl = ptr::addr_of!((*inst).unit_info_vtbl);
        (*inst).unit_info.parent = inst;
    }

    // The host's first reference: the interface it asked for, through the
    // same lookup `queryInterface` serves.
    // SAFETY: `inst` is live and wired; `component` is one of its
    // interfaces; `iid` is a TUID and `obj` writable (this function's
    // contract). On refusal nothing else has seen `inst`, so the
    // `Box::into_raw` above is reclaimed here.
    unsafe {
        let component = ptr::addr_of_mut!((*inst).component).cast::<FUnknown>();
        let result = shared_query_interface::<P>(component, iid.cast(), obj);
        if result != K_RESULT_OK {
            drop(Box::from_raw(inst));
        }
        result
    }
}

// Build the factory vtable as a const fn so the macro can produce a static.
pub const fn build_factory_vtbl<P: Vst3Export>() -> IPluginFactoryVtbl {
    IPluginFactoryVtbl {
        query_interface: factory_query_interface,
        add_ref: factory_add_ref,
        release: factory_release,
        get_factory_info: factory_get_factory_info::<P>,
        count_classes: factory_count_classes,
        get_class_info: factory_get_class_info::<P>,
        create_instance: factory_create_instance::<P>,
        get_class_info_2: factory_get_class_info_2::<P>,
        get_class_info_unicode: factory_get_class_info_unicode::<P>,
        set_host_context: factory_set_host_context,
    }
}
