//! Stateful ADAA (DSP.md A1) on the ngspice decks: `Circuit<Dk>` with
//! [`Antialiasing::Adaa`] at the oversampling its plain case runs at,
//! against the same reference — the in-band error ADAA introduces.
//!
//! ADAA's output is the circuit half a sample late (every term of the
//! update lands at `n − ½`), so each case is read at the reference's
//! instants by the four-point midpoint interpolation of the run,
//! `(−y[n−1] + 9·y[n] + 9·y[n+1] − y[n+2]) / 16` (exact for cubics); the
//! printout gives the unaligned error beside it. What is left is ADAA's
//! own: the averaging filter `½(1 + z⁻¹)` on the linear part and the
//! trapezoidal rule at `1.5·T`. Each budget sits above its measurement
//! (recorded on its test); the plain cases' budgets in
//! `dk_matches_ngspice.rs` and `transistors_match_ngspice.rs` are
//! untouched, ADAA being off by default.

use std::path::PathBuf;

use stave_dsp::circuit::methods::adaa::Antialiasing;
use stave_dsp::circuit::methods::dk::{Dk, Health};
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::Circuit;
use stave_test::reference::{Case, Metric, Reference};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The deck's own stimulus at `oversample` × 48 kHz under ADAA, read at
/// the reference's instants: `(aligned, unaligned, health)`.
fn transient(name: &str, oversample: u32) -> (Vec<f64>, Vec<f64>, Health) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
    circuit.set_antialiasing(Antialiasing::Adaa);
    circuit.prepare(rate as f32);
    let last = reference.abscissa().last().copied().unwrap_or(0.0);
    let samples = (last * rate).round() as usize + 3;
    let mut y = Vec::with_capacity(samples + 1);
    y.push(circuit.output());
    for _ in 0..samples {
        y.push(circuit.step(0.0));
    }
    let at = |i: isize| y[i.clamp(0, y.len() as isize - 1) as usize];
    let (mut aligned, mut unaligned) = (Vec::new(), Vec::new());
    for &t in reference.abscissa() {
        let n = (t * rate).round() as isize;
        // Sample 0 is the operating point itself, before any half-sample.
        aligned.push(if n == 0 {
            at(0)
        } else {
            (-at(n - 1) + 9.0 * at(n) + 9.0 * at(n + 1) - at(n + 2)) / 16.0
        });
        unaligned.push(at(n));
    }
    (aligned, unaligned, circuit.health())
}

fn check(name: &str, oversample: u32, budget: f64, about_operating_point: bool) {
    let (aligned, unaligned, health) = transient(name, oversample);
    let case = || {
        let c = Case::ngspice(name).budget(Metric::NormalizedRms, budget);
        if about_operating_point {
            c.about_operating_point()
        } else {
            c
        }
    };
    let raw = case()
        .named(format!("{name}/dk {oversample}x ADAA, unaligned"))
        .compare(&unaligned);
    println!("  unaligned (half a sample late): {:.3e}", raw.error);
    println!(
        "  DK solve: {:.2} iterations/solve, {} of {} capped, {} continuations",
        health.newton.mean_iterations(),
        health.newton.capped,
        health.newton.solves,
        health.continuations
    );
    assert_eq!(health.newton.capped, 0, "{name} hit its iteration cap");
    case()
        .named(format!("{name}/dk {oversample}x ADAA"))
        .compare(&aligned)
        .assert();
}

/// Plain: 5e-4 at 4× (`dk_matches_ngspice.rs`). Measured 6.3e-4 under
/// ADAA (1.8e-2 unaligned), by Lobatto quadrature: the series
/// resistances make the pair a two-port.
#[test]
fn diode_clipper_with_adaa_matches_ngspice() {
    check("diode_clipper", 4, 8e-4, false);
}

/// Plain: 5e-5 at 16×. The closed-form one-port: measured 2.2e-4 under
/// ADAA (5.5e-3 unaligned), 1.1e-4 at 32× — the averaging's error at the
/// clipping knees falls as h, not h².
#[test]
fn opamp_clipper_with_adaa_matches_ngspice() {
    check("opamp_clipper", 16, 3e-4, false);
}

/// Plain: 1e-5 at 4× about the operating point. Measured 1.4e-4 under
/// ADAA (1.6e-2 unaligned), 3.6e-5 at 8× (h²): the averaging filter's
/// `cos(ωT/2)` on the 1 kHz tone and its harmonics.
#[test]
fn common_cathode_with_adaa_matches_ngspice() {
    check("common_cathode", 4, 2e-4, true);
}

/// Plain: 4e-5 at 4× (`transistors_match_ngspice.rs`). Measured 4.4e-3
/// under ADAA (1.8e-2 unaligned), 1.3e-3 at 8×: the fuzz's square edges
/// are where `1.5·T` and the averaging cost most.
#[test]
fn fuzz_face_with_adaa_matches_ngspice() {
    check("fuzz_face", 4, 6e-3, false);
}

/// Plain: 6e-5 at 16×. Measured 1.2e-4 under ADAA (1.1e-3 unaligned).
#[test]
fn tube_screamer_with_adaa_matches_ngspice() {
    check("tube_screamer", 16, 1.6e-4, false);
}

/// The wave-digital clipper with ADAA at its root (Albertini et al.),
/// against the same deck at 4× (plain: 5e-4 with either pair solver).
/// Its input is analytic, so it is fed half a sample early instead of
/// read late.
#[test]
fn wdf_diode_clipper_with_adaa_matches_ngspice() {
    use stave_dsp::{DiodeClipper, DiodeClipperConfig, PairSolver, Shockley, Stage};
    use stave_test::reference::{Drive, Stimulus};
    let rate = 48_000.0 * 4.0;
    for (name, solver) in [
        ("diode_clipper/newton ADAA", PairSolver::default()),
        ("diode_clipper/wright_omega ADAA", PairSolver::WrightOmega),
    ] {
        let mut clipper = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::N4148,
            solver,
        });
        clipper.set_antialiasing(Antialiasing::Adaa);
        clipper.prepare(rate as f32);
        let early = Stimulus::new("1 V, 1 kHz, half a sample early", move |t| {
            (std::f64::consts::TAU * 1_000.0 * (t + 0.5 / rate)).sin()
        });
        Case::ngspice("diode_clipper")
            .named(name)
            .stimulus(early)
            .budget(Metric::NormalizedRms, WDF_BUDGET)
            .stage(Drive::oversampled(4), |x| f64::from(clipper.tick(x as f32)))
            .assert();
        assert_eq!(clipper.stats().capped, 0);
    }
}
/// Measured 6.9e-4 (Newton) under ADAA.
const WDF_BUDGET: f64 = 9e-4;
