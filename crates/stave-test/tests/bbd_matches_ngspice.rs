//! H1: the BBD delay's anti-alias and reconstruction filters
//! (`stave_dsp::circuit::topologies::bucket_brigade`) against ngspice's
//! AC analysis of the same circuits (`spice/bbd_input_filter.cir`,
//! `spice/bbd_output_filter.cir`): the partial fractions the clocked
//! line runs, built from the topology's part values, and `Circuit<Dk>`
//! on the deck itself. The decks' parts are read back and must be the
//! topology's, so the two cannot drift apart.

use std::path::PathBuf;

use stave_dsp::circuit::devices::bbd::{Filter, SallenKey};
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::{ElementKind, Netlist};
use stave_dsp::circuit::topologies::bucket_brigade::{INPUT_FILTER, OUTPUT_FILTER};
use stave_dsp::circuit::Circuit;
use stave_test::reference::{Case, Metric};

fn deck(name: &str) -> Netlist {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.cir"));
    Netlist::parse_spice(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The deck's parts are the topology's: R1–R4 and C1–C4, section by
/// section.
fn same_parts(netlist: &Netlist, sections: &[SallenKey; 2]) {
    let ohms = |name: &str| match netlist.element(name).unwrap().kind {
        ElementKind::Resistor { ohms, .. } => ohms,
        ref other => panic!("{name}: {other:?}"),
    };
    let farads = |name: &str| match netlist.element(name).unwrap().kind {
        ElementKind::Capacitor { farads, .. } => farads,
        ref other => panic!("{name}: {other:?}"),
    };
    for (i, s) in sections.iter().enumerate() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs();
        assert!(close(ohms(&format!("R{}", 2 * i + 1)), s.r1.get()), "section {i} r1");
        assert!(close(ohms(&format!("R{}", 2 * i + 2)), s.r2.get()), "section {i} r2");
        assert!(close(farads(&format!("C{}", 2 * i + 1)), s.c1.get()), "section {i} c1");
        assert!(close(farads(&format!("C{}", 2 * i + 2)), s.c2.get()), "section {i} c2");
    }
}

/// Both filters, 20 Hz–100 kHz: the partial fractions within 1e-6 dB of
/// ngspice (measured ~1e-10: the closed form is the circuit's), and
/// `Circuit<Dk>`'s analog response on the deck within the same.
#[test]
fn filters_match_ngspice() {
    for (name, sections) in [("bbd_input_filter", &INPUT_FILTER), ("bbd_output_filter", &OUTPUT_FILTER)] {
        let netlist = deck(name);
        same_parts(&netlist, sections);
        let filter = Filter::sallen_key(sections).unwrap();
        let worst = Case::ngspice(name)
            .named(format!("{name}/partial fractions"))
            .budget(Metric::MaxDb, 1e-6)
            .pointwise(|hz| {
                let p = filter.response(hz);
                p.re.hypot(p.im)
            })
            .assert();
        println!("{name}: partial fractions {worst:.2e} dB");
        let circuit = Circuit::<Dk>::new(&netlist).unwrap();
        Case::ngspice(name)
            .named(format!("{name}/dk"))
            .budget(Metric::MaxDb, 1e-6)
            .pointwise(|hz| circuit.analog_response(hz).unwrap().magnitude())
            .assert();
    }
}

/// The NE570 compressor's hot onset from rest (`spice/ne570_onset.cir`:
/// the same rectifier, cell and limits as a continuous-time loop around
/// the LM741 macromodel, the sheet's "equivalent to a 741"): 2 V DC after
/// silence rides the rail while the rectifier charges, then the loop
/// closes and the 2:1 law takes over. The sample model at 48 kHz against
/// ngspice: the time on the rail within a sample (measured: both 4.90
/// ms), and the waveform within 4.5 % normalized RMS (measured 2.9 %,
/// nearly all of it the two samples where the 741 recovers from
/// saturation (its loop, slower than the slew) while the sample model
/// closes at once), within 0.5 % pointwise from 5 ms on (measured 0.31 %).
#[test]
fn ne570_onset_matches_ngspice() {
    use stave_dsp::circuit::devices::compander::{Chip, Compander, Mode};
    use stave_dsp::Stage;
    let mut c = Compander::new(Chip::NE570, Mode::Compress);
    c.prepare(48_000.0);
    let case = Case::ngspice("ne570_onset").budget(Metric::NormalizedRms, 0.045);
    let run: Vec<f64> = case.reference().values().iter().map(|_| c.tick_f64(2.0)).collect();
    // The rail: above 6 V (the macromodel's soft clamp sits 45 mV
    // inside the swing).
    let railed = |v: &[f64]| v.iter().filter(|&&y| y > 6.0).count();
    let (spice, model) = (railed(case.reference().values()), railed(&run));
    println!("ne570_onset: on the rail {spice} samples (ngspice), {model} (model)");
    assert!(spice.abs_diff(model) <= 1, "on the rail: ngspice {spice} samples, model {model}");
    let tail = case.reference().values().len() / 4;
    let late: f64 = case.reference().values()[tail..]
        .iter()
        .zip(&run[tail..])
        .map(|(a, b)| ((a - b) / a).abs())
        .fold(0.0, f64::max);
    println!("ne570_onset: after the loop closes, within {late:.2e} of ngspice");
    assert!(late < 5e-3, "after the loop closes: {late}");
    case.named("ne570_onset/compander").compare(&run).assert();
}

/// The partial fractions' phase is the circuit's too (ngspice gives the
/// magnitude; `Circuit<Dk>` the full phasor, at every decade point).
#[test]
fn filter_phase_matches_the_circuit() {
    for (name, sections) in [("bbd_input_filter", &INPUT_FILTER), ("bbd_output_filter", &OUTPUT_FILTER)] {
        let circuit = Circuit::<Dk>::new(&deck(name)).unwrap();
        let filter = Filter::sallen_key(sections).unwrap();
        for hz in [20.0, 200.0, 2_000.0, 6_000.0, 10_000.0, 20_000.0, 100_000.0] {
            let (a, b) = (filter.response(hz), circuit.analog_response(hz).unwrap());
            let (pa, pb) = (a.im.atan2(a.re), b.im.atan2(b.re));
            let d = (pa - pb + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            assert!(d.abs() < 1e-9, "{name} at {hz} Hz: {pa} vs {pb}");
        }
    }
}
