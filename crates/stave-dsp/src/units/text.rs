//! How units read: the shared formatting behind every unit's `Display`.
//!
//! Where a unit has a stave-core param [`Unit`] twin (Hz, dB, ms/s,
//! ratio) it reads exactly as that unit formats a param value, unit
//! switch included (`999.96 Hz` reads `1.00 kHz`, judged on the value as
//! rounded for display); `stave-core`'s tests pin the two together.
//! Circuit units read in engineering notation with three significant
//! figures and an SI prefix (`4.70 kΩ`, `22.0 nF`), the way a schematic
//! labels them.
//!
//! [`Unit`]: https://docs.rs/stave-core/latest/stave_core/params/enum.Unit.html

use core::fmt::{self, Write as _};

/// A fixed stack buffer, enough for any `{:.1}` of a value below 1e15.
struct Stack {
    buf: [u8; 40],
    len: usize,
}

impl fmt::Write for Stack {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// `v` as it reads at one decimal place, parsed back: the value a unit
/// switch is judged on (stave-core's `shown`). No allocation.
pub(crate) fn shown_1dp(v: f64) -> f64 {
    if v.abs() >= 1e15 {
        return v; // past every switch point; rounding can't matter
    }
    let mut s = Stack {
        buf: [0; 40],
        len: 0,
    };
    if write!(s, "{v:.1}").is_err() {
        return v;
    }
    core::str::from_utf8(&s.buf[..s.len])
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(v)
}

/// A value in a base unit that switches to a ×1000 unit at 1000 (Hz →
/// kHz, ms → s): `{:.1} small` below, `{:.2} big` from the switch.
pub(crate) fn switching(f: &mut fmt::Formatter<'_>, v: f64, small: &str, big: &str) -> fmt::Result {
    if v.is_finite() && shown_1dp(v) >= 1000.0 {
        write!(f, "{:.2} {big}", v / 1000.0)
    } else {
        write!(f, "{v:.1} {small}")
    }
}

/// SI prefixes from femto to giga, each with the scale it divides by.
/// Thresholds and divisors are the same constants, so a value at or
/// above `SCALES[i]` divides to at least 1.
pub(crate) const PREFIXES: [&str; 9] = ["f", "p", "n", "µ", "m", "", "k", "M", "G"];
pub(crate) const SCALES: [f64; 9] = [1e-15, 1e-12, 1e-9, 1e-6, 1e-3, 1.0, 1e3, 1e6, 1e9];

/// The prefix index for a magnitude: the largest scale at or below it,
/// clamped to the table (femto below it, giga above). Never out of
/// bounds, for every `f64` (NaN and zero pick the unprefixed unit).
#[inline]
pub(crate) fn prefix_index(magnitude: f64) -> usize {
    if magnitude.is_nan() || magnitude == 0.0 {
        return 5;
    }
    let mut i = 0;
    while i + 1 < SCALES.len() && magnitude >= SCALES[i + 1] {
        i += 1;
    }
    i
}

/// Engineering notation: three significant figures and an SI prefix
/// (`4.70 kΩ`, `22.0 nF`, `100 mH`), rounding carried into the next
/// prefix (`999.96 Ω` reads `1.00 kΩ`).
pub(crate) fn engineering(f: &mut fmt::Formatter<'_>, v: f64, symbol: &str) -> fmt::Result {
    if !v.is_finite() {
        return write!(f, "{v} {symbol}");
    }
    let mut i = prefix_index(v.abs());
    let mut scaled = v / SCALES[i];
    // Three significant figures; a value that rounds up to 1000 takes the
    // next prefix (when there is one).
    if scaled.abs() >= 999.5 && i + 1 < SCALES.len() {
        i += 1;
        scaled = v / SCALES[i];
    }
    let a = scaled.abs();
    let decimals = if a >= 99.95 {
        0
    } else if a >= 9.995 {
        1
    } else {
        2
    };
    write!(f, "{scaled:.decimals$} {}{symbol}", PREFIXES[i])
}
