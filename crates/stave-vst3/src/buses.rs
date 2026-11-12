//! IComponent bus surface + IAudioProcessor arrangement negotiation
//! buses and arrangements are DERIVED from the plugin's
//! declared `PortConfig` set. The unit of negotiation is the WHOLE
//! configuration: `setBusArrangements` searches CONFIGS for a member
//! matching the full proposal (main + aux inputs, in declaration
//! order) and SELECTS it — per-instance, served by every bus query
//! and the process aux path. No per-bus support predicate exists for
//! hosts to probe (the JUCE lesson).

use core::sync::atomic::Ordering;

use stave_core::{Arrangement, Plugin, PortConfig};

use crate::host::{host_array, out_param};
use crate::sys::*;
use crate::wire_strings::{plugin_is_instrument, write_utf16_name};
use crate::wrapper::{ffi_entry, parent_ref, Instance, Vst3Export};
use stave_core::diagnostics::CallError;

/// The instance's host-accepted configuration.
fn accepted<P: Vst3Export>(inst: &Instance<P>) -> PortConfig {
    let idx = inst.config.load(Ordering::Acquire);
    <P as Plugin>::CONFIGS[idx.min(<P as Plugin>::CONFIGS.len() - 1)]
}

pub(crate) unsafe extern "C" fn comp_get_bus_count<P: Vst3Export>(
    this: *mut FUnknown,
    media_type: i32,
    dir: i32,
) -> i32 {
    // SAFETY: host contract (see `crate::host`).
    let inst = unsafe { parent_ref::<P>(this) };
    ffi_entry::<P, _>(|| 0, || bus_count::<P>(accepted(inst), media_type, dir))
}

fn bus_count<P: Vst3Export>(ports: PortConfig, media_type: i32, dir: i32) -> i32 {
    if media_type == MEDIA_TYPE_AUDIO {
        if dir == BUS_DIR_INPUT {
            // Main (if any) + declared aux buses, declaration order.
            let main = if ports.in_channels() > 0 { 1 } else { 0 };
            main + ports.aux_in.len() as i32
        } else if ports.out_channels() > 0 {
            1
        } else {
            0
        }
    } else if media_type == MEDIA_TYPE_EVENT {
        // One MIDI input bus if the plugin advertises Instrument feature.
        if dir == BUS_DIR_INPUT && plugin_is_instrument::<P>() {
            1
        } else {
            0
        }
    } else {
        0
    }
}

pub(crate) unsafe extern "C" fn comp_get_bus_info<P: Vst3Export>(
    this: *mut FUnknown,
    media_type: i32,
    dir: i32,
    index: i32,
    bus: *mut BusInfo,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, bus) = unsafe { (parent_ref::<P>(this), out_param(bus, "bus")) };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            let result =
                bus.and_then(|bus| bus_info::<P>(accepted(inst), media_type, dir, index, bus));
            inst.answer("getBusInfo", result)
        },
    )
}

fn bus_info<P: Vst3Export>(
    ports: PortConfig,
    media_type: i32,
    dir: i32,
    index: i32,
    info: &mut BusInfo,
) -> Result<(), CallError> {
    let out_of_range = CallError::IndexOutOfRange(index.into());
    if index < 0 {
        return Err(out_of_range);
    }
    info.media_type = media_type;
    info.direction = dir;
    info.bus_type = BUS_TYPE_MAIN;
    info.flags = BUS_FLAG_DEFAULT_ACTIVE;

    if media_type == MEDIA_TYPE_AUDIO {
        if dir == BUS_DIR_INPUT {
            let main = if ports.in_channels() > 0 { 1 } else { 0 };
            if index < main {
                info.channel_count = ports.in_channels() as i32;
                write_utf16_name(&mut info.name, "Input");
            } else {
                // Aux buses follow the main input, declaration order.
                let aux = ports
                    .aux_in
                    .get((index - main) as usize)
                    .ok_or(out_of_range)?;
                info.bus_type = BUS_TYPE_AUX;
                info.channel_count = aux.arrangement.channels() as i32;
                write_utf16_name(&mut info.name, aux.name);
            }
            Ok(())
        } else {
            if index != 0 || ports.out_channels() == 0 {
                return Err(out_of_range);
            }
            info.channel_count = ports.out_channels() as i32;
            write_utf16_name(&mut info.name, "Output");
            Ok(())
        }
    } else if media_type == MEDIA_TYPE_EVENT
        && dir == BUS_DIR_INPUT
        && index == 0
        && plugin_is_instrument::<P>()
    {
        info.channel_count = 16; // standard MIDI channel count
        write_utf16_name(&mut info.name, "Event Input");
        Ok(())
    } else {
        Err(out_of_range)
    }
}

pub(crate) unsafe extern "C" fn comp_get_routing_info(
    _this: *mut FUnknown,
    _in_info: *mut RoutingInfo,
    _out_info: *mut RoutingInfo,
) -> Tresult {
    K_NOT_IMPLEMENTED
}

pub(crate) unsafe extern "C" fn comp_activate_bus(
    _this: *mut FUnknown,
    _media_type: i32,
    _dir: i32,
    _index: i32,
    _state: TBool,
) -> Tresult {
    K_RESULT_OK
}

/// The semantic map: a declared `Arrangement` IS a speaker
/// arrangement — never reconstructed from a channel count.
pub(crate) fn speaker_arr(a: Arrangement) -> SpeakerArrangement {
    match a {
        Arrangement::Mono => K_MONO,
        Arrangement::Stereo => K_STEREO,
        Arrangement::Surround51 => K_51,
    }
}

/// Does `member` match the host's full proposal?
fn member_matches(
    member: &PortConfig,
    inputs: &[SpeakerArrangement],
    outputs: &[SpeakerArrangement],
) -> bool {
    let declared_ins = member
        .main_in
        .into_iter()
        .chain(member.aux_in.iter().map(|aux| aux.arrangement))
        .map(speaker_arr);
    let ins_ok = inputs.len() == usize::from(member.main_in.is_some()) + member.aux_in.len()
        && inputs.iter().copied().eq(declared_ins);
    let outs_ok = match (member.out_channels() > 0, outputs) {
        (false, []) => true,
        (true, [main]) => *main == speaker_arr(member.main_out),
        _ => false,
    };
    ins_ok && outs_ok
}

pub(crate) unsafe extern "C" fn ap_set_bus_arrangements<P: Vst3Export>(
    this: *mut FUnknown,
    inputs: *mut SpeakerArrangement,
    num_ins: i32,
    outputs: *mut SpeakerArrangement,
    num_outs: i32,
) -> Tresult {
    // Set negotiation: accept the FIRST declared member whose
    // whole tuple matches the proposal and select it for this
    // instance; refuse everything else so the host falls back to its
    // converter instead of feeding us mystery channel counts.
    // SAFETY: host contract (see `crate::host`): each array holds its count.
    let (inst, inputs, outputs) = unsafe {
        (
            parent_ref::<P>(this),
            host_array(inputs, num_ins),
            host_array(outputs, num_outs),
        )
    };
    let (Some(inputs), Some(outputs)) = (inputs, outputs) else {
        return K_RESULT_FALSE;
    };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || match <P as Plugin>::CONFIGS
            .iter()
            .position(|member| member_matches(member, inputs, outputs))
        {
            Some(idx) => {
                inst.config.store(idx, Ordering::Release);
                K_RESULT_OK
            }
            None => K_RESULT_FALSE,
        },
    )
}

pub(crate) unsafe extern "C" fn ap_get_bus_arrangement<P: Vst3Export>(
    this: *mut FUnknown,
    dir: i32,
    index: i32,
    arr: *mut SpeakerArrangement,
) -> Tresult {
    // SAFETY: host contract (see `crate::host`).
    let (inst, arr) = unsafe { (parent_ref::<P>(this), out_param(arr, "arrangement")) };
    ffi_entry::<P, _>(
        || K_INTERNAL_ERROR,
        || {
            let result = arr.and_then(|arr| bus_arrangement(accepted(inst), dir, index, arr));
            inst.answer("getBusArrangement", result)
        },
    )
}

fn bus_arrangement(
    ports: PortConfig,
    dir: i32,
    index: i32,
    arr: &mut SpeakerArrangement,
) -> Result<(), CallError> {
    let out_of_range = CallError::IndexOutOfRange(index.into());
    if index < 0 {
        return Err(out_of_range);
    }
    let arrangement: SpeakerArrangement = if dir == BUS_DIR_INPUT {
        let main = if ports.main_in.is_some() { 1 } else { 0 };
        if index < main {
            speaker_arr(ports.main_in.expect("main == 1"))
        } else {
            let aux = ports
                .aux_in
                .get((index - main) as usize)
                .ok_or(out_of_range)?;
            speaker_arr(aux.arrangement)
        }
    } else {
        if index != 0 {
            return Err(out_of_range);
        }
        speaker_arr(ports.main_out)
    };
    *arr = arrangement;
    Ok(())
}
