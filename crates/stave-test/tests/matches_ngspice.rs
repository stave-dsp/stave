//! Circuits against ngspice on the same netlist (`stave-test/spice/*.cir`,
//! simulated into `*.ngspice.txt` by scripts/ngspice-simulate.py), each a
//! `stave_test::reference::Case` with its metric and budget. The
//! waveforms are only worth comparing against if ngspice is right on
//! circuits with exact answers, so the first cases pin that.

use stave_test::reference::{assert_converges, Case, Drive, Metric, Reference, Stimulus};
use stave_dsp::units::Hz;

/// The oracle on a closed form: the RC step, 1 − e^(−t/RC).
#[test]
fn rc_step_ngspice_matches_its_closed_form() {
    let rc = 1e3 * 100e-9;
    let case = Case::ngspice("rc_step").budget(Metric::MaxAbs, 2e-5);
    assert_eq!(case.reference().len(), 241);
    case.pointwise(|t| 1.0 - (-t / rc).exp()).assert();
}

/// The oracle's AC analysis on a closed form: |1 / (1 + jωRC)|.
#[test]
fn rc_lowpass_ngspice_matches_its_closed_form() {
    let rc = 1e3 * 100e-9;
    Case::ngspice("rc_lowpass")
        .budget(Metric::MaxDb, 1e-9)
        .pointwise(|hz| 1.0 / (1.0 + (std::f64::consts::TAU * hz * rc).powi(2)).sqrt())
        .assert();
}

#[test]
fn diode_clipper_ngspice_clips_symmetrically_near_a_diode_drop() {
    let r = Reference::ngspice("diode_clipper");
    let settled = &r.values()[r.len() / 2..];
    let (max, min) = settled
        .iter()
        .fold((f64::MIN, f64::MAX), |(hi, lo), &v| (hi.max(v), lo.min(v)));
    assert!((0.4..0.7).contains(&max), "max {max}");
    assert!((max + min).abs() < 0.01, "asymmetric: {max} {min}");
}

/// The WDF clipper at 4× the waveform's rate, with either pair solver.
#[test]
fn diode_clipper_matches_ngspice() {
    use stave_dsp::{DiodeClipper, DiodeClipperConfig, PairSolver, Shockley, Stage};
    for (name, solver) in [
        ("diode_clipper/newton", PairSolver::default()),
        ("diode_clipper/wright_omega", PairSolver::WrightOmega),
    ] {
        let mut clipper = DiodeClipper::new(DiodeClipperConfig {
            input_ohms: 2.2e3,
            capacitor_farads: 10e-9,
            diode: Shockley::N4148,
            solver,
        });
        clipper.prepare(48_000.0 * 4.0);
        Case::ngspice("diode_clipper")
            .named(name)
            .stimulus(Stimulus::sine(1.0, 1_000.0))
            .budget(Metric::NormalizedRms, 5e-4)
            .stage(Drive::oversampled(4), |x| f64::from(clipper.tick(x as f32)))
            .assert();
        assert_eq!(clipper.stats().capped, 0);
    }
}

#[test]
fn common_cathode_matches_ngspice() {
    use stave_dsp::{CommonCathode, CommonCathodeConfig, Koren, Stage};
    let case = Case::ngspice("common_cathode")
        .stimulus(Stimulus::sine(0.5, 1_000.0))
        .budget(Metric::NormalizedRms, 1e-5)
        .about_operating_point();
    let mut stage = CommonCathode::new(CommonCathodeConfig {
        tube: Koren::T12AX7,
        source_ohms: 0.0,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    stage.prepare(48_000.0 * 4.0);
    let operating_point = case.reference().values()[0];
    assert!(
        (stage.plate_volts() - operating_point).abs() < 1e-6,
        "operating point {} vs {operating_point}",
        stage.plate_volts()
    );
    case.stage(Drive::oversampled(4), |x| f64::from(stage.tick(x as f32)))
        .assert();
    assert_eq!(stage.stats().capped, 0);
}

/// Dempwolf's triode with grid current, driven 3 V peak through 33 kΩ:
/// positive peaks draw grid current, which drops volts across the source
/// resistance and flattens the plate's negative swing.
#[test]
fn common_cathode_grid_current_matches_ngspice() {
    use stave_dsp::{CommonCathode, CommonCathodeConfig, Dempwolf, Stage};
    let case = Case::ngspice("common_cathode_grid_current")
        .stimulus(Stimulus::sine(3.0, 1_000.0))
        .budget(Metric::NormalizedRms, 2e-5)
        .about_operating_point();
    let mut stage = CommonCathode::new(CommonCathodeConfig {
        tube: Dempwolf::RSD_1,
        source_ohms: 33e3,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    stage.prepare(48_000.0 * 4.0);
    let operating_point = case.reference().values()[0];
    assert!(
        (stage.plate_volts() - operating_point).abs() < 1e-6,
        "operating point {} vs {operating_point}",
        stage.plate_volts()
    );
    case.stage(Drive::oversampled(4), |x| f64::from(stage.tick(x as f32)))
        .assert();
    assert_eq!(stage.stats().capped, 0);
}

/// A CLC B+ supply from switch-on: inrush, the choke's ring (the LC
/// rings B+ past the rectified peak, to 642 V), ripple at twice mains,
/// and sag and recovery around a 20 → 80 → 20 mA load step. The
/// stimulus is the load current; ngspice's points start one step in.
#[test]
fn power_supply_matches_ngspice() {
    use stave_dsp::{PowerSupply, PowerSupplyConfig, Stage};
    let mut supply = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
    supply.prepare(48_000.0 * 4.0);
    let load = |t: f64| {
        if (0.3..0.4).contains(&t) {
            80e-3
        } else {
            20e-3
        }
    };
    Case::ngspice("power_supply")
        .stimulus(Stimulus::new("load 20 mA, 80 mA from 0.3 s to 0.4 s", load))
        .budget(Metric::NormalizedRms, 2e-5)
        .stage(Drive::oversampled(4).at_step_end(), |amps| {
            supply.tick_f64(amps)
        })
        .assert();
    assert_eq!(supply.stats().capped, 0);
}

/// The passive treble/bass/mid stack's network function (Yeh & Smith's
/// closed form) against ngspice's AC analysis of the circuit itself, at
/// three pot settings.
#[test]
fn tone_stack_matches_ngspice() {
    use stave_dsp::{PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents};
    let components = PassiveToneStackComponents {
        treble_capacitance_farads: 250e-12,
        bass_capacitance_farads: 100e-9,
        midrange_capacitance_farads: 47e-9,
        treble_potentiometer_ohms: 250e3,
        bass_potentiometer_ohms: 1e6,
        midrange_potentiometer_ohms: 25e3,
        slope_resistance_ohms: 100e3,
    };
    for (name, treble, bass, midrange) in [
        ("tone_stack_duncan_fender_mid", 0.5, 0.5, 0.5),
        ("tone_stack_duncan_fender_bright", 0.9, 0.1, 0.7),
        ("tone_stack_duncan_fender_dark", 0.2, 0.8, 0.3),
    ] {
        let mut stack = PassiveToneStack::new(components);
        stack.set_controls(PassiveToneControlPositions::new(treble, bass, midrange));
        Case::ngspice(name)
            .budget(Metric::MaxDb, 1e-10)
            .pointwise(|hz| stack.circuit_magnitude_at(hz))
            .assert();
    }
}

/// The coupling cap into its grid leak, from rest.
#[test]
fn coupler_matches_ngspice() {
    use stave_dsp::{Coupler, Stage};
    let mut coupler = Coupler::new(Hz::new((1.0 / (std::f64::consts::TAU * 22e-9 * 1e6)) as f32));
    coupler.prepare(48_000.0);
    Case::ngspice("coupler")
        .stimulus(Stimulus::sine(1.0, 50.0))
        .budget(Metric::NormalizedRms, 2e-6)
        .stage(Drive::oversampled(1), |x| f64::from(coupler.tick(x as f32)))
        .assert();
}

/// The triode stage's tabulated half (Koren 12AX7 plate, Dempwolf grid
/// current through the 68 kΩ stopper, fixed bias) against an ngspice
/// sweep of the grid from −10 V to +10 V. The operating point must agree
/// absolutely; the curve agrees up to the stage's level calibration, one
/// fitted scale factor.
#[test]
fn triode_stage_static_transfer_matches_ngspice() {
    use stave_dsp::{Stage, Triode};
    let mut stage = Triode::new();
    stage.prepare(48_000.0);
    stage.set_grid_drive(1.0);
    let bias = stage.tube().typical_grid_bias;
    let grid = Stimulus::ramp(-10.0, 1_000.0);
    let r = Reference::ngspice("triode_stage");
    let quiescent = (0..r.len())
        .min_by(|&a, &b| {
            let off = |i: usize| (grid.at(r.abscissa()[i]) - bias).abs();
            off(a).total_cmp(&off(b))
        })
        .unwrap();
    assert!((grid.at(r.abscissa()[quiescent]) - bias).abs() < 1e-6);
    let plate = r.values()[quiescent];
    assert!(
        (stage.operating_point() - plate).abs() < 1e-6,
        "operating point {} vs {plate}",
        stage.operating_point()
    );
    Case::new("triode_stage", r)
        .stimulus(grid)
        .budget(Metric::NormalizedRms, 5e-7)
        .map_reference("the plate's fall from its operating point", move |v| {
            plate - v
        })
        .up_to_scale()
        .pointwise(|g| stage.static_transfer(g - bias))
        .assert();
}

/// The push-pull pair's static transfer — two halves at the class point,
/// grids driven in antiphase through the phase inverter's 40 kΩ — against
/// the ngspice plate difference, up to the pair's level calibration.
#[test]
fn push_pull_static_transfer_matches_ngspice() {
    use stave_dsp::{PushPull, Stage};
    let mut pair = PushPull::new();
    pair.prepare(48_000.0);
    Case::ngspice("push_pull")
        .stimulus(Stimulus::ramp(-16.0, 1_600.0))
        .budget(Metric::NormalizedRms, 2e-6)
        .up_to_scale()
        .pointwise(|swing| pair.static_transfer(swing))
        .assert();
}

/// E3: the push-pull pair with real pentodes (`PushPull::set_pentode`,
/// the EL34 at its sheet's fixed-bias conditions) against ngspice's
/// EL34 pair, up to the pair's level calibration. Measured 2.6e-7 (the
/// tables' interpolation over 116 grid volts).
#[test]
fn push_pull_pentode_static_transfer_matches_ngspice() {
    use stave_dsp::circuit::topologies::push_pull::PentodePair;
    use stave_dsp::{PushPull, Stage};
    let mut pair = PushPull::new();
    pair.set_pentode(PentodePair::EL34);
    pair.prepare(48_000.0);
    Case::ngspice("push_pull_pentode")
        .stimulus(Stimulus::ramp(-54.0, 5_400.0))
        .budget(Metric::NormalizedRms, 1e-6)
        .up_to_scale()
        .pointwise(|swing| pair.static_transfer(swing))
        .assert();
}

/// The op-amp diode clipper (TS808 values, ideal op-amp). Its feedback
/// pole (51 kΩ · 51 pF = 2.6 µs) is faster than a 4× step, so the limit
/// is set at 16× and the error must fall as the trapezoidal rule's h² —
/// converging to the reference, not to a model of its own.
#[test]
fn opamp_clipper_matches_ngspice() {
    use stave_dsp::{OpAmpClipper, OpAmpClipperConfig, Stage};
    let case = Case::ngspice("opamp_clipper")
        .stimulus(Stimulus::sine(0.5, 1_000.0))
        .budget(Metric::NormalizedRms, 5e-5);
    let at = |oversample: usize| {
        let mut stage = OpAmpClipper::new(OpAmpClipperConfig::TS808);
        stage.prepare(48_000.0 * oversample as f32);
        let outcome = case.stage(Drive::oversampled(oversample), |x| stage.tick_f64(x));
        assert_eq!(stage.stats().capped, 0);
        outcome
    };
    let (coarse, fine) = (at(16), at(32));
    assert_converges(&coarse, &fine, 3.0);
    coarse.assert();
}
