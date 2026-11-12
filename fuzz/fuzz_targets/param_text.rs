//! Value text typed by a person (a host's text field, a face's entry):
//! any text, any unit — parse never panics, and a parsed finite value
//! formats to text that parses back to text that formats the same (the
//! display fixed point hosts and validators check).
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_core::params::Unit;

fuzz_target!(|input: (u8, i16, &str)| {
    let (kind, n, text) = input;
    let unit = match kind % 8 {
        0 => Unit::Plain { decimals: (n.unsigned_abs() % 7) as u8 },
        1 => Unit::Db,
        2 => Unit::DbFloor(n.clamp(-200, 0)),
        3 => Unit::Hz,
        4 => Unit::Ms,
        5 => Unit::Percent,
        6 => Unit::Ratio,
        _ => Unit::Plain { decimals: 2 },
    };
    let Some(v) = unit.parse(text) else { return };
    if !v.is_finite() {
        return;
    }
    let shown = unit.format(v);
    let again = unit
        .parse(&shown)
        .unwrap_or_else(|| panic!("{unit:?}: {text:?} → {v} → {:?} does not parse", &*shown));
    assert_eq!(
        &*unit.format(again),
        &*shown,
        "{unit:?}: {text:?} → {v} → {:?} → {again}",
        &*shown
    );
});
