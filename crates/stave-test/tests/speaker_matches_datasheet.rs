//! The loudspeaker driver (DSP.md E9) against real guitar speakers'
//! published data: Jensen's C12N, P12N, C12K and P10R specification
//! sheets (8 Ω versions), their printed Thiele–Small tables
//! (`crates/stave-test/datasheet/jensen_*_ts.txt`), free-air impedance
//! curves (`jensen_*_impedance.txt`) and on-axis response on an IEC
//! baffle at 1 W, 1 m (`jensen_*_spl.txt`), the manufacturer's own
//! plotted numbers.
//!
//! Budgets are the reading's uncertainty plus a stated model tolerance
//! (R13). A miss is a *known gap*: the test holds it no worse than
//! recorded, so an improvement is promoted on purpose.
//!
//! Each test runs on its own thread (libtest's); nothing here spawns.

use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
use stave_dsp::Stage;
use stave_test::reference::Datasheet;

const DRIVERS: [(&str, ThieleSmall, ThieleSmall); 4] = [
    ("jensen_c12n", ThieleSmall::JENSEN_C12N, ThieleSmall::JENSEN_C12N_LR2),
    ("jensen_p12n", ThieleSmall::JENSEN_P12N, ThieleSmall::JENSEN_P12N_LR2),
    ("jensen_c12k", ThieleSmall::JENSEN_C12K, ThieleSmall::JENSEN_C12K_LR2),
    ("jensen_p10r", ThieleSmall::JENSEN_P10R, ThieleSmall::JENSEN_P10R_LR2),
];

/// The 8 Ω version's printed value of `key`.
fn printed(sheet: &Datasheet, key: &str) -> f64 {
    let r = &sheet.curve(&[(key, 1.0)]).reference;
    let i = r.abscissa().iter().position(|&x| x == 8.0).expect("an 8-ohm column");
    r.values()[i]
}

/// The 8 Ω curve of an impedance or response table: `(Hz, value)`.
fn curve(name: &str) -> Vec<(f64, f64)> {
    let sheet = Datasheet::load(name);
    let r = &sheet.curve(&[("nominal", 8.0)]).reference;
    r.abscissa().iter().copied().zip(r.values().iter().copied()).collect()
}

/// Hold `error` to `budget`, or — a known gap — to `recorded`.
fn hold(what: &str, error: f64, budget: f64, recorded: Option<f64>) {
    match recorded {
        None => {
            eprintln!("  {what}: {error:.4} (budget {budget})");
            assert!(error <= budget, "{what}: {error} over {budget}");
        }
        Some(gap) => {
            eprintln!("  {what}: {error:.4} — known gap (budget {budget}, recorded {gap})");
            assert!(error > budget, "{what}: {error} now within {budget}: promote it");
            assert!(error <= gap, "{what}: {error} worse than recorded {gap}");
        }
    }
}

#[test]
fn the_printed_tables_close_on_themselves() {
    for (name, part, _) in DRIVERS {
        let sheet = Datasheet::load(&format!("{name}_ts"));
        eprintln!("{name} (8 Ω), as printed:");
        // The constant is the table.
        for (key, unit, value) in [
            ("re", 1.0, part.re),
            ("le", 1e-3, part.le),
            ("bl", 1.0, part.bl),
            ("mms", 1e-3, part.mms),
            ("cms", 1e-6, part.cms),
            ("sd", 1e-4, part.sd),
        ] {
            assert!((value / (printed(&sheet, key) * unit) - 1.0).abs() < 1e-12, "{name} {key}");
        }
        assert!((part.qms() / printed(&sheet, "qms") - 1.0).abs() < 1e-6, "{name} qms");
        // Small's relations against the sheet's own derived values:
        // 2 % (printing to three figures, ρ = 1.204 kg/m³).
        let rel = |a: f64, b: f64| (a / b - 1.0).abs();
        hold("fs = 1/2π√(Mms·Cms)", rel(part.resonance(), printed(&sheet, "fs")), 0.02, None);
        hold("Qes = ωs·Mms·Re/Bl²", rel(part.qes(), printed(&sheet, "qes")), 0.02, None);
        hold("Qts", rel(part.qts(), printed(&sheet, "qts")), 0.02, None);
        hold("Vas = ρc²·Cms·Sd²", rel(part.vas(), printed(&sheet, "vas") * 1e-3), 0.02, None);
        // The sheets' η₀ runs 21–24 % above Small's from their own
        // table, and their sensitivity ~1 dB above the piston's.
        hold("η₀ (Small 1972)", rel(part.efficiency() * 100.0, printed(&sheet, "eta0")), 0.02, Some(0.25));
        hold(
            "1 W / 1 m sensitivity (dB)",
            (part.sensitivity() - printed(&sheet, "sensitivity")).abs(),
            0.5,
            Some(1.65),
        );
    }
}

/// `(worst, rms)` relative |Z| error of `part` against the curve, over
/// the points `keep` selects.
fn impedance_error(part: &ThieleSmall, curve: &[(f64, f64)], keep: impl Fn(f64) -> bool) -> (f64, f64) {
    let errors: Vec<f64> = curve
        .iter()
        .filter(|(f, _)| keep(*f))
        .map(|&(f, z)| (part.impedance(f).norm() / z - 1.0).abs())
        .collect();
    let worst = errors.iter().fold(0.0f64, |m, &e| m.max(e));
    (worst, (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt())
}

#[test]
fn impedance_follows_the_published_curve() {
    // The printed table with a pure Le (at 1 kHz) misses the curve: its
    // resonance and Re are not the measured unit's, and a guitar
    // speaker's coil is semi-inductive. Recorded worst errors below and
    // above 1 kHz, and for the LR-2 fit over the whole curve.
    let recorded = [(0.36, 1.33, Some(0.142)), (0.27, 1.47, Some(0.119)), (0.57, 1.06, Some(0.143)), (0.40, 0.92, None)];
    for ((name, part, fitted), (low, high, fit)) in DRIVERS.into_iter().zip(recorded) {
        let z = curve(&format!("{name}_impedance"));
        eprintln!("{name} (8 Ω) |Z| against the published curve:");
        let (w_low, _) = impedance_error(&part, &z, |f| f < 1_000.0);
        let (w_high, _) = impedance_error(&part, &z, |f| f >= 1_000.0);
        hold("printed table, below 1 kHz", w_low, 0.10, Some(low));
        hold("printed table, 1–20 kHz", w_high, 0.10, Some(high));
        // The LR-2 fit: within 5 % rms over the whole curve. Its worst
        // points are the resonance, whose published peak is sampled
        // every 1/12 octave (and, for the high-Qms drivers, reads below
        // Re·(1 + Qms/Qes) of any fit), and the cone's breakup wiggles,
        // which no lumped coil follows.
        let (worst, rms) = impedance_error(&fitted, &z, |_| true);
        hold("LR-2 fit, rms", rms, 0.05, None);
        hold("LR-2 fit, worst", worst, 0.10, fit);
        // The fitted coil's LR-2 is what the curve needs: the pure Le
        // reads high above 1 kHz.
        let (fit_high, _) = impedance_error(&fitted, &z, |f| f >= 1_000.0);
        assert!(fit_high < 0.25 * w_high, "{name}");
    }
}

#[test]
fn response_follows_the_published_curve_in_the_piston_band() {
    // 1 W into the nominal 8 Ω (2.83 V), 1 m, half space, against the
    // IEC baffle's curve from 150 to 600 Hz: the band where the cone is
    // a piston and the baffle (1.35 × 1.65 m) is large. Below, the
    // baffle's edge loses the bass the infinite baffle keeps; above, the
    // cone breaks up. Recorded: the worst point, dB.
    let recorded = [2.6, 3.1, 4.2, 5.0];
    for ((name, _, fitted), worst_gap) in DRIVERS.into_iter().zip(recorded) {
        let spl = curve(&format!("{name}_spl"));
        let band: Vec<f64> = spl
            .iter()
            .filter(|(f, _)| (150.0..=600.0).contains(f))
            .map(|&(f, db)| fitted.spl(f, 8.0f64.sqrt(), 1.0, 0.0) - db)
            .collect();
        let mean = band.iter().sum::<f64>() / band.len() as f64;
        let worst = band.iter().fold(0.0f64, |m, &d| m.max(d.abs()));
        eprintln!("{name} (8 Ω, LR-2 fit) SPL against the IEC-baffle curve, 150–600 Hz:");
        hold("mean level (dB)", mean.abs(), 2.0, None);
        hold("worst point (dB)", worst, 2.0, Some(worst_gap));
    }
}

/// The stage itself, not only its analytic twin: a fitted driver run at
/// 48 kHz shows the curve's impedance, to the bilinear transform's
/// warping, where the curve is lumped.
#[test]
fn the_running_driver_shows_the_published_impedance() {
    let rate = 48_000.0;
    let (name, _, fitted) = DRIVERS[0];
    let z = curve(&format!("{name}_impedance"));
    let mut d = Driver::new(fitted, LargeSignal::LINEAR);
    d.prepare(rate as f32);
    eprintln!("{name} (8 Ω, LR-2 fit) running at 48 kHz:");
    let mut worst = 0.0f64;
    for &(hz, published) in z.iter().filter(|(f, _)| (40.0..=1_000.0).contains(f)).step_by(4) {
        d.reset();
        // A whole number of cycles after 0.4 s of settling.
        let window = ((0.2 * hz).ceil() * rate / hz).round() as usize;
        let tail = (0.4 * rate) as usize;
        let (mut uu, mut ic, mut is) = (0.0, 0.0, 0.0);
        for k in 0..tail + window {
            let w = core::f64::consts::TAU * hz * k as f64 / rate;
            d.drive(w.cos());
            if k >= tail {
                // Projections onto the tone: |Z| = |U|/|I|.
                uu += w.cos() * w.cos();
                ic += d.current() * w.cos();
                is += d.current() * w.sin();
            }
        }
        let measured = uu / (ic * ic + is * is).sqrt();
        let e = (measured / published - 1.0).abs();
        worst = worst.max(e);
    }
    hold("worst |Z| error, 40 Hz–1 kHz", worst, 0.10, None);
}
