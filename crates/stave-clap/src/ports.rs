//! CLAP audio-ports extension: ports are DERIVED from the
//! plugin's declared `PortConfig` — main + aux inputs in declaration
//! order, names from the `AuxIn` declarations. audio-ports-config
//! (the declared SET) joins here with negotiation.

use core::ffi::c_char;

use stave_core::Plugin;

use crate::sys;
use crate::wrapper::{out_param, write_c_str, ClapExport, Instance};
use stave_core::diagnostics::CallError;

use core::sync::atomic::Ordering;

use stave_core::PortConfig;

/// The instance's host-selected configuration.
fn selected<P: ClapExport>(inst: &Instance<P>) -> PortConfig {
    let idx = inst.config.load(Ordering::Acquire);
    <P as Plugin>::CONFIGS[idx.min(<P as Plugin>::CONFIGS.len() - 1)]
}

pub(crate) unsafe extern "C" fn audio_ports_count<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    is_input: bool,
) -> u32 {
    // SAFETY: host contract (see `wrapper::host`).
    let ports = selected(unsafe { Instance::<P>::from_plugin_ptr(plugin) });
    if is_input {
        // Main (if any) + declared aux ports, in declaration order.
        let main = if ports.in_channels() > 0 { 1 } else { 0 };
        main + ports.aux_in.len() as u32
    } else if ports.out_channels() > 0 {
        1
    } else {
        0
    }
}

pub(crate) unsafe extern "C" fn audio_ports_get<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    index: u32,
    is_input: bool,
    info: *mut sys::ClapAudioPortInfo,
) -> bool {
    // SAFETY: host contract (see `wrapper::host`).
    let (inst, info) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(info, "info"),
        )
    };
    inst.answer(
        "audio_ports.get",
        info.and_then(|info| port_info(selected(inst), index, is_input, info)),
    )
}

fn port_info(
    ports: PortConfig,
    index: u32,
    is_input: bool,
    out: &mut sys::ClapAudioPortInfo,
) -> Result<(), CallError> {
    let (arrangement, flags, name): (stave_core::Arrangement, u32, &str) = if is_input {
        let main = if ports.main_in.is_some() { 1 } else { 0 };
        if index < main {
            (
                ports.main_in.expect("main == 1"),
                sys::CLAP_AUDIO_PORT_IS_MAIN,
                "main",
            )
        } else {
            // Aux ports follow the main input, declaration order.
            let aux = ports
                .aux_in
                .get((index - main) as usize)
                .ok_or(CallError::IndexOutOfRange(index.into()))?;
            (aux.arrangement, 0, aux.name)
        }
    } else {
        if index != 0 || ports.out_channels() == 0 {
            return Err(CallError::IndexOutOfRange(index.into()));
        }
        (ports.main_out, sys::CLAP_AUDIO_PORT_IS_MAIN, "main")
    };

    out.id = index;
    out.flags = flags;
    out.channel_count = arrangement.channels();
    out.port_type = port_type_for(arrangement);
    out.in_place_pair = sys::CLAP_INVALID_ID;

    write_c_str(&mut out.name, name);
    Ok(())
}

/// The semantic map: CLAP port-type string per Arrangement.
/// Surround has NO well-known core type string — null is the legal
/// "unspecified" value; hosts read channel_count (the CLAP surround
/// extension can add the channel map when a host demands one).
fn port_type_for(a: stave_core::Arrangement) -> *const c_char {
    match a {
        stave_core::Arrangement::Mono => sys::CLAP_PORT_MONO.as_ptr(),
        stave_core::Arrangement::Stereo => sys::CLAP_PORT_STEREO.as_ptr(),
        stave_core::Arrangement::Surround51 => core::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// audio-ports-config: the declared SET, host-selectable
// ---------------------------------------------------------------------------

pub(crate) unsafe extern "C" fn config_count<P: ClapExport>(
    _plugin: *const sys::ClapPlugin,
) -> u32 {
    <P as Plugin>::CONFIGS.len() as u32
}

pub(crate) unsafe extern "C" fn config_get<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    index: u32,
    out: *mut sys::ClapAudioPortsConfig,
) -> bool {
    // SAFETY: host contract (see `wrapper::host`).
    let (inst, out) = unsafe {
        (
            Instance::<P>::from_plugin_ptr(plugin),
            out_param(out, "config"),
        )
    };
    inst.answer(
        "audio_ports_config.get",
        out.and_then(|out| config_info::<P>(index, out)),
    )
}

fn config_info<P: ClapExport>(
    index: u32,
    cfg: &mut sys::ClapAudioPortsConfig,
) -> Result<(), CallError> {
    let member = <P as Plugin>::CONFIGS
        .get(index as usize)
        .ok_or(CallError::IndexOutOfRange(index.into()))?;
    cfg.id = index;
    let main_ins = if member.in_channels() > 0 { 1 } else { 0 };
    cfg.input_port_count = main_ins + member.aux_in.len() as u32;
    cfg.output_port_count = if member.out_channels() > 0 { 1 } else { 0 };
    cfg.has_main_input = member.in_channels() > 0;
    cfg.main_input_channel_count = member.in_channels();
    cfg.main_input_port_type = member.main_in.map_or(core::ptr::null(), port_type_for);
    cfg.has_main_output = member.out_channels() > 0;
    cfg.main_output_channel_count = member.out_channels();
    cfg.main_output_port_type = port_type_for(member.main_out);
    // Name: main arrangement + aux summary, e.g. "stereo + sidechain".
    let name: &str = if member.aux_in.is_empty() {
        if member.in_channels() == 1 {
            "mono"
        } else {
            "stereo"
        }
    } else {
        if member.in_channels() == 1 {
            "mono + sidechain"
        } else {
            "stereo + sidechain"
        }
    };
    write_c_str(&mut cfg.name, name);
    Ok(())
}

pub(crate) unsafe extern "C" fn config_select<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    config_id: sys::ClapId,
) -> bool {
    // SAFETY: host contract (see `wrapper::host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    let known = (config_id as usize) < <P as Plugin>::CONFIGS.len();
    if known {
        inst.config.store(config_id as usize, Ordering::Release);
    }
    inst.answer(
        "audio_ports_config.select",
        known
            .then_some(())
            .ok_or(CallError::UnknownConfigId(config_id)),
    )
}
