//! CLAP audio-ports extension: ports are DERIVED from the
//! plugin's declared `Layout` — main + aux inputs in declaration
//! order, names from the `AuxIn` declarations. audio-ports-config
//! (the declared SET) joins here with negotiation.

use core::ffi::c_char;

use stave_core::runtime::Erased;

use crate::sys;
use crate::wrapper::{ffi_entry, out_param, write_c_str, ClapExport, Instance};
use stave_core::diagnostics::CallError;

use core::sync::atomic::Ordering;

use stave_core::Layout;

/// The instance's host-selected configuration.
pub(crate) fn selected<P: ClapExport>(inst: &Instance<P>) -> Layout {
    let idx = inst.config.load(Ordering::Acquire);
    P::LAYOUTS[idx.min(P::LAYOUTS.len() - 1)]
}

pub(crate) unsafe extern "C" fn audio_ports_count<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    is_input: bool,
) -> u32 {
    // SAFETY: host contract (see `wrapper::host`).
    let inst = unsafe { Instance::<P>::from_plugin_ptr(plugin) };
    ffi_entry::<P, _>(
        || 0,
        || {
            let ports = selected(inst);
            if is_input {
                // Main (if any) + declared aux ports, in declaration order.
                let main = if ports.input_channels() > 0 { 1 } else { 0 };
                main + ports.aux.len() as u32
            } else if ports.output_channels() > 0 {
                1
            } else {
                0
            }
        },
    )
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
    ffi_entry::<P, _>(
        || false,
        || {
            inst.answer(
                "audio_ports.get",
                info.and_then(|info| port_info(selected(inst), index, is_input, info)),
            )
        },
    )
}

fn port_info(
    ports: Layout,
    index: u32,
    is_input: bool,
    out: &mut sys::ClapAudioPortInfo,
) -> Result<(), CallError> {
    let (arrangement, flags, name): (stave_core::Channels, u32, &str) = if is_input {
        let main = if ports.input.is_some() { 1 } else { 0 };
        if index < main {
            (
                ports.input.expect("main == 1"),
                sys::CLAP_AUDIO_PORT_IS_MAIN,
                "main",
            )
        } else {
            // Aux ports follow the main input, declaration order.
            let aux = ports
                .aux
                .get((index - main) as usize)
                .ok_or(CallError::IndexOutOfRange(index.into()))?;
            (aux.channels, 0, aux.name)
        }
    } else {
        if index != 0 || ports.output_channels() == 0 {
            return Err(CallError::IndexOutOfRange(index.into()));
        }
        (ports.output, sys::CLAP_AUDIO_PORT_IS_MAIN, "main")
    };

    out.id = index;
    out.flags = flags;
    out.channel_count = arrangement.count() as u32;
    out.port_type = port_type_for(arrangement);
    out.in_place_pair = sys::CLAP_INVALID_ID;

    write_c_str(&mut out.name, name);
    Ok(())
}

/// The semantic map: CLAP port-type string per Channels.
/// Surround has NO well-known core type string — null is the legal
/// "unspecified" value; hosts read channel_count (the CLAP surround
/// extension can add the channel map when a host demands one).
fn port_type_for(a: stave_core::Channels) -> *const c_char {
    match a {
        stave_core::Channels::Mono => sys::CLAP_PORT_MONO.as_ptr(),
        stave_core::Channels::Stereo => sys::CLAP_PORT_STEREO.as_ptr(),
        stave_core::Channels::Surround51 => core::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// audio-ports-config: the declared SET, host-selectable
// ---------------------------------------------------------------------------

pub(crate) unsafe extern "C" fn config_count<P: ClapExport>(
    _plugin: *const sys::ClapPlugin,
) -> u32 {
    P::LAYOUTS.len() as u32
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
    ffi_entry::<P, _>(
        || false,
        || {
            inst.answer(
                "audio_ports_config.get",
                out.and_then(|out| config_info::<P>(index, out)),
            )
        },
    )
}

fn config_info<P: ClapExport>(
    index: u32,
    cfg: &mut sys::ClapAudioPortsConfig,
) -> Result<(), CallError> {
    let member = P::LAYOUTS
        .get(index as usize)
        .ok_or(CallError::IndexOutOfRange(index.into()))?;
    cfg.id = index;
    let main_ins = if member.input_channels() > 0 { 1 } else { 0 };
    cfg.input_port_count = main_ins + member.aux.len() as u32;
    cfg.output_port_count = if member.output_channels() > 0 { 1 } else { 0 };
    cfg.has_main_input = member.input_channels() > 0;
    cfg.main_input_channel_count = member.input_channels() as u32;
    cfg.main_input_port_type = member.input.map_or(core::ptr::null(), port_type_for);
    cfg.has_main_output = member.output_channels() > 0;
    cfg.main_output_channel_count = member.output_channels() as u32;
    cfg.main_output_port_type = port_type_for(member.output);
    // Name: main arrangement + aux summary, e.g. "stereo + sidechain".
    let name: &str = if member.aux.is_empty() {
        if member.input_channels() == 1 {
            "mono"
        } else {
            "stereo"
        }
    } else {
        if member.input_channels() == 1 {
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
    ffi_entry::<P, _>(
        || false,
        || {
            let known = (config_id as usize) < P::LAYOUTS.len();
            if known {
                inst.config.store(config_id as usize, Ordering::Release);
            }
            inst.answer(
                "audio_ports_config.select",
                known
                    .then_some(())
                    .ok_or(CallError::UnknownConfigId(config_id)),
            )
        },
    )
}

// ---------------------------------------------------------------------------
// note-ports: one CLAP-dialect input for a plugin that takes notes
// ---------------------------------------------------------------------------

/// Whether `P` takes notes: an instrument (no main audio input, or
/// declared one) or a note effect. Only then does it offer note ports —
/// without one, a CLAP host sends it no notes.
pub(crate) fn takes_notes<P: Erased>() -> bool {
    use stave_core::Feature as F;
    let declared = P::INFO
        .features
        .iter()
        .any(|f| matches!(f, F::Instrument | F::Synthesizer | F::Sampler | F::Drum | F::NoteEffect));
    declared || P::LAYOUTS.iter().any(|c| c.input.is_none())
}

pub(crate) unsafe extern "C" fn note_ports_count<P: ClapExport>(_plugin: *const sys::ClapPlugin, is_input: bool) -> u32 {
    u32::from(is_input && takes_notes::<P>())
}

pub(crate) unsafe extern "C" fn note_ports_get<P: ClapExport>(
    plugin: *const sys::ClapPlugin,
    index: u32,
    is_input: bool,
    info: *mut sys::ClapNotePortInfo,
) -> bool {
    // SAFETY: host contract (see `wrapper::host`); `info` is null or
    // writable (clap.h).
    let (inst, info) = unsafe { (Instance::<P>::from_plugin_ptr(plugin), out_param(info, "info")) };
    ffi_entry::<P, _>(
        || false,
        || {
            let result = info.and_then(|info| {
                if !is_input || index != 0 || !takes_notes::<P>() {
                    return Err(CallError::IndexOutOfRange(index.into()));
                }
                // The wrapper reads CLAP note events (on, off, choke);
                // hosts translate MIDI into them.
                info.id = 0;
                info.supported_dialects = sys::CLAP_NOTE_DIALECT_CLAP;
                info.preferred_dialect = sys::CLAP_NOTE_DIALECT_CLAP;
                write_c_str(&mut info.name, "notes");
                Ok(())
            });
            inst.answer("note_ports.get", result)
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use stave_core::params::{NoParams, Params};
    use stave_core::runtime::Erased;
    use stave_core::{Block, Feature, Info, Layout, Telemetry};

    const fn info(features: &'static [Feature]) -> Info {
        Info {
            id: "com.stave.test.notes",
            name: "Notes",
            vendor: "stave tests",
            features,
            ..Info::EFFECT
        }
    }

    macro_rules! plugin {
        ($name:ident, $features:expr, $layouts:expr) => {
            struct $name;
            impl Erased for $name {
                const INFO: Info = info($features);
                const LAYOUTS: &'static [Layout] = $layouts;
                fn new(_: &Telemetry) -> Self {
                    $name
                }
                fn params(&self) -> Arc<dyn Params> {
                    Arc::new(NoParams)
                }
                fn process(&mut self, _block: &mut Block<'_>) {}
            }
        };
    }

    plugin!(Synth, &[], &[Layout::INSTRUMENT]);
    plugin!(Arp, &[Feature::NoteEffect], &[Layout::STEREO]);
    plugin!(Fx, &[Feature::AudioEffect], &[Layout::STEREO]);

    /// An instrument (no audio in) or a declared note effect gets a note
    /// port, so CLAP hosts send it notes; an effect does not.
    #[test]
    fn instruments_and_note_effects_take_notes() {
        assert!(super::takes_notes::<Synth>());
        assert!(super::takes_notes::<Arp>());
        assert!(!super::takes_notes::<Fx>());
    }
}
