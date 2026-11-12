//! Allocation-free value display and parsing, per unit.
//!
//! Hosts call `getParamStringByValue`-class APIs from arbitrary
//! threads at arbitrary rates; formatting must not allocate. Values
//! format into a fixed [`ValueText`] buffer. `parse` is format's
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
    /// Decibels.
    Db,
    /// Decibels, where values at or below the floor (dB) read "-inf".
    DbFloor(i16),
    /// Hertz; ≥ 1000 reads as kHz ("2.40 kHz"). The switch is judged on
    /// the value rounded as it would show in Hz, so 999.96 reads
    /// "1.00 kHz", never "1000.0 Hz".
    Hz,
    /// Milliseconds; ≥ 1000 reads as seconds ("1.25 s"), judged on the
    /// value rounded as it would show in ms (as for [`Unit::Hz`]).
    Ms,
    /// 0..=1, read as 0–100%.
    Percent,
    /// Compression-style ratio ("4.0:1").
    Ratio,
}

/// Fixed-capacity text: fits every format this module emits.
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
    /// The "-inf" floor of a [`Unit::DbFloor`].
    fn db_floor(&self) -> Option<i16> {
        match *self {
            Unit::DbFloor(f) => Some(f),
            _ => None,
        }
    }

    /// Bare unit suffix for host fields that carry units separately
    /// from values (VST3 `ParameterInfo::units`).
    pub fn label(&self) -> &'static str {
        match self {
            Unit::Plain { .. } => "",
            Unit::Db | Unit::DbFloor(_) => "dB",
            Unit::Hz => "Hz",
            Unit::Ms => "ms",
            Unit::Percent => "%",
            Unit::Ratio => ":1",
        }
    }

    /// `plain` as text for a host or a face. Never allocates, never
    /// panics; non-finite values read "?" (they should have been
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
            Unit::Db | Unit::DbFloor(_) => {
                let floor_db = self.db_floor();
                // The floor is judged on the value as shown: −59.96 reads
                // "-60.0 dB", which would parse back onto a −60 floor.
                if floor_db.is_some_and(|f| shown(plain, 1) <= f as f64) {
                    out.write_str("-inf dB")
                } else {
                    write!(out, "{plain:.1} dB")
                }
            }
            Unit::Hz => {
                if shown(plain, 1) >= 1000.0 {
                    write!(out, "{:.2} kHz", plain / 1000.0)
                } else {
                    write!(out, "{plain:.1} Hz")
                }
            }
            Unit::Ms => {
                if shown(plain, 1) >= 1000.0 {
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
            Unit::Db | Unit::DbFloor(_) => {
                let floor_db = self.db_floor();
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

/// `plain` exactly as `{:.decimals$}` formats it, read back: unit
/// switches (ms → s, Hz → kHz, the dB floor) are decided on the shown
/// value, so a value that rounds onto a boundary reads the same as the
/// boundary itself and format → parse → format is a fixpoint.
fn shown(plain: f64, decimals: usize) -> f64 {
    if plain.abs() >= 1e15 {
        return plain; // past every switch point; rounding can't matter
    }
    let mut text = ValueText::new();
    let _ = write!(text, "{plain:.decimals$}");
    text.parse::<f64>().unwrap_or(plain)
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
            &*Unit::DbFloor(-60)
            .format(-6.0),
            "-6.0 dB"
        );
        assert_eq!(
            &*Unit::DbFloor(-60)
            .format(-60.0),
            "-inf dB"
        );
        assert_eq!(&*Unit::Db.format(-60.0), "-60.0 dB");
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
        let db = Unit::DbFloor(-60);
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
    // Safe-code sweeps: Miri adds nothing but hours.
    #[cfg_attr(miri, ignore)]
    fn format_parse_fixpoint() {
        let cases: &[(Unit, f64)] = &[
            (
                Unit::DbFloor(-60),
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

    /// Every unit kind, every value across its range — including each
    /// switch point (ms→s, Hz→kHz, the dB floor) ± a few ulps and ± the
    /// display's half-step — is a format → parse → format fixpoint.
    /// (S23: saga's ramp at 999.9999999999999 read "1000.0 ms", which
    /// parsed back to 1000 and re-read "1.00 s".)
    #[test]
    // Safe-code sweeps: Miri adds nothing but hours.
    #[cfg_attr(miri, ignore)]
    fn every_unit_round_trips_across_its_range() {
        let units = [
            Unit::Plain { decimals: 0 },
            Unit::Plain { decimals: 1 },
            Unit::Plain { decimals: 2 },
            Unit::Plain { decimals: 4 },
            Unit::Db,
            Unit::DbFloor(-60),
            Unit::DbFloor(-100),
            Unit::DbFloor(0),
            Unit::Hz,
            Unit::Ms,
            Unit::Percent,
            Unit::Ratio,
        ];
        // Sweep: a dense linear run around zero plus a geometric run out
        // to 1e6 on both signs.
        // Miri interprets every format and parse, so it walks a sparse
        // sweep; the switch points below stay exhaustive.
        let (stride, growth) = if cfg!(miri) { (500, 1.05) } else { (1, 1.0007) };
        let mut values: Vec<f64> =
            (-20_000..=20_000).step_by(stride).map(|i| i as f64 * 0.0137).collect();
        let mut v = 1e-4;
        while v < 1e6 {
            values.push(v);
            values.push(-v);
            v *= growth;
        }
        // Switch points and rounding edges, each nudged by ulps and by
        // the display's half-step.
        let edges = [
            0.0, 0.005, 0.05, 0.5, 1.0, 9.995, 99.95, 999.95, 1000.0, 1000.05, 9999.5, 10_000.0,
            -60.0, -59.95, -60.05, -100.0, -99.95,
        ];
        for edge in edges {
            for delta in [0.0, 1e-12, 1e-9, 1e-6, 0.004, 0.005, 0.049, 0.05, 0.051] {
                for x in [edge + delta, edge - delta] {
                    let mut y = x;
                    for _ in 0..4 {
                        values.push(y);
                        y = f64::from_bits(y.to_bits() + 1);
                    }
                    let mut y = x;
                    for _ in 0..4 {
                        values.push(y);
                        y = f64::from_bits(y.to_bits().wrapping_sub(1));
                    }
                }
            }
        }
        values.push(999.9999999999999);
        for unit in units {
            for &v in &values {
                if !v.is_finite() {
                    continue;
                }
                let shown = unit.format(v);
                let parsed = unit
                    .parse(&shown)
                    .unwrap_or_else(|| panic!("{unit:?}: {v} → {shown:?} does not parse"));
                let again = unit.format(parsed);
                assert_eq!(
                    &*again, &*shown,
                    "{unit:?}: {v} → {shown:?} → {parsed} → {again:?}"
                );
            }
        }
    }

    /// Both readings of a switch point parse to the same value.
    #[test]
    // Safe-code sweeps: Miri adds nothing but hours.
    #[cfg_attr(miri, ignore)]
    fn both_sides_of_a_unit_switch_parse() {
        assert_eq!(Unit::Ms.parse("1000.0 ms"), Some(1000.0));
        assert_eq!(Unit::Ms.parse("1.00 s"), Some(1000.0));
        assert_eq!(Unit::Hz.parse("1000.0 Hz"), Some(1000.0));
        assert_eq!(Unit::Hz.parse("1.00 kHz"), Some(1000.0));
        assert_eq!(&*Unit::Ms.format(999.9999999999999), "1.00 s");
        assert_eq!(&*Unit::Ms.format(999.94), "999.9 ms");
        assert_eq!(&*Unit::Hz.format(999.96), "1.00 kHz");
        let db = Unit::DbFloor(-60);
        assert_eq!(&*db.format(-59.96), "-inf dB");
        assert_eq!(&*db.format(-59.94), "-59.9 dB");
    }
}
