//! IUnitInfo: parameter folders and the factory-preset program list.

use super::*;

pub(super) unsafe extern "C" fn ui_get_unit_count<P: Vst3Export>(this: *mut FUnknown) -> i32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.units.count() as i32
}

pub(super) unsafe extern "C" fn ui_get_unit_info<P: Vst3Export>(
    this: *mut FUnknown,
    unit_index: i32,
    info: *mut UnitInfo,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, info) = unsafe { (parent_ref::<P>(this), out_param(info, "info")) };
    let result = info.and_then(|dest| {
        // SAFETY: all-zero is a valid `UnitInfo` (integers, UTF-16 arrays).
        *dest = unsafe { mem::zeroed() };
        usize::try_from(unit_index)
            .is_ok_and(|i| inst.units.info(i, inst.programs.list_id(), dest))
            .then_some(())
            .ok_or(CallError::IndexOutOfRange(unit_index.into()))
    });
    inst.answer("getUnitInfo", result)
}

pub(super) unsafe extern "C" fn ui_get_program_list_count<P: Vst3Export>(
    this: *mut FUnknown,
) -> i32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    i32::from(inst.programs.list_id().is_some())
}

pub(super) unsafe extern "C" fn ui_get_program_list_info<P: Vst3Export>(
    this: *mut FUnknown,
    list_index: i32,
    info: *mut ProgramListInfo,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, info) = unsafe { (parent_ref::<P>(this), out_param(info, "info")) };
    let result = info.and_then(|dest| {
        let (Some(id), 0) = (inst.programs.list_id(), list_index) else {
            return Err(CallError::IndexOutOfRange(list_index.into()));
        };
        *dest = ProgramListInfo {
            id,
            name: [0; 128],
            program_count: inst.programs.count() as i32,
        };
        write_utf16_name(&mut dest.name, "Factory Presets");
        Ok(())
    });
    inst.answer("getProgramListInfo", result)
}

pub(super) unsafe extern "C" fn ui_get_program_name<P: Vst3Export>(
    this: *mut FUnknown,
    list_id: ProgramListId,
    program_index: i32,
    name: *mut TChar,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, name) = unsafe { (parent_ref::<P>(this), string128(name, "name")) };
    let result = name.and_then(|dest| {
        if inst.programs.list_id() != Some(list_id) {
            return Err(CallError::UnknownProgramListId(list_id));
        }
        let preset = usize::try_from(program_index)
            .ok()
            .and_then(|i| inst.programs.name_at(i))
            .ok_or(CallError::IndexOutOfRange(program_index.into()))?;
        write_utf16_name(dest, preset);
        Ok(())
    });
    inst.answer("getProgramName", result)
}

pub(super) unsafe extern "C" fn ui_get_program_info(
    _this: *mut FUnknown,
    _list_id: ProgramListId,
    _program_index: i32,
    _attribute_id: FidString,
    _value: *mut TChar,
) -> Tresult {
    K_RESULT_FALSE
}

pub(super) unsafe extern "C" fn ui_has_program_pitch_names(
    _this: *mut FUnknown,
    _list_id: ProgramListId,
    _program_index: i32,
) -> Tresult {
    K_RESULT_FALSE
}

pub(super) unsafe extern "C" fn ui_get_program_pitch_name(
    _this: *mut FUnknown,
    _list_id: ProgramListId,
    _program_index: i32,
    _midi_pitch: i16,
    _name: *mut TChar,
) -> Tresult {
    K_RESULT_FALSE
}

pub(super) unsafe extern "C" fn ui_get_selected_unit<P: Vst3Export>(this: *mut FUnknown) -> UnitId {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    inst.units.selected()
}

pub(super) unsafe extern "C" fn ui_select_unit<P: Vst3Export>(
    this: *mut FUnknown,
    unit_id: UnitId,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    let result = inst
        .units
        .select(unit_id)
        .then_some(())
        .ok_or(CallError::UnknownUnitId(unit_id));
    inst.answer("selectUnit", result)
}

pub(super) unsafe extern "C" fn ui_get_unit_by_bus<P: Vst3Export>(
    this: *mut FUnknown,
    _media_type: i32,
    _direction: i32,
    _bus_index: i32,
    _channel: i32,
    unit_id: *mut UnitId,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, unit_id) = unsafe { (parent_ref::<P>(this), out_param(unit_id, "unitId")) };
    inst.answer("getUnitByBus", unit_id.map(|out| *out = ROOT_UNIT_ID))
}

pub(super) unsafe extern "C" fn ui_set_unit_program_data(
    _this: *mut FUnknown,
    _list_or_unit_id: i32,
    _program_index: i32,
    _data: *mut IBStream,
) -> Tresult {
    K_NOT_IMPLEMENTED
}

pub(super) const fn build_unit_info_vtbl<P: Vst3Export>() -> IUnitInfoVtbl {
    IUnitInfoVtbl {
        query_interface: shared_query_interface::<P>,
        add_ref: shared_add_ref::<P>,
        release: shared_release::<P>,
        get_unit_count: ui_get_unit_count::<P>,
        get_unit_info: ui_get_unit_info::<P>,
        get_program_list_count: ui_get_program_list_count::<P>,
        get_program_list_info: ui_get_program_list_info::<P>,
        get_program_name: ui_get_program_name::<P>,
        get_program_info: ui_get_program_info,
        has_program_pitch_names: ui_has_program_pitch_names,
        get_program_pitch_name: ui_get_program_pitch_name,
        get_selected_unit: ui_get_selected_unit::<P>,
        select_unit: ui_select_unit::<P>,
        get_unit_by_bus: ui_get_unit_by_bus::<P>,
        set_unit_program_data: ui_set_unit_program_data,
    }
}
