//! Units: property tests over seeded random values (round trips and
//! conversions against their definitions and the crate's existing
//! helpers), the Display text, the debug-only range checks, and the
//! zero-cost layout.

use super::*;
use crate::primitives::gain::{amp_to_db, db_to_amp};

/// SplitMix64: a seeded stream, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [lo, hi).
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Log-uniform in [lo, hi): spans decades evenly.
    fn log_uniform(&mut self, lo: f64, hi: f64) -> f64 {
        self.uniform(lo.ln(), hi.ln()).exp()
    }
    /// A host sample rate between 8 kHz and 768 kHz, fractional ones included.
    fn sample_rate(&mut self) -> SampleRate {
        SampleRate::new(self.uniform(8_000.0, 768_000.0) as f32)
    }
}

const CASES: usize = 10_000;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

#[test]
fn db_and_gain_round_trip_and_match_the_helpers() {
    let mut r = Rng(1);
    for _ in 0..CASES {
        let d = r.uniform(-170.0, 40.0) as f32;
        let g = db(d).to_gain();
        assert_eq!(g.get().to_bits(), db_to_amp(d).to_bits(), "{d} dB");
        assert!(
            (g.to_db().get() - d).abs() < 1e-3,
            "{d} dB -> {g} -> {}",
            g.to_db()
        );

        let a = r.log_uniform(1e-8, 100.0) as f32;
        let back = Gain::new(a).to_db();
        assert_eq!(back.get().to_bits(), amp_to_db(a).to_bits(), "{a}");
        assert!(close(back.to_gain().get() as f64, a as f64, 1e-4), "{a}");
        // Magnitude: a polarity flip has the same level.
        assert_eq!(Gain::new(-a).to_db(), back);
    }
    assert_eq!(Gain::ZERO.to_db(), Db::FLOOR);
    assert_eq!(db(f32::NEG_INFINITY).to_gain(), Gain::ZERO);
    assert_eq!(Db::UNITY.to_gain(), Gain::UNITY);
    // Levels add where gains multiply.
    let (x, y) = (db(-6.0), db(-3.0));
    assert!(close(
        (x + y).to_gain().get() as f64,
        (x.to_gain() * y.to_gain()).get() as f64,
        1e-6
    ));
}

#[test]
fn seconds_and_samples_round_trip_at_any_rate() {
    let mut r = Rng(2);
    for _ in 0..CASES {
        let sample_rate = r.sample_rate();
        let t = seconds(r.log_uniform(1e-6, 3_600.0));
        let n = t.samples(sample_rate);
        assert!(
            close(n.seconds(sample_rate).get(), t.get(), 1e-12),
            "{t} at {sample_rate}"
        );
        assert_eq!(n.get(), t.get() * sample_rate.get() as f64);
        // A whole count survives the trip exactly, far past f32's 2^24.
        let whole = (r.next() >> 24) as usize; // < 2^40
        let back = Samples::of_frames(whole)
            .seconds(sample_rate)
            .samples(sample_rate);
        assert_eq!(back.frames(), whole, "{whole} samples at {sample_rate}");
        // ms is a constructor for Seconds.
        let m = r.uniform(-1e4, 1e4);
        assert_eq!(ms(m), seconds(m / 1e3));
        assert!(close(ms(m).to_ms(), m, 1e-15));
        assert_eq!(us(m), seconds(m / 1e6));
    }
    assert_eq!(rate_48k().period().samples(rate_48k()).get(), 1.0);
}

fn rate_48k() -> SampleRate {
    SampleRate::new(48_000.0)
}

#[test]
fn frames_round_to_nearest_and_never_go_negative() {
    assert_eq!(samples(2.49).frames(), 2);
    assert_eq!(samples(2.5).frames(), 3);
    assert_eq!(samples(-3.0).frames(), 0);
    assert_eq!(Samples(f64::NAN).frames(), 0);
    assert_eq!(samples(f64::INFINITY).frames(), usize::MAX);
}

#[test]
fn period_and_frequency_are_inverses() {
    let mut r = Rng(3);
    for _ in 0..CASES {
        let f = hz(r.log_uniform(0.01, 100_000.0) as f32);
        assert!(
            close(f.period().frequency().get() as f64, f.get() as f64, 1e-6),
            "{f}"
        );
        let sample_rate = r.sample_rate();
        // One period is 1 / (cycles per sample) samples.
        let step = f.cycles_per_sample(sample_rate);
        assert!(
            close(f.period().samples(sample_rate).get() * step, 1.0, 1e-12),
            "{f} at {sample_rate}"
        );
    }
    assert_eq!(khz(1.0), hz(1000.0));
}

#[test]
fn intervals_convert_to_frequency_ratios() {
    let mut r = Rng(4);
    for _ in 0..CASES {
        let s = semitones(r.uniform(-48.0, 48.0) as f32);
        let ratio = s.ratio();
        assert!((ratio.semitones() - s.get()).abs() < 1e-3, "{s}");
        // Cents and semitones are one scale.
        let c = Cents::from(s);
        assert!(
            close(c.ratio().get() as f64, ratio.get() as f64, 1e-5),
            "{s} {c}"
        );
        assert!((Semitones::from(c) - s).get().abs() < 1e-5);
        assert!((ratio.cents() - c.get()).abs() < 0.1);
        // Intervals add where ratios multiply.
        let t = semitones(r.uniform(-24.0, 24.0) as f32);
        assert!(close(
            (s + t).ratio().get() as f64,
            (s.ratio() * t.ratio()).get() as f64,
            1e-5
        ));
        // A frequency moved by an interval and back: `Hz / Hz` is the interval.
        let f = hz(r.log_uniform(20.0, 20_000.0) as f32);
        let moved = f * ratio;
        assert!(((moved / f).semitones() - s.get()).abs() < 1e-3, "{f} {s}");
        assert!(close((moved / ratio).get() as f64, f.get() as f64, 1e-6));
    }
    assert_eq!(semitones(12.0).ratio(), Ratio::new(2.0));
    assert_eq!(semitones(0.0).ratio(), Ratio::UNITY);
    assert_eq!(cents(-1200.0).ratio(), Ratio::new(0.5));
}

#[test]
fn ohms_law_and_time_constants() {
    let mut r = Rng(5);
    for _ in 0..CASES {
        let v = volts(r.uniform(-500.0, 500.0));
        let res = ohms(r.log_uniform(1.0, 1e7));
        let i = v / res;
        assert!(close((i * res).get(), v.get(), 1e-15), "{v} {res}");
        assert!(close((res * i).get(), v.get(), 1e-15));
        if v.get() != 0.0 {
            assert!(close((v / i).get(), res.get(), 1e-15));
        }
        let c = farads(r.log_uniform(1e-12, 1e-3));
        assert_eq!((res * c).get(), res.get() * c.get());
        assert_eq!(res * c, c * res);
        let l = henries(r.log_uniform(1e-6, 10.0));
        assert_eq!((l / res).get(), l.get() / res.get());
        // Parallel: symmetric, below both, and an equal pair halves.
        let other = ohms(r.log_uniform(1.0, 1e7));
        let p = res.parallel(other);
        assert_eq!(p, other.parallel(res));
        let below = |a: Ohms| p.get() <= a.get() * (1.0 + 1e-15);
        assert!(below(res) && below(other), "{res} || {other} = {p}");
        assert!(close(res.parallel(res).get(), res.get() / 2.0, 1e-15));
    }
    assert_eq!(kohm(1.0).parallel(ohms(0.0)), ohms(0.0));
    assert_eq!(kohm(1.0).parallel(ohms(f64::INFINITY)), kohm(1.0));
    assert_eq!(volts(9.0) / kohm(4.5), ma(2.0));
}

#[test]
fn si_prefixes_read_as_written() {
    assert_eq!(kohm(4.7).get(), 4.7e3);
    assert_eq!(megohm(1.0), kohm(1000.0));
    assert_eq!(nf(22.0).get(), 22.0 * 1e-9);
    assert_eq!(uf(1.0), farads(1e-6));
    assert_eq!(pf(470.0).get(), 470.0 * 1e-12);
    assert_eq!(mh(100.0), henries(0.1));
    assert_eq!(uh(1.0), henries(1e-6));
    assert_eq!(mv(250.0), volts(0.25));
    assert_eq!(uv(1.0), volts(1e-6));
    assert_eq!(ma(1.5), amps(1.5e-3));
    assert_eq!(ua(1.0), amps(1e-6));
    assert_eq!(na(1.0), amps(1e-9));
}

#[test]
fn display_prints_the_unit() {
    let cases: &[(String, &str)] = &[
        (hz(440.0).to_string(), "440.0 Hz"),
        (hz(2400.0).to_string(), "2.40 kHz"),
        (hz(999.96).to_string(), "1.00 kHz"),
        (hz(999.94).to_string(), "999.9 Hz"),
        (SampleRate::new(44_100.0).to_string(), "44.10 kHz"),
        (db(-6.0).to_string(), "-6.0 dB"),
        (db(f32::NEG_INFINITY).to_string(), "-inf dB"),
        (Gain::new(0.5).to_string(), "×0.500"),
        (ms(30.0).to_string(), "30.0 ms"),
        (seconds(1.25).to_string(), "1.25 s"),
        (samples(480.0).to_string(), "480.00 samples"),
        (Ratio::new(4.0).to_string(), "4.0:1"),
        (semitones(-12.0).to_string(), "-12.00 st"),
        (cents(5.0).to_string(), "+5.0 cents"),
        (kohm(4.7).to_string(), "4.70 kΩ"),
        (ohms(999.96).to_string(), "1.00 kΩ"),
        (ohms(100.0).to_string(), "100 Ω"),
        (ohms(0.0).to_string(), "0.00 Ω"),
        (megohm(1.0).to_string(), "1.00 MΩ"),
        (nf(22.0).to_string(), "22.0 nF"),
        (uf(1.0).to_string(), "1.00 µF"),
        (pf(470.0).to_string(), "470 pF"),
        (mh(100.0).to_string(), "100 mH"),
        (mv(-250.0).to_string(), "-250 mV"),
        (ma(1.5).to_string(), "1.50 mA"),
        (volts(f64::INFINITY).to_string(), "inf V"),
    ];
    for (got, want) in cases {
        assert_eq!(got, want);
    }
}

/// Engineering notation over every decade: the prefix and three
/// significant figures read back to the value within rounding.
#[test]
fn engineering_text_reads_back() {
    let prefix = |p: &str| match p {
        "f" => 1e-15,
        "p" => 1e-12,
        "n" => 1e-9,
        "µ" => 1e-6,
        "m" => 1e-3,
        "" => 1.0,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        other => panic!("prefix {other:?}"),
    };
    let mut r = Rng(6);
    for _ in 0..CASES {
        let v = r.log_uniform(1e-15, 1e11) * if r.next() & 1 == 0 { 1.0 } else { -1.0 };
        let text = volts(v).to_string();
        let (num, unit) = text.split_once(' ').unwrap();
        let p = unit.strip_suffix('V').unwrap();
        let n: f64 = num.parse().unwrap();
        let digits = num.trim_start_matches('-').replace('.', "");
        assert_eq!(digits.len(), 3, "{text}: three figures");
        assert!(n.abs() < 1000.0 || p == "G", "{text}");
        assert!(n.abs() >= 1.0 || p == "f", "{text}");
        assert!(close(n * prefix(p), v, 5e-3), "{v} reads {text}");
    }
}

#[test]
fn sample_rate_derives_its_frequencies() {
    let sample_rate = rate_48k();
    assert_eq!(sample_rate.hz(), hz(48_000.0));
    assert_eq!(sample_rate.nyquist(), hz(24_000.0));
    assert_eq!(sample_rate.region(4), SampleRate::new(192_000.0));
    assert_eq!(f32::from(sample_rate), 48_000.0);
    assert_eq!(hz(12_000.0).cycles_per_sample(sample_rate), 0.25);
}

#[test]
fn arithmetic_reads_as_physics() {
    assert_eq!(hz(100.0) + hz(50.0), hz(150.0));
    assert_eq!(hz(100.0) * 2.0, hz(200.0));
    assert_eq!(2.0 * hz(100.0), hz(200.0));
    assert_eq!(hz(300.0) / hz(100.0), Ratio::new(3.0));
    assert_eq!(seconds(1.0) / seconds(4.0), 0.25);
    assert_eq!(-db(6.0), db(-6.0));
    let mut t = ms(10.0);
    t += ms(5.0);
    t -= ms(1.0);
    assert!(close(t.to_ms(), 14.0, 1e-12));
    let mut g = Gain::new(0.5);
    g *= Gain::new(0.5);
    assert_eq!(g, Gain::new(0.25));
    assert_eq!(0.8 * Gain::new(0.5), 0.4);
    assert_eq!(Ratio::new(2.0) / Ratio::new(4.0), Ratio::new(0.5));
}

#[test]
fn units_cost_nothing() {
    use core::mem::{align_of, size_of};
    assert_eq!(size_of::<Hz>(), size_of::<f32>());
    assert_eq!(size_of::<Db>(), size_of::<f32>());
    assert_eq!(size_of::<Gain>(), size_of::<f32>());
    assert_eq!(size_of::<SampleRate>(), size_of::<f32>());
    assert_eq!(size_of::<Seconds>(), size_of::<f64>());
    assert_eq!(size_of::<Samples>(), size_of::<f64>());
    assert_eq!(size_of::<Ohms>(), size_of::<f64>());
    assert_eq!(align_of::<[Hz; 4]>(), align_of::<[f32; 4]>());
    assert_eq!(size_of::<Option<Hz>>(), size_of::<Option<f32>>());
}

/// Range checks fire in debug builds only (a release build never panics
/// on the audio path; these tests are compiled out there).
#[cfg(debug_assertions)]
mod range_checks {
    use super::*;

    #[test]
    #[should_panic(expected = "Hz is NaN")]
    fn nan_hz() {
        let _ = hz(f32::NAN);
    }

    #[test]
    #[should_panic(expected = "SampleRate is not finite and positive")]
    fn zero_rate() {
        let _ = SampleRate::new(0.0);
    }

    #[test]
    #[should_panic(expected = "Ohms is negative or NaN")]
    fn negative_ohms() {
        let _ = kohm(-1.0);
    }

    #[test]
    #[should_panic(expected = "Ratio is not positive")]
    fn zero_ratio() {
        let _ = Ratio::new(0.0);
    }

    #[test]
    #[should_panic(expected = "the period of 0 Hz")]
    fn period_of_dc() {
        let _ = hz(0.0).period();
    }

    #[test]
    fn differences_skip_the_check() {
        // A difference of two valid values may leave the range: results
        // of arithmetic are not re-checked.
        assert_eq!((kohm(1.0) - kohm(2.0)).get(), -1000.0);
    }
}
