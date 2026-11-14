use super::*;
use crate::kernel::node::Stage;
use crate::primitives::time::delay::{Delay, DelayLine};
use crate::units::ms;

const FRACS: [f32; 7] = [0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95];

/// The interpolated read of a constant is that constant: FIR weights sum
/// to one at every fraction, and the allpass has unity gain at DC.
#[test]
fn weights_sum_to_one() {
    let ones = [1.0f32; MAX_TAPS];
    for k in 0..=100 {
        let frac = k as f32 / 101.0;
        let s3: f32 = Lagrange3::weights(frac).iter().sum();
        let s5: f32 = Lagrange5::weights(frac).iter().sum();
        assert!((s3 - 1.0).abs() < 1e-6, "Lagrange3 at {frac}: {s3}");
        assert!((s5 - 1.0).abs() < 1e-6, "Lagrange5 at {frac}: {s5}");
        for sum in [
            Sinc::<4>::weights(frac).iter().sum::<f32>(),
            Sinc::<8>::weights(frac).iter().sum::<f32>(),
            Sinc::<32>::weights(frac).iter().sum::<f32>(),
        ] {
            assert!((sum - 1.0).abs() < 1e-5, "Sinc at {frac}: {sum}");
        }
        // The tabled reads, between rows included.
        let (mut s8, mut s16) = (Sinc::<8>::default(), Sinc::<16>::default());
        s8.prepare();
        s16.prepare();
        assert!((s8.interpolate(&ones[..8], frac) - 1.0).abs() < 1e-5);
        assert!((s16.interpolate(&ones[..16], frac) - 1.0).abs() < 1e-5);
        assert!((Linear.interpolate(&ones[..2], frac) - 1.0).abs() < 1e-6);
        let mut thiran = Thiran::default();
        let settled = (0..64).fold(0.0, |_, _| thiran.interpolate(&ones[..3], frac));
        assert!((settled - 1.0).abs() < 1e-6, "Thiran DC at {frac}: {settled}");
    }
}

/// Magnitude (dB) and phase (cycles) error of a sine at `f` cycles per
/// sample read `20 + frac` samples back, against the exact delayed sine.
fn sine_error<I: Interpolator>(f: f64, frac: f32) -> (f64, f64) {
    let delay = 20.0 + f64::from(frac);
    let mut line = DelayLine::<I>::default();
    line.allocate(64);
    let (warm, n) = (256usize, 4_000usize); // n holds whole periods of f
    let (mut cs, mut cc) = (0.0f64, 0.0f64);
    for i in 0..warm + n {
        let t = i as f64;
        line.write((core::f64::consts::TAU * f * t).sin() as f32);
        let y = f64::from(line.interpolated(20.0 + frac));
        line.advance();
        if i >= warm {
            let theta = core::f64::consts::TAU * f * (t - delay);
            cs += y * theta.sin();
            cc += y * theta.cos();
        }
    }
    let (cs, cc) = (2.0 * cs / n as f64, 2.0 * cc / n as f64);
    let magnitude_db = 20.0 * cs.hypot(cc).log10();
    let phase_cycles = cc.atan2(cs) / core::f64::consts::TAU;
    (magnitude_db, phase_cycles)
}

/// The worst magnitude (|dB|) and phase (|cycles|) error over `FRACS`.
fn worst<I: Interpolator>(f: f64) -> (f64, f64) {
    FRACS.iter().fold((0.0f64, 0.0f64), |(m, p), &frac| {
        let (dm, dp) = sine_error::<I>(f, frac);
        (m.max(dm.abs()), p.max(dp.abs()))
    })
}

/// Each interpolator's documented budget: (worst |dB|, worst |cycles|)
/// at 0.1 and at 0.25 cycles per sample (4.8 and 12 kHz at 48 kHz).
#[test]
fn sine_errors_stay_inside_their_budgets() {
    #[allow(clippy::type_complexity)]
    let cases: [(&str, fn(f64) -> (f64, f64), [(f64, f64); 2]); 7] = [
        ("Linear", worst::<Linear>, [(0.45, 0.001), (3.05, 0.012)]),
        ("Lagrange3", worst::<Lagrange3>, [(0.035, 0.0001), (1.1, 0.005)]),
        ("Lagrange5", worst::<Lagrange5>, [(0.003, 0.00001), (0.45, 0.002)]),
        ("Thiran", worst::<Thiran>, [(0.001, 0.0025), (0.001, 0.03)]),
        ("Sinc<8>", worst::<Sinc<8>>, [(0.006, 0.00003), (0.001, 0.0001)]),
        ("Sinc<16>", worst::<Sinc<16>>, [(0.0015, 0.00005), (0.0035, 0.00005)]),
        ("Sinc<32>", worst::<Sinc<32>>, [(0.005, 0.00001), (0.0005, 0.00002)]),
    ];
    let mut over = Vec::new();
    for (name, measure, budgets) in cases {
        for (f, (db_budget, cycle_budget)) in [0.1, 0.25].into_iter().zip(budgets) {
            let (db, cycles) = measure(f);
            println!("{name:<10} f={f:.2}: |mag| {db:.5} dB, |phase| {cycles:.6} cycles");
            if db > db_budget || cycles > cycle_budget {
                over.push(format!(
                    "{name} at {f}: {db:.5} dB (budget {db_budget}), {cycles:.6} cycles (budget {cycle_budget})"
                ));
            }
        }
    }
    assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
}

/// Higher order is better in the treble: the budgets above are ordered.
#[test]
fn higher_order_reads_closer() {
    let (l1, l3, l5) = (worst::<Linear>(0.25), worst::<Lagrange3>(0.25), worst::<Lagrange5>(0.25));
    assert!(l1.0 > l3.0 && l3.0 > l5.0, "{l1:?} {l3:?} {l5:?}");
    let (s8, s16) = (worst::<Sinc<8>>(0.25), worst::<Sinc<16>>(0.25));
    assert!(s8.0 < l5.0 && s16.0 < l5.0, "{s8:?} {s16:?}");
}

/// A delay swept at extreme rates — sub-Hz to near Nyquist, random
/// jumps, NaN settings — reads a full-scale signal without a NaN and
/// stays bounded, for every interpolator.
fn modulated_stays_finite<I: Interpolator>(name: &str) {
    let rate = 48_000.0f32;
    let mut lcg = 0x1234_5678u32;
    let mut noise = move || {
        lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    for mod_hz in [0.01f32, 7.0, 1_000.0, 11_000.0, 23_999.0] {
        let mut d = Delay::<I>::new(ms(20.0), ms(10.0));
        d.prepare(rate);
        let mut peak = 0.0f32;
        for n in 0..4_800 {
            let phase = core::f32::consts::TAU * mod_hz * n as f32 / rate;
            let ms = match n % 1_000 {
                0 => f32::NAN,
                1..=99 => 10.0 * (noise() + 1.0), // random jumps over the range
                _ => 10.0 + 9.99 * phase.sin(),
            };
            d.set_delay(crate::units::ms(f64::from(ms)));
            let x = if n % 64 < 32 { 1.0 } else { noise() };
            let y = d.tick(x);
            assert!(y.is_finite(), "{name} at {mod_hz} Hz, sample {n}: {y}");
            peak = peak.max(y.abs());
        }
        assert!(peak < 3.0, "{name} at {mod_hz} Hz: peak {peak}");
    }
}

#[test]
fn modulated_reads_stay_finite() {
    modulated_stays_finite::<Linear>("Linear");
    modulated_stays_finite::<Lagrange3>("Lagrange3");
    modulated_stays_finite::<Lagrange5>("Lagrange5");
    modulated_stays_finite::<Thiran>("Thiran");
    modulated_stays_finite::<Sinc<8>>("Sinc<8>");
    modulated_stays_finite::<Sinc<16>>("Sinc<16>");
}

/// The table holds the directly computed weights to −90 dB.
#[test]
fn sinc_table_matches_the_direct_weights() {
    let mut s = Sinc::<16>::default();
    s.prepare();
    let mut taps = [0.0f32; 16];
    for k in 0..=200 {
        let frac = k as f32 / 201.0;
        let direct = Sinc::<16>::weights(frac);
        for j in 0..16 {
            taps.fill(0.0);
            taps[j] = 1.0;
            let tabled = s.interpolate(&taps, frac);
            assert!((tabled - direct[j]).abs() < 3e-5, "frac {frac} tap {j}");
        }
    }
}
