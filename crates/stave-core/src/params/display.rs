//! Allocation-free value display and parsing, per unit.
//!
//! Hosts call `getParamStringByValue`-class APIs from arbitrary
//! threads at arbitrary rates; formatting must not allocate. Values
//! render into a fixed [`ValueText`] buffer. `parse` is format's
//! inverse for typed entry ("2.4k", "-6 dB", "35%") — forgiving about
//! whitespace and the unit suffix, never panicking.

use core::fmt::Write as _;

/// What a plain value MEANS, and therefore how it reads and parses.
/// Closed vocabulary like [`super::Taper`]: every unit added here gets
/// format/parse round-trip checks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Bare number, `decimals` places.
    Plain { decimals: u8 },
    /// Decibels. Values at or below `floor_db` (if set) read "-inf".
    Db { floor_db: Option<i16> },
    /// Hertz; ≥ 1000 renders as kHz ("2.40 kHz").
    Hz,
    /// Milliseconds; ≥ 1000 renders as seconds ("1.25 s").
    Ms,
    /// 0..=1 plain value rendered 0–100%.
    Percent,
    /// Compression-style ratio ("4.0:1").
    Ratio,
}

/// Fixed-capacity render target: fits every format this module emits.
pub const VALUE_TEXT_BYTES: usize = 32;

/// Stack string for formatted values. Derefs to `&str`.
#[derive(Copy, Clone, Debug)]
pub struct ValueText {
    buf: [u8; VALUE_TEXT_BYTES],
    len: u8,
}

impl Default for ValueText {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueText {
    pub(crate) fn new() -> Self {
        Self {
            buf: [0; VALUE_TEXT_BYTES],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // Only ever filled through fmt::Write with valid UTF-8.
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

impl core::ops::Deref for ValueText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl core::fmt::Write for ValueText {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let take = s.len().min(VALUE_TEXT_BYTES - self.len as usize);
        // Truncation at capacity is silent-but-bounded; capacity is
        // sized so no format in this module can hit it.
        self.buf[self.len as usize..self.len as usize + take]
            .copy_from_slice(&s.as_bytes()[..take]);
        self.len += take as u8;
        Ok(())
    }
}

impl Unit {
    /// Bare unit suffix for host fields that carry units separately
    /// from values (VST3 `ParameterInfo::units`).
    pub fn label(&self) -> &'static str {
        match self {
            Unit::Plain { .. } => "",
            Unit::Db { .. } => "dB",
            Unit::Hz => "Hz",
            Unit::Ms => "ms",
            Unit::Percent => "%",
            Unit::Ratio => ":1",
        }
    }

    /// Render `plain` for host/editor display. Never allocates, never
    /// panics; non-finite values render as "?" (they should have been
    /// stopped upstream, but display is the wrong layer to trust that).
    pub fn format(&self, plain: f64) -> ValueText {
        let mut out = ValueText::new();
        if !plain.is_finite() {
            let _ = out.write_str("?");
            return out;
        }
        let _ = match self {
            Unit::Plain { decimals } => {
                write!(out, "{:.*}", *decimals as usize, plain)
            }
            Unit::Db { floor_db } => {
                if floor_db.is_some_and(|f| plain <= f as f64) {
                    out.write_str("-inf dB")
                } else {
                    write!(out, "{plain:.1} dB")
                }
            }
            Unit::Hz => {
                if plain >= 1000.0 {
                    write!(out, "{:.2} kHz", plain / 1000.0)
                } else {
                    write!(out, "{plain:.1} Hz")
                }
            }
            Unit::Ms => {
                if plain >= 1000.0 {
                    write!(out, "{:.2} s", plain / 1000.0)
                } else {
                    write!(out, "{plain:.1} ms")
                }
            }
            Unit::Percent => write!(out, "{:.0}%", plain * 100.0),
            Unit::Ratio => write!(out, "{plain:.1}:1"),
        };
        out
    }

    /// Parse typed entry back to a plain value. Accepts what `format`
    /// emits plus common shorthand ("2k4" excluded; "2.4k", "2400",
    /// "-inf", "50 %" accepted). `None` = not parseable; callers keep
    /// the old value.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        let lower_buf = {
            // Case-fold ASCII in place — no allocation.
            let mut b = [0u8; VALUE_TEXT_BYTES];
            let take = t.len().min(VALUE_TEXT_BYTES);
            b[..take].copy_from_slice(&t.as_bytes()[..take]);
            b[..take].make_ascii_lowercase();
            b
        };
        let t = core::str::from_utf8(&lower_buf[..t.len().min(VALUE_TEXT_BYTES)]).ok()?;

        match self {
            Unit::Db { floor_db } => {
                if t.starts_with("-inf") {
                    return floor_db.map(|f| f as f64);
                }
                strip_number(t).map(|(v, _)| v)
            }
            Unit::Hz => {
                let (v, rest) = strip_number(t)?;
                if rest.trim_start().starts_with('k') {
                    Some(v * 1000.0)
                } else {
                    Some(v)
                }
            }
            Unit::Ms => {
                let (v, rest) = strip_number(t)?;
                let rest = rest.trim_start();
                // "s" alone means seconds; "ms" means milliseconds.
                if rest.starts_with('s') {
                    Some(v * 1000.0)
                } else {
                    Some(v)
                }
            }
            Unit::Percent => {
                let (v, _) = strip_number(t)?;
                Some(v / 100.0)
            }
            Unit::Plain { .. } | Unit::Ratio => strip_number(t).map(|(v, _)| v),
        }
    }
}

/// Leading float off the front of `t`; returns (value, remainder).
fn strip_number(t: &str) -> Option<(f64, &str)> {
    let bytes = t.as_bytes();
    let mut end = 0;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit()
            || bytes[end] == b'.'
            || bytes[end] == b'-'
            || bytes[end] == b'+')
    {
        end += 1;
    }
    if end == 0 {
        return None;
    }
    t[..end].parse::<f64>().ok().map(|v| (v, &t[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_matrix() {
        assert_eq!(
            &*Unit::Db {
                floor_db: Some(-60)
            }
            .format(-6.0),
            "-6.0 dB"
        );
        assert_eq!(
            &*Unit::Db {
                floor_db: Some(-60)
            }
            .format(-60.0),
            "-inf dB"
        );
        assert_eq!(&*Unit::Db { floor_db: None }.format(-60.0), "-60.0 dB");
        assert_eq!(&*Unit::Hz.format(440.0), "440.0 Hz");
        assert_eq!(&*Unit::Hz.format(2400.0), "2.40 kHz");
        assert_eq!(&*Unit::Ms.format(15.0), "15.0 ms");
        assert_eq!(&*Unit::Ms.format(1250.0), "1.25 s");
        assert_eq!(&*Unit::Percent.format(0.35), "35%");
        assert_eq!(&*Unit::Ratio.format(4.0), "4.0:1");
        // 0.125 would hit round-half-to-even ("0.12"); use a non-tie.
        assert_eq!(&*Unit::Plain { decimals: 2 }.format(0.126), "0.13");
        assert_eq!(&*Unit::Hz.format(f64::NAN), "?");
    }

    #[test]
    fn parse_accepts_format_output_and_shorthand() {
        let db = Unit::Db {
            floor_db: Some(-60),
        };
        assert_eq!(db.parse("-6.0 dB"), Some(-6.0));
        assert_eq!(db.parse("-inf dB"), Some(-60.0));
        assert_eq!(db.parse("-INF"), Some(-60.0));
        assert_eq!(db.parse("3"), Some(3.0));

        assert_eq!(Unit::Hz.parse("2.40 kHz"), Some(2400.0));
        assert_eq!(Unit::Hz.parse("2.4k"), Some(2400.0));
        assert_eq!(Unit::Hz.parse("440"), Some(440.0));

        assert_eq!(Unit::Ms.parse("1.25 s"), Some(1250.0));
        assert_eq!(Unit::Ms.parse("15 ms"), Some(15.0));

        assert_eq!(Unit::Percent.parse("35%"), Some(0.35));
        assert_eq!(Unit::Percent.parse(" 35 % "), Some(0.35));

        assert_eq!(Unit::Ratio.parse("4.0:1"), Some(4.0));
        assert_eq!(Unit::Hz.parse("fast"), None);
        assert_eq!(Unit::Hz.parse(""), None);
    }

    /// format → parse → format is stable (the typed-entry contract).
    #[test]
    fn format_parse_fixpoint() {
        let cases: &[(Unit, f64)] = &[
            (
                Unit::Db {
                    floor_db: Some(-60),
                },
                -6.0,
            ),
            (Unit::Hz, 440.0),
            (Unit::Hz, 2400.0),
            (Unit::Ms, 15.0),
            (Unit::Percent, 0.35),
            (Unit::Ratio, 4.0),
        ];
        for (unit, v) in cases {
            let shown = unit.format(*v);
            let parsed = unit
                .parse(&shown)
                .unwrap_or_else(|| panic!("parse {shown:?}"));
            assert_eq!(
                &*unit.format(parsed),
                &*shown,
                "{unit:?}: {v} → {shown:?} → {parsed} not a fixpoint"
            );
        }
    }
}
