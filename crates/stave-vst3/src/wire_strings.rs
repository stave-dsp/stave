//! Wire-format string helpers: nul-terminated C strings, UTF-16
//! String128 fields, and VST3 subcategory concatenation.

use core::ffi::{c_char, CStr};

use stave_core::runtime::Erased;
use stave_core::Feature;

/// Text a host field takes: a `str` or a `CStr`, written without its nul.
pub(crate) trait WireText {
    fn wire_bytes(&self) -> &[u8];
}

impl WireText for str {
    fn wire_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl WireText for CStr {
    fn wire_bytes(&self) -> &[u8] {
        self.to_bytes()
    }
}

pub(crate) fn write_cstr_into<S: WireText + ?Sized>(dst: &mut [c_char], src: &S) {
    let bytes = src.wire_bytes();
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

pub(crate) fn write_subcategories<P: Erased>(dst: &mut [c_char]) {
    // Concatenate feature strings into "Cat1|Cat2|...". Bounded by dst len.
    let mut pos = 0;
    let mut first = true;
    for f in P::INFO.features {
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

pub(crate) fn vst3_subcategory_bytes(f: Feature) -> &'static [u8] {
    match f {
        Feature::Instrument | Feature::Synthesizer => b"Instrument|Synth",
        Feature::Sampler => b"Instrument|Sampler",
        Feature::Drum => b"Instrument|Drum",
        Feature::AudioEffect => b"Fx",
        Feature::Filter => b"Fx|Filter",
        Feature::Delay => b"Fx|Delay",
        Feature::Reverb => b"Fx|Reverb",
        Feature::Distortion => b"Fx|Distortion",
        Feature::Eq => b"Fx|EQ",
        Feature::Analyzer => b"Analyzer",
        Feature::Stereo => b"Stereo",
        Feature::Mono => b"Mono",
        _ => b"",
    }
}

/// Feature classification shared by bus/category logic.
pub(crate) fn plugin_is_instrument<P: Erased>() -> bool {
    for f in P::INFO.features {
        if matches!(
            f,
            Feature::Instrument
                | Feature::Synthesizer
                | Feature::Sampler
                | Feature::Drum
        ) {
            return true;
        }
    }
    false
}
