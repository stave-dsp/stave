//! An Audio Unit's identity, decided at compile time from the plugin's
//! [`Info`] and [`Layout`]s: its component type, its manufacturer and
//! subtype codes, its version number, and the `AudioComponents` entry of
//! its bundle's `Info.plist`. Portable: these are facts about the
//! plugin, checked on every OS.

use stave_core::{AuCodes, Feature, Info, Layout};

use crate::AuExport;

/// The factory function the entry names (`export_au!` defines it).
pub const FACTORY: &str = "StaveAUFactory";

/// The type, subtype and manufacturer hosts find the component by.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// `aumu` (instrument), `aumf` (effect that takes notes) or `aufx`.
    pub kind: [u8; 4],
    pub subtype: [u8; 4],
    pub manufacturer: [u8; 4],
}

impl Identity {
    /// `P`'s identity.
    pub const fn of<P: AuExport>() -> Self {
        Self::from_info(&P::INFO, P::LAYOUTS)
    }

    pub const fn from_info(info: &Info, layouts: &[Layout]) -> Self {
        let codes = codes(info);
        Identity {
            kind: kind(info, layouts),
            subtype: codes.subtype,
            manufacturer: codes.manufacturer,
        }
    }

    /// The three codes as `auval -v` takes them.
    pub fn auval_args(&self) -> [String; 3] {
        [self.kind, self.subtype, self.manufacturer].map(|c| String::from_utf8_lossy(&c).into_owned())
    }
}

/// An instrument (`aumu`) takes notes and no audio input, or says it is
/// one; an effect that takes notes is a music effect (`aumf`); any other
/// effect is `aufx`.
pub const fn kind(info: &Info, layouts: &[Layout]) -> [u8; 4] {
    let (mut instrument, mut notes) = (false, false);
    let mut i = 0;
    while i < info.features.len() {
        match info.features[i] {
            Feature::Instrument | Feature::Synthesizer | Feature::Sampler | Feature::Drum => instrument = true,
            Feature::NoteEffect => notes = true,
            _ => {}
        }
        i += 1;
    }
    let no_input = layouts.is_empty() || layouts[0].input.is_none();
    if instrument || no_input {
        *b"aumu"
    } else if notes {
        *b"aumf"
    } else {
        *b"aufx"
    }
}

/// Whether the plugin takes notes (an instrument or a music effect).
pub const fn takes_notes(kind: [u8; 4]) -> bool {
    matches!(&kind, b"aumu" | b"aumf")
}

/// `info.au`, or codes derived from it: the manufacturer from the vendor
/// (its first four letters and digits, the first a capital: "Ryan Bahan"
/// is `Ryan`), the subtype from a hash of the id (stable forever, as the
/// id is). Pin them with `Info::au` before shipping: two vendors whose
/// names start alike derive the same manufacturer.
pub const fn codes(info: &Info) -> AuCodes {
    match info.au {
        Some(codes) => codes,
        None => AuCodes {
            manufacturer: derived_manufacturer(info.vendor),
            subtype: derived_subtype(info.id),
        },
    }
}

/// FNV-1a over `bytes`.
const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// Four characters of `alphabet` from `h`.
const fn spell(mut h: u32, alphabet: &[u8]) -> [u8; 4] {
    let mut out = [0u8; 4];
    let mut i = 0;
    while i < 4 {
        out[i] = alphabet[(h % alphabet.len() as u32) as usize];
        h /= alphabet.len() as u32;
        i += 1;
    }
    out
}

const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

const fn derived_subtype(id: &str) -> [u8; 4] {
    spell(fnv1a(id.as_bytes()), LOWER)
}

const fn derived_manufacturer(vendor: &str) -> [u8; 4] {
    let bytes = vendor.as_bytes();
    let fill = spell(fnv1a(bytes), UPPER);
    let mut out = fill;
    let (mut n, mut i) = (0, 0);
    while i < bytes.len() && n < 4 {
        if bytes[i].is_ascii_alphanumeric() {
            out[n] = if n == 0 { bytes[i].to_ascii_uppercase() } else { bytes[i] };
            n += 1;
        }
        i += 1;
    }
    // A leading digit leaves no capital: Apple reserves those codes.
    if !(out[0].is_ascii_uppercase()
        || out[1].is_ascii_uppercase()
        || out[2].is_ascii_uppercase()
        || out[3].is_ascii_uppercase())
    {
        out[3] = fill[3];
    }
    out
}

/// `major.minor.patch` as the component's version number,
/// `0xMMMMmmpp` (minor and patch held to 255); unparsable parts are 0.
pub const fn version_number(version: &str) -> u32 {
    let bytes = version.as_bytes();
    let mut parts = [0u32; 3];
    let (mut part, mut i) = (0, 0);
    while i < bytes.len() && part < 3 {
        let b = bytes[i];
        if b == b'.' {
            part += 1;
        } else if b.is_ascii_digit() {
            parts[part] = parts[part].saturating_mul(10).saturating_add((b - b'0') as u32);
        } else {
            break; // a pre-release suffix ("-dev") ends the number
        }
        i += 1;
    }
    let minor = if parts[1] > 255 { 255 } else { parts[1] };
    let patch = if parts[2] > 255 { 255 } else { parts[2] };
    (parts[0] & 0xFFFF) << 16 | minor << 8 | patch
}

/// The room the `Info.plist` entry has in the binary.
pub const ENTRY_BYTES: usize = 4096;

/// The `AudioComponents` entry (one `<dict>`), NUL-padded to
/// [`ENTRY_BYTES`].
pub struct Entry {
    buf: [u8; ENTRY_BYTES],
    len: usize,
}

impl Entry {
    const fn new() -> Self {
        Entry { buf: [0; ENTRY_BYTES], len: 0 }
    }

    const fn raw(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            assert!(self.len < ENTRY_BYTES - 1, "the Audio Unit's Info.plist entry is too long");
            self.buf[self.len] = s[i];
            self.len += 1;
            i += 1;
        }
    }

    /// `s` as XML character data.
    const fn text(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            match s[i] {
                b'&' => self.raw(b"&amp;"),
                b'<' => self.raw(b"&lt;"),
                b'>' => self.raw(b"&gt;"),
                b => self.raw(&[b]),
            }
            i += 1;
        }
    }

    const fn string(&mut self, key: &str, value: &[u8]) {
        self.raw(b"<key>");
        self.raw(key.as_bytes());
        self.raw(b"</key><string>");
        self.text(value);
        self.raw(b"</string>");
    }

    const fn integer(&mut self, key: &str, mut value: u32) {
        self.raw(b"<key>");
        self.raw(key.as_bytes());
        self.raw(b"</key><integer>");
        let mut digits = [0u8; 10];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.raw(&[digits[n]]);
        }
        self.raw(b"</integer>");
    }

    /// The entry, NUL-padded.
    pub const fn bytes(&self) -> [u8; ENTRY_BYTES] {
        self.buf
    }

    /// The entry's text.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).expect("built from UTF-8")
    }
}

/// The browser tag for a feature, if Apple's list has one.
const fn tag(feature: Feature) -> Option<&'static str> {
    Some(match feature {
        Feature::Synthesizer => "Synthesizer",
        Feature::Sampler => "Sampler",
        Feature::Drum => "Drums",
        Feature::Filter => "Filter",
        Feature::Delay => "Delay",
        Feature::Reverb => "Reverb",
        Feature::Distortion => "Distortion",
        Feature::Eq => "EQ",
        Feature::Mastering => "Mastering",
        Feature::Analyzer => "Analyzer",
        _ => return None,
    })
}

/// `P`'s `AudioComponents` entry.
pub const fn entry<P: AuExport>() -> Entry {
    entry_for(&P::INFO, P::LAYOUTS, P::VERSION)
}

/// The entry for a plugin described by `info` and `layouts` at
/// `version`.
pub const fn entry_for(info: &Info, layouts: &[Layout], version: &str) -> Entry {
    let id = Identity::from_info(info, layouts);
    let mut e = Entry::new();
    e.raw(b"<dict>");
    e.string("type", &id.kind);
    e.string("subtype", &id.subtype);
    e.string("manufacturer", &id.manufacturer);
    // Hosts show "Vendor: Name", grouped by the vendor.
    e.raw(b"<key>name</key><string>");
    e.text(info.vendor.as_bytes());
    e.raw(b": ");
    e.text(info.name.as_bytes());
    e.raw(b"</string>");
    let description = if info.description.is_empty() { info.name } else { info.description };
    e.string("description", description.as_bytes());
    e.integer("version", version_number(version));
    e.string("factoryFunction", FACTORY.as_bytes());
    // Nothing outside the bundle and the user's log folder is touched.
    e.raw(b"<key>sandboxSafe</key><true/>");
    e.raw(b"<key>tags</key><array>");
    if !matches!(&id.kind, b"aumu") {
        e.string_item("Effects");
    }
    let mut i = 0;
    while i < info.features.len() {
        if let Some(t) = tag(info.features[i]) {
            e.string_item(t);
        }
        i += 1;
    }
    e.raw(b"</array>");
    e.raw(b"</dict>");
    e
}

impl Entry {
    const fn string_item(&mut self, value: &str) {
        self.raw(b"<string>");
        self.text(value.as_bytes());
        self.raw(b"</string>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILT: Info = Info {
        id: "com.ryanbahan.stave.tilt",
        name: "Stave Tilt",
        vendor: "Ryan Bahan",
        features: &[Feature::AudioEffect, Feature::Eq],
        ..Info::EFFECT
    };

    #[test]
    fn the_kind_follows_the_plugin() {
        assert_eq!(&kind(&TILT, &[Layout::STEREO]), b"aufx");
        assert_eq!(&kind(&Info::INSTRUMENT, &[Layout::INSTRUMENT]), b"aumu");
        // No audio input is an instrument whatever it says.
        assert_eq!(&kind(&TILT, &[Layout::INSTRUMENT]), b"aumu");
        let arp = Info { features: &[Feature::NoteEffect], ..TILT };
        assert_eq!(&kind(&arp, &[Layout::STEREO]), b"aumf");
    }

    #[test]
    fn derived_codes_are_readable_stable_and_never_apple_reserved() {
        let c = codes(&TILT);
        assert_eq!(&c.manufacturer, b"Ryan");
        // Pinned: the derivation is an identity every shipped plugin
        // that never set `Info::au` depends on.
        assert_eq!(&c.subtype, &derived_subtype("com.ryanbahan.stave.tilt"));
        assert_eq!(codes(&TILT), codes(&TILT));
        assert!(c.subtype.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
        for vendor in ["", "a", "42 audio", "ÉCOLE", "x-y"] {
            let m = derived_manufacturer(vendor);
            assert!(m.iter().all(u8::is_ascii_alphanumeric), "{vendor}: {m:?}");
            assert!(m.iter().any(u8::is_ascii_uppercase), "{vendor}: {m:?}");
        }
        assert_eq!(&derived_manufacturer("acme audio"), b"Acme");
    }

    #[test]
    fn pinned_codes_win() {
        let pinned = Info { au: Some(AuCodes::new(*b"Stav", *b"tilt")), ..TILT };
        let id = Identity::from_info(&pinned, &[Layout::STEREO]);
        assert_eq!((&id.manufacturer, &id.subtype), (b"Stav", b"tilt"));
        assert_eq!(id.auval_args(), ["aufx", "tilt", "Stav"]);
    }

    #[test]
    fn version_numbers_pack_major_minor_patch() {
        assert_eq!(version_number("0.4.0"), 0x0000_0400);
        assert_eq!(version_number("1.2.3"), 0x0001_0203);
        assert_eq!(version_number("2.10.300-dev"), 0x0002_0AFF);
        assert_eq!(version_number(""), 0);
    }

    #[test]
    fn the_entry_is_a_well_formed_audio_components_dict() {
        let info = Info { description: "Tips <lows> & highs", ..TILT };
        let e = entry_for(&info, &[Layout::STEREO], "0.4.0");
        let s = e.as_str();
        let sub = String::from_utf8(codes(&TILT).subtype.to_vec()).unwrap();
        for want in [
            "<key>type</key><string>aufx</string>".to_string(),
            format!("<key>subtype</key><string>{sub}</string>"),
            "<key>manufacturer</key><string>Ryan</string>".into(),
            "<key>name</key><string>Ryan Bahan: Stave Tilt</string>".into(),
            "<key>description</key><string>Tips &lt;lows&gt; &amp; highs</string>".into(),
            "<key>version</key><integer>1024</integer>".into(),
            "<key>factoryFunction</key><string>StaveAUFactory</string>".into(),
            "<string>Effects</string><string>EQ</string>".into(),
        ] {
            assert!(s.contains(&want), "{want} in {s}");
        }
        assert!(s.starts_with("<dict>") && s.ends_with("</dict>"));
        assert!(e.bytes()[s.len()..].iter().all(|&b| b == 0), "NUL-padded");
    }
}
