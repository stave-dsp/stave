//! The DSP library's units read exactly as the param units they twin:
//! `Hz` as `Unit::Hz`, `Db` as `Unit::Db`, `Seconds` as `Unit::Ms` (of
//! its milliseconds), `Ratio` as `Unit::Ratio`, unit switches included.
//! Swept over seeded values across every decade and around each switch
//! point, so the two formatters cannot drift apart.

use stave_core::params::Unit;
use stave_dsp::units::{db, hz, seconds, Ratio, SampleRate};

/// SplitMix64, seeded.
fn stream(mut state: u64) -> impl FnMut() -> f64 {
    move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Values across decades, signs, and the 1000 switch point ± a little.
fn values() -> Vec<f64> {
    let mut next = stream(7);
    let mut out = Vec::new();
    for _ in 0..5_000 {
        let decade = (next() * 14.0 - 6.0).round();
        let v = 10f64.powf(decade + next());
        out.push(if next() < 0.5 { v } else { -v });
    }
    for edge in [
        999.9,
        999.94,
        999.95,
        999.949_999,
        999.950_001,
        999.96,
        1000.0,
        1000.04,
    ] {
        out.push(edge);
        out.push(-edge);
    }
    out.push(0.0);
    out
}

#[test]
fn hz_reads_as_unit_hz() {
    for v in values() {
        let f = v as f32;
        assert_eq!(hz(f).to_string(), &*Unit::Hz.format(f as f64), "{f}");
        if f > 0.0 {
            assert_eq!(SampleRate::new(f).to_string(), &*Unit::Hz.format(f as f64));
        }
    }
}

#[test]
fn db_reads_as_unit_db() {
    for v in values() {
        let d = v as f32;
        assert_eq!(db(d).to_string(), &*Unit::Db.format(d as f64), "{d}");
    }
}

#[test]
fn seconds_read_as_unit_ms() {
    for v in values() {
        let t = seconds(v);
        assert_eq!(t.to_string(), &*Unit::Ms.format(t.to_ms()), "{v}");
    }
}

#[test]
fn ratio_reads_as_unit_ratio() {
    for v in values() {
        let r = v.abs() as f32;
        if r > 0.0 {
            assert_eq!(
                Ratio::new(r).to_string(),
                &*Unit::Ratio.format(r as f64),
                "{r}"
            );
        }
    }
}
