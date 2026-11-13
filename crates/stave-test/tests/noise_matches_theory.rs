//! H9: a circuit's own noise against the analytic formulas, and a power
//! supply's ripple into a circuit against its small-signal response.
//!
//! Thermal noise: a resistor `R` is a current source of one-sided PSD
//! `4kT/R` beside it, so an RC lowpass's output PSD is `4kTR/(1 +
//! (ΩRC)²)` — through the trapezoidal rule exactly, with `Ω = 2f_s·tan(πf/
//! f_s)`. Shot noise: a junction carrying `I` is a current of PSD `2qI`
//! beside it, seen through the node's small-signal impedance. Every
//! estimate is Welch's (Hann, half overlap) over 2²⁰ samples, averaged
//! over 16-bin bands: about 1 % standard error a band.

use stave_dsp::circuit::methods::dk::{Dk, Noise, BOLTZMANN, ELECTRON_CHARGE};
use stave_dsp::circuit::netlist::{Netlist, Variable};
use stave_dsp::circuit::topologies::power_supply::{PowerSupply, PowerSupplyConfig, SupplyFilter};
use stave_dsp::circuit::Circuit;
use stave_dsp::units::Ohms;
use stave_dsp::Stage;
use stave_measure::spectrum::{goertzel, welch};

const FS: f64 = 48_000.0;
const SEGMENT: usize = 4096;
const SAMPLES: usize = 1 << 20;
const KELVIN: f64 = 300.15;

const RC: &str = "* RC lowpass
V1 in 0 0
R1 in out 10k
C1 out 0 10n
.control
wrdata out v(out)
.endc";

fn run(dk: &mut Circuit<Dk>, n: usize) -> Vec<f64> {
    (0..n).map(|_| dk.tick_f64(0.0)).collect()
}

/// `4kTR/(1 + (ΩRC)²)` at bin `k`, warped as the trapezoidal rule warps.
fn rc_psd(ohms: f64, farads: f64, k: usize) -> f64 {
    let f = k as f64 * FS / SEGMENT as f64;
    let omega = 2.0 * FS * (std::f64::consts::PI * f / FS).tan();
    4.0 * BOLTZMANN * KELVIN * ohms / (1.0 + (omega * ohms * farads).powi(2))
}

/// The worst band's error in dB of `psd` against `expect(bin)`, 16-bin
/// bands from bin 16 up to `top`.
fn worst_band_db(psd: &[f64], top: usize, expect: impl Fn(usize) -> f64) -> f64 {
    (16..top)
        .step_by(16)
        .map(|lo| {
            let measured: f64 = psd[lo..lo + 16].iter().sum();
            let predicted: f64 = (lo..lo + 16).map(&expect).sum();
            (10.0 * (measured / predicted).log10()).abs()
        })
        .fold(0.0, f64::max)
}

fn variance(x: &[f64]) -> f64 {
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / x.len() as f64
}

#[test]
fn thermal_noise_of_an_rc_matches_4ktr() {
    let mut dk = Circuit::<Dk>::new(&Netlist::parse_spice(RC).unwrap()).unwrap();
    dk.drive("v1").unwrap();
    dk.prepare(FS as f32);
    dk.set_noise(Some(Noise::room(1).thermal_only()));
    assert_eq!(dk.noise_sources(), 1);
    let y = run(&mut dk, SAMPLES);
    let psd = welch(&y, SEGMENT, FS);
    let worst = worst_band_db(&psd, SEGMENT / 4, |k| rc_psd(10e3, 10e-9, k));
    assert!(worst < 0.3, "worst band {worst:.3} dB");
    // The variance: the warped PSD integrated over 0 … fs/2, and kT/C
    // (the analog total, reported).
    let fine = 1 << 16;
    let predicted: f64 = (0..fine)
        .map(|i| {
            let f = (i as f64 + 0.5) * FS / 2.0 / fine as f64;
            let omega = 2.0 * FS * (std::f64::consts::PI * f / FS).tan();
            4.0 * BOLTZMANN * KELVIN * 10e3 / (1.0 + (omega * 10e3 * 10e-9).powi(2))
        })
        .sum::<f64>()
        * FS
        / 2.0
        / fine as f64;
    let measured = variance(&y);
    let kt_c = BOLTZMANN * KELVIN / 10e-9;
    eprintln!(
        "RC: worst band {worst:.3} dB; variance {measured:.4e} V², predicted {predicted:.4e}, kT/C {kt_c:.4e}"
    );
    assert!((measured / predicted - 1.0).abs() < 0.015, "{measured:e} against {predicted:e}");
    // The discrete circuit holds the band below fs/2 only: less than kT/C.
    assert!(predicted < kt_c && predicted > 0.85 * kt_c, "{predicted:e} against kT/C {kt_c:e}");
}

#[test]
fn a_variable_resistor_makes_the_noise_of_its_present_value() {
    // The same RC with R1 a variable resistor (an MNA branch, its noise a
    // series voltage of 4kTR), first at 10k, then moved to 47k.
    let mut netlist = Netlist::parse_spice(RC).unwrap();
    netlist.add_variable(Variable::resistor("r1")).unwrap();
    let mut dk = Circuit::<Dk>::new(&netlist).unwrap();
    dk.drive("v1").unwrap();
    dk.prepare(FS as f32);
    dk.set_noise(Some(Noise::room(2).thermal_only()));
    let y = run(&mut dk, SAMPLES);
    let worst = worst_band_db(&welch(&y, SEGMENT, FS), SEGMENT / 4, |k| rc_psd(10e3, 10e-9, k));
    assert!(worst < 0.3, "10k: worst band {worst:.3} dB");
    let r = dk.variable_resistor("r1").unwrap();
    dk.set_ohms(r, Ohms::new(47e3));
    dk.settle();
    run(&mut dk, 4_800);
    let y = run(&mut dk, SAMPLES);
    let worst = worst_band_db(&welch(&y, SEGMENT, FS), SEGMENT / 4, |k| rc_psd(47e3, 10e-9, k));
    assert!(worst < 0.3, "47k: worst band {worst:.3} dB");
}

#[test]
fn shot_noise_of_a_biased_diode_matches_2qi() {
    let deck = "* a diode biased through 10k
V1 in 0 5
R1 in a 10k
D1 a 0 DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(a)
.endc";
    let mut dk = Circuit::<Dk>::new(&Netlist::parse_spice(deck).unwrap()).unwrap();
    dk.prepare(FS as f32);
    let volts = dk.output();
    dk.set_noise(Some(Noise::room(3).shot_only()));
    assert_eq!(dk.noise_sources(), 1);
    let y = run(&mut dk, SAMPLES);
    let amps = (5.0 - volts) / 10e3;
    let vt = BOLTZMANN * KELVIN / ELECTRON_CHARGE;
    let rd = 1.752 * vt / (amps + 2.52e-9);
    let z = 1.0 / (1.0 / 10e3 + 1.0 / rd);
    let psd_expect = 2.0 * ELECTRON_CHARGE * amps * z * z;
    let worst = worst_band_db(&welch(&y, SEGMENT, FS), SEGMENT / 2 - 16, |_| psd_expect);
    let predicted = psd_expect * FS / 2.0;
    let measured = variance(&y);
    eprintln!(
        "diode at {:.3} mA: worst band {worst:.3} dB; variance {measured:.4e} V² against {predicted:.4e}",
        amps * 1e3
    );
    // The resistor's thermal noise through the same node, for scale.
    let mut thermal = Circuit::<Dk>::new(&Netlist::parse_spice(deck).unwrap()).unwrap();
    thermal.prepare(FS as f32);
    thermal.set_noise(Some(Noise::room(3).thermal_only()));
    let t = run(&mut thermal, SAMPLES);
    let thermal_predicted = 4.0 * BOLTZMANN * KELVIN / 10e3 * z * z * FS / 2.0;
    eprintln!("thermal through the same node: {:.4e} against {thermal_predicted:.4e}", variance(&t));
    assert!(worst < 0.3, "worst band {worst:.3} dB");
    assert!((measured / predicted - 1.0).abs() < 0.01, "{measured:e} against {predicted:e}");
}

#[test]
fn noise_is_opt_in_seeded_and_reproducible() {
    let netlist = Netlist::parse_spice(RC).unwrap();
    let signal = |n: usize| (0.01 * n as f64).sin();
    let mut plain = Circuit::<Dk>::new(&netlist).unwrap();
    plain.drive("v1").unwrap();
    plain.prepare(FS as f32);
    let mut toggled = plain.clone();
    toggled.set_noise(Some(Noise::room(5)));
    toggled.set_noise(None);
    let a: Vec<f64> = (0..4096).map(|n| plain.tick_f64(signal(n))).collect();
    let b: Vec<f64> = (0..4096).map(|n| toggled.tick_f64(signal(n))).collect();
    assert_eq!(a, b, "noise off is the circuit without it, bit for bit");
    let mut noisy = plain.clone();
    noisy.set_noise(Some(Noise::room(5)));
    noisy.reset();
    let c: Vec<f64> = (0..4096).map(|n| noisy.tick_f64(signal(n))).collect();
    noisy.reset();
    let d: Vec<f64> = (0..4096).map(|n| noisy.tick_f64(signal(n))).collect();
    assert_eq!(c, d, "a reset replays the same noise");
    assert_ne!(a, c);
    let mut other = plain.clone();
    other.set_noise(Some(Noise::room(6)));
    let e: Vec<f64> = (0..4096).map(|n| other.tick_f64(signal(n))).collect();
    assert_ne!(c, e, "another seed, another draw");
}

/// A resistor-filtered supply: reservoir 47 µF, 1 kΩ, 47 µF.
fn rc_supply(secondary_peak_volts: f64) -> PowerSupply {
    let mut supply = PowerSupply::new(PowerSupplyConfig {
        secondary_peak_volts,
        reservoir_farads: 47e-6,
        filter: SupplyFilter::Resistor { ohms: 1e3 },
        filter_farads: 47e-6,
        ..PowerSupplyConfig::SILICON_CHOKE
    });
    supply.prepare(FS as f32);
    supply
}

#[test]
fn supply_ripple_matches_the_reservoir_formula() {
    // Full-wave into C₁ under a steady load I: a sawtooth of I/(2fC₁)
    // peak to peak, its fundamental (at 2f) 1/π of that, then the RC
    // filter's |1/(1 + jωRC₂)|.
    let amps = 0.02;
    let mut supply = rc_supply(350.0);
    for _ in 0..(2.0 * FS) as usize {
        supply.tick_f64(amps);
    }
    let y: Vec<f64> = (0..48_000).map(|_| supply.tick_f64(amps)).collect();
    let ripple = goertzel(&y, 120.0 / FS);
    let sawtooth = amps / (2.0 * 60.0 * 47e-6) / std::f64::consts::PI;
    let filter = 1.0 / (1.0 + (std::f64::consts::TAU * 120.0 * 1e3 * 47e-6).powi(2)).sqrt();
    let predicted = sawtooth * filter;
    eprintln!("ripple at 120 Hz {:.2} mV, the formula {:.2} mV", ripple * 1e3, predicted * 1e3);
    assert!((ripple / predicted - 1.0).abs() < 0.05, "{ripple} against {predicted}");
}

#[test]
fn supply_ripple_reaches_a_stage_through_its_supply_response() {
    // A 12AX7 common-cathode stage whose 300 V rail is held, sample by
    // sample, to a supply model: the plate's hum at 120 Hz is the rail's
    // ripple times the stage's response from its rail.
    let deck = include_str!("../spice/common_cathode.cir");
    let mut stage = Circuit::<Dk>::new(&Netlist::parse_spice(deck).unwrap()).unwrap();
    stage.drive("v1").unwrap();
    stage.prepare(FS as f32);
    let rail = stage.supply("vb").unwrap();
    let plate = stage.output();
    let amps = (300.0 - plate) / 100e3;
    let mut supply = rc_supply(305.0);
    let mut bplus = Vec::new();
    let mut out = Vec::new();
    for n in 0..(3.0 * FS) as usize {
        let volts = supply.tick_f64(amps);
        stage.set_supply(rail, volts);
        let y = stage.tick_f64(0.0);
        if n >= (2.0 * FS) as usize {
            bplus.push(volts);
            out.push(y);
        }
    }
    let ripple = goertzel(&bplus, 120.0 / FS);
    let hum = goertzel(&out, 120.0 / FS);
    let response = stage.supply_response(rail, 120.0).unwrap();
    let predicted = ripple * (response.re.hypot(response.im));
    let db = 20.0 * (hum / predicted).log10();
    eprintln!(
        "rail ripple {:.2} mV, plate hum {:.3} mV, predicted {:.3} mV ({db:.3} dB)",
        ripple * 1e3,
        hum * 1e3,
        predicted * 1e3
    );
    assert!(db.abs() < 0.1, "{db} dB");
    assert!(stage.stats().capped == 0);
}
