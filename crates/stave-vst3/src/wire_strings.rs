//! Wire-format string helpers: nul-terminated C strings, UTF-16
//! String128 fields, and VST3 subcategory concatenation.

use core::ffi::{c_char, CStr};

use stave_core::{Plugin, PluginFeature};

pub(crate) fn write_cstr_into(dst: &mut [c_char], src: &CStr) {
    let bytes = src.to_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    for i in 0..n {
        dst[i] = bytes[i] as c_char;
    }
    if !dst.is_empty() {
        dst[n] = 0;
    }
}

pub(crate) fn write_utf16_name(dst: &mut [u16], src: &str) {
    let max = dst.len().saturating_sub(1);
    let mut i = 0;
    for c in src.encode_utf16() {
        if i >= max {
            break;
        }
        dst[i] = c;
        i += 1;
    }
    if !dst.is_empty() {
        dst[i] = 0;
    }
}

pub(crate) fn write_subcategories<P: Plugin>(dst: &mut [c_char]) {
    // Concatenate feature strings into "Cat1|Cat2|...". Bounded by dst len.
    let mut pos = 0;
    let mut first = true;
    for f in <P as Plugin>::DESCRIPTOR.features {
        let bytes = vst3_subcategory_bytes(*f);
        if bytes.is_empty() {
            continue;
        }
        if !first {
            if pos + 1 >= dst.len() {
                break;
            }
            dst[pos] = b'|' as c_char;
            pos += 1;
        }
        for &b in bytes {
            if pos + 1 >= dst.len() {
                break;
            }
            dst[pos] = b as c_char;
            pos += 1;
        }
        first = false;
    }
    if pos < dst.len() {
        dst[pos] = 0;
    }
}

pub(crate) fn vst3_subcategory_bytes(f: PluginFeature) -> &'static [u8] {
    match f {
        PluginFeature::Instrument | PluginFeature::Synthesizer => b"Instrument|Synth",
        PluginFeature::Sampler => b"Instrument|Sampler",
        PluginFeature::Drum => b"Instrument|Drum",
        PluginFeature::AudioEffect => b"Fx",
        PluginFeature::Filter => b"Fx|Filter",
        PluginFeature::Delay => b"Fx|Delay",
        PluginFeature::Reverb => b"Fx|Reverb",
        PluginFeature::Distortion => b"Fx|Distortion",
        PluginFeature::Eq => b"Fx|EQ",
        PluginFeature::Analyzer => b"Analyzer",
        PluginFeature::Stereo => b"Stereo",
        PluginFeature::Mono => b"Mono",
        _ => b"",
    }
}

pub(crate) fn cstr_to_str(s: &CStr) -> &str {
    s.to_str().unwrap_or("")
}

/// Feature classification shared by bus/category logic.
pub(crate) fn plugin_is_instrument<P: Plugin>() -> bool {
    for f in <P as Plugin>::DESCRIPTOR.features {
        if matches!(
            f,
            PluginFeature::Instrument
                | PluginFeature::Synthesizer
                | PluginFeature::Sampler
                | PluginFeature::Drum
        ) {
            return true;
        }
    }
    false
}
