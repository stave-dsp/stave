//! A plugin's VST3 class ids, derived from its `Info::id`: stable for as
//! long as the id is, with nothing for an author to generate or paste.

use crate::sys::{make_tuid, Tuid};

/// The class id for `role` (`"component"`, `"controller"`) of the plugin
/// whose `Info::id` is `id`: a 128-bit FNV-1a hash of
/// `"stave.vst3/" + role + "/" + id`, marked as an RFC 9562 version 8
/// (custom) UUID. Pinned by a test — changing it orphans every saved
/// session of every plugin.
pub const fn derive_cid(id: &str, role: &str) -> Tuid {
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    let parts: [&[u8]; 4] = [b"stave.vst3/", role.as_bytes(), b"/", id.as_bytes()];
    let mut p = 0;
    while p < parts.len() {
        let bytes = parts[p];
        let mut i = 0;
        while i < bytes.len() {
            h ^= bytes[i] as u128;
            h = h.wrapping_mul(PRIME);
            i += 1;
        }
        p += 1;
    }
    let w = [(h >> 96) as u32, (h >> 64) as u32, (h >> 32) as u32, h as u32];
    // Version 8 (bits 48–51) and the RFC variant (bits 64–65).
    let w1 = (w[1] & 0xFFFF_0FFF) | 0x0000_8000;
    let w2 = (w[2] & 0x3FFF_FFFF) | 0x8000_0000;
    make_tuid(w[0], w1, w2, w[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derivation is forever: these bytes are what hosts saved.
    #[test]
    fn class_ids_are_pinned() {
        let c = derive_cid("com.example.tilt", "component");
        let e = derive_cid("com.example.tilt", "controller");
        assert_ne!(c, e);
        assert_ne!(c, derive_cid("com.example.tilt2", "component"));
        // Big-endian words (the non-Windows layout) of the pinned id.
        #[cfg(not(windows))]
        assert_eq!(
            c,
            [
                0x65, 0x61, 0x82, 0xc9, 0x88, 0x06, 0x81, 0xb7, 0x88, 0x3b, 0x83, 0x69, 0x53, 0x0d,
                0xcc, 0x47
            ],
            "{c:02x?}"
        );
    }
}
