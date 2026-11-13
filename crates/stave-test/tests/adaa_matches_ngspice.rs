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
/// resistances make the pair a two-port, and the card's junction charge
/// has no closed form either (its own test below, where the charge
/// decides the waveform; here the 10 nF shunt does).
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

/// `netlist` with its diodes' stored charge (depletion and transit
/// time) taken out.
fn without_charge(netlist: &Netlist) -> Netlist {
    use stave_dsp::circuit::netlist::ElementKind;
    let mut out = netlist.clone();
    let names: Vec<String> = netlist.elements().iter().map(|e| e.name.clone()).collect();
    for name in names {
        let mut kind = out.element(&name).unwrap().kind.clone();
        if let ElementKind::Diode { diode, .. } = &mut kind {
            diode.capacitance.zero_bias_farads = 0.0;
            diode.transit_seconds = 0.0;
            out.remove(&name);
            out.add(&name, kind).unwrap();
        }
    }
    out
}

/// `netlist` at `oversample` × 48 kHz, plain or under ADAA, read at the
/// reference's instants (an ADAA run midpoint-aligned as in
/// [`transient`]), and the circuit after it.
fn run(name: &str, netlist: &Netlist, oversample: u32, antialiasing: Antialiasing) -> (Vec<f64>, Circuit<Dk>) {
    let reference = Reference::ngspice(name);
    let rate = 48_000.0 * f64::from(oversample);
    let mut circuit = Circuit::<Dk>::new(netlist).unwrap();
    circuit.set_antialiasing(antialiasing);
    circuit.prepare(rate as f32);
    let last = reference.abscissa().last().copied().unwrap_or(0.0);
    let samples = (last * rate).round() as usize + 3;
    let mut y = Vec::with_capacity(samples + 1);
    y.push(circuit.output());
    for _ in 0..samples {
        y.push(circuit.step(0.0));
    }
    let at = |i: isize| y[i.clamp(0, y.len() as isize - 1) as usize];
    let out = reference
        .abscissa()
        .iter()
        .map(|&t| {
            let n = (t * rate).round() as isize;
            if antialiasing == Antialiasing::None || n == 0 {
                at(n)
            } else {
                (-at(n - 1) + 9.0 * at(n) + 9.0 * at(n + 1) - at(n + 2)) / 16.0
            }
        })
        .collect();
    (out, circuit)
}

/// The normalised RMS of `model − reference`, as [`Metric::NormalizedRms`].
fn normalized_rms(model: &[f64], reference: &[f64]) -> f64 {
    let error: f64 = model.iter().zip(reference).map(|(m, r)| (m - r) * (m - r)).sum();
    let power: f64 = reference.iter().map(|r| r * r).sum();
    (error / power).sqrt()
}

/// A junction with stored charge under ADAA (DSP.md R11's open note):
/// the 1N4148 pair at 10 kHz with no shunt capacitor
/// (`diode_clipper_hf`, the full card: `Cjo`, `M`, `TT` = 20 ns), so the
/// junctions' own depletion and diffusion charge is all that stores and
/// their recovery shapes every edge. A charged junction has no
/// closed-form mean, so ADAA takes the Lobatto quadrature. The reference
/// is ngspice stepping at most 20 ns (over 1 000 steps a 48 kHz sample).
///
/// Measured, normalised RMS:
///
/// | rate | plain vs ngspice | ADAA vs ngspice | ADAA, charge removed, vs its exact solve |
/// |---|---|---|---|
/// | 4× | 3.5e-3 | 5.4e-2 | 5.3e-2 |
/// | 16× | 3.7e-4 | 9.1e-4 | 6.4e-4 |
/// | 32× | 5.3e-6 | 2.3e-4 | 2.3e-4 |
///
/// The last column is ADAA's own in-band error: without its charge the
/// pair is memoryless, so the plain solve is exact at any rate and all
/// that is left is the averaging filter's `cos(ωT/2)` on the 10 kHz tone
/// and its harmonics (`1 − cos(π·10k/768k)` = 8.4e-4 at 16×). With the
/// charge, ADAA's error is that same error to within 1.41× (at 16×; 1.02×
/// at 4× and 0.96× at 32×) and falls as h² (16× → 32×: 4.0×): the
/// quadrature carries the charge's `Qn(v)` outputs as well as the
/// currents, adding nothing of its own beyond ADAA's. ADAA's error is the
/// larger at every rate — plain oversampling is the better buy for
/// accuracy here, ADAA's case being aliasing (`aliasing_budgets.rs`).
/// Run without its charge, ADAA misses ngspice by 6.6e-3 at 16×, 5× its
/// budget, so the reference is one the charge decides. Budgets 1.4× the
/// measurements; the charged/bare ratio's at 1.6.
#[test]
fn charged_junction_with_adaa_matches_ngspice() {
    use stave_dsp::circuit::methods::dk::Mean;
    let name = "diode_clipper_hf";
    let netlist = deck(name);
    let bare = without_charge(&netlist);
    let compare = |label: &str, run: &[f64], budget: f64| {
        Case::ngspice(name)
            .named(format!("{name}/dk {label}"))
            .budget(Metric::NormalizedRms, budget)
            .compare(run)
            .assert()
    };
    let mut errors = Vec::new();
    for (oversample, plain_budget, adaa_budget) in [(4, 5e-3, 7.5e-2), (16, 5.2e-4, 1.3e-3), (32, 7.5e-6, 3.2e-4)] {
        let (plain, circuit) = run(name, &netlist, oversample, Antialiasing::None);
        assert_eq!(circuit.health().newton.capped, 0, "{name} hit its iteration cap");
        let plain = compare(&format!("{oversample}x"), &plain, plain_budget);
        let (adaa, circuit) = run(name, &netlist, oversample, Antialiasing::Adaa);
        assert_eq!(circuit.adaa_mean(), Some(Mean::Quadrature { nodes: 3 }));
        assert_eq!(circuit.health().newton.capped, 0, "{name} hit its iteration cap under ADAA");
        let adaa = compare(&format!("{oversample}x ADAA"), &adaa, adaa_budget);
        // ADAA's own error: the memoryless pair, against its exact solve.
        let (own, _) = run(name, &bare, oversample, Antialiasing::Adaa);
        let (exact, _) = run(name, &bare, oversample, Antialiasing::None);
        let own = normalized_rms(&own, &exact);
        println!("  {oversample}x: plain {plain:.3e}, ADAA {adaa:.3e}, ADAA's own (no charge) {own:.3e}");
        assert!(
            adaa < 1.6 * own,
            "{oversample}x: the charge adds error under ADAA ({adaa:.3e} vs {own:.3e} without it)"
        );
        errors.push(adaa);
    }
    assert!(errors[1] / errors[2] > 3.0, "ADAA does not converge as h²: {errors:?}");
    let (uncharged, _) = run(name, &bare, 16, Antialiasing::Adaa);
    let uncharged = Case::ngspice(name)
        .named(format!("{name}/dk 16x ADAA, no charge"))
        .budget(Metric::NormalizedRms, f64::INFINITY)
        .compare(&uncharged)
        .assert();
    println!("  16x ADAA without the charge: {uncharged:.3e}");
    assert!(uncharged > 3.0 * 1.3e-3, "the charge moves the waveform only {uncharged:e}");
}

