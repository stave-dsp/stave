//! The ngspice waveforms are only worth comparing against if ngspice is
//! right on circuits with exact answers; this pins that before any
//! stave topology is judged by them. Regenerate with
//! scripts/ngspice-render.py.

use std::path::PathBuf;

use stave_measure::waveform::Waveform;

fn ngspice(name: &str) -> Waveform {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.ngspice.txt"));
    Waveform::read(&path).unwrap()
}

#[test]
fn rc_step_matches_its_analytic_response() {
    let g = ngspice("rc_step");
    let rc = 1e3 * 100e-9;
    let exact: Vec<f64> = g.time_s.iter().map(|t| 1.0 - (-t / rc).exp()).collect();
    let d = g.difference(&exact);
    assert_eq!(d.points, 241);
    assert!(d.max_abs < 5e-5, "{d:?}");
}

#[test]
fn diode_clipper_clips_symmetrically_near_a_diode_drop() {
    let g = ngspice("diode_clipper");
    let settled = &g.value[g.value.len() / 2..];
    let (max, min) = settled
        .iter()
        .fold((f64::MIN, f64::MAX), |(hi, lo), &v| (hi.max(v), lo.min(v)));
    assert!((0.4..0.7).contains(&max), "max {max}");
    assert!((max + min).abs() < 0.01, "asymmetric: {max} {min}");
}

/// The WDF clipper at 4× the waveform's rate, read at its instants:
/// ngspice samples a continuous waveform, so a decimated
/// render without anti-alias filtering is the like-for-like comparison.
fn wdf_clipper_deviation(solver: stave_dsp::PairSolver) -> stave_measure::waveform::Difference {
    use stave_dsp::{DiodeClipper, DiodeClipperConfig, Shockley, Stage};
    let g = ngspice("diode_clipper");
    let oversample = 4;
    let rate = 48_000.0 * oversample as f64;
    let mut clipper = DiodeClipper::new(DiodeClipperConfig {
        input_ohms: 2.2e3,
        capacitor_farads: 10e-9,
        diode: Shockley::D1N4148,
        solver,
    });
    clipper.prepare(rate as f32);
    let frames = g.value.len() * oversample;
    let render: Vec<f64> = (0..frames)
        .map(|n| {
            let t = n as f64 / rate;
            f64::from(clipper.tick((std::f64::consts::TAU * 1_000.0 * t).sin() as f32))
        })
        .step_by(oversample)
        .collect();
    assert_eq!(clipper.stats().capped, 0);
    g.difference(&render)
}

#[test]
fn the_wdf_diode_clipper_matches_spice_with_either_solver() {
    let newton = wdf_clipper_deviation(stave_dsp::PairSolver::default());
    assert!(newton.normalized_rms < 0.002, "{newton:?}");
    let closed = wdf_clipper_deviation(stave_dsp::PairSolver::WrightOmega);
    assert!(closed.normalized_rms < 0.002, "{closed:?}");
}

#[test]
fn the_common_cathode_stage_matches_spice() {
    use stave_dsp::{CommonCathode, CommonCathodeConfig, Koren, Stage};
    let g = ngspice("common_cathode");
    let oversample = 4;
    let rate = 48_000.0 * oversample as f64;
    let mut stage = CommonCathode::new(CommonCathodeConfig {
        tube: Koren::T12AX7,
        source_ohms: 0.0,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    stage.prepare(rate as f32);
    assert!(
        (stage.plate_volts() - g.value[0]).abs() < 1e-6,
        "operating point"
    );
    let frames = g.value.len() * oversample;
    let render: Vec<f64> = (0..frames)
        .map(|n| {
            let grid = 0.5 * (std::f64::consts::TAU * 1_000.0 * n as f64 / rate).sin();
            f64::from(stage.tick(grid as f32))
        })
        .step_by(oversample)
        .collect();
    let swing = |v: &[f64]| v.iter().map(|x| x - g.value[0]).collect::<Vec<_>>();
    let ac = stave_measure::waveform::Waveform {
        time_s: g.time_s.clone(),
        value: swing(&g.value),
    };
    let d = ac.difference(&swing(&render));
    assert!(d.normalized_rms < 1e-4, "{d:?}");
    assert_eq!(stage.stats().capped, 0);
}

/// Dempwolf's triode with grid current, driven 3 V peak through 33 kΩ:
/// positive peaks draw grid current, which drops volts across the source
/// resistance and flattens the plate's negative swing.
#[test]
fn the_common_cathode_stage_with_grid_current_matches_spice() {
    use stave_dsp::{CommonCathode, CommonCathodeConfig, Dempwolf, Stage};
    let g = ngspice("common_cathode_grid_current");
    let oversample = 4;
    let rate = 48_000.0 * oversample as f64;
    let mut stage = CommonCathode::new(CommonCathodeConfig {
        tube: Dempwolf::RSD_1,
        source_ohms: 33e3,
        supply_volts: 300.0,
        plate_load_ohms: 100e3,
        cathode_ohms: 1.5e3,
        bypass_farads: 22e-6,
        newton: Default::default(),
    });
    stage.prepare(rate as f32);
    assert!(
        (stage.plate_volts() - g.value[0]).abs() < 1e-6,
        "operating point {} vs {}",
        stage.plate_volts(),
        g.value[0]
    );
    let frames = g.value.len() * oversample;
    let render: Vec<f64> = (0..frames)
        .map(|n| {
            let input = 3.0 * (std::f64::consts::TAU * 1_000.0 * n as f64 / rate).sin();
            f64::from(stage.tick(input as f32))
        })
        .step_by(oversample)
        .collect();
    let swing = |v: &[f64]| v.iter().map(|x| x - g.value[0]).collect::<Vec<_>>();
    let ac = stave_measure::waveform::Waveform {
        time_s: g.time_s.clone(),
        value: swing(&g.value),
    };
    let d = ac.difference(&swing(&render));
    assert!(d.normalized_rms < 1e-4, "{d:?}");
    assert_eq!(stage.stats().capped, 0);
}

/// A CLC B+ supply from switch-on: inrush, the choke's ring (the LC
/// rings B+ past the rectified peak, to 642 V), ripple at twice mains,
/// and sag and recovery around a 20 → 80 → 20 mA load step.
#[test]
fn the_power_supply_matches_spice() {
    use stave_dsp::{PowerSupply, PowerSupplyConfig};
    let g = ngspice("power_supply");
    let oversample = 4;
    let rate = 48_000.0 * oversample as f64;
    let mut supply = PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE);
    supply.prepare(rate);
    let load = |t: f64| {
        if (0.3..0.4).contains(&t) {
            80e-3
        } else {
            20e-3
        }
    };
    // Tick i ends at (i + 1)/rate; ngspice's points are at k/48 kHz, k ≥ 1.
    let render: Vec<f64> = (0..g.value.len() * oversample)
        .map(|i| supply.tick(load((i + 1) as f64 / rate)))
        .skip(oversample - 1)
        .step_by(oversample)
        .collect();
    let d = g.difference(&render);
    assert!(d.normalized_rms < 1e-4, "{d:?}");
    assert_eq!(supply.stats().capped, 0);
}

/// The passive treble/bass/mid stack's network function (Yeh & Smith's
/// closed form) against ngspice's AC analysis of the circuit itself, at
/// three pot settings (these waveforms' first column is frequency).
#[test]
fn the_tone_stack_matches_spice() {
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
        ("tone_stack_mid", 0.5, 0.5, 0.5),
        ("tone_stack_bright", 0.9, 0.1, 0.7),
        ("tone_stack_dark", 0.2, 0.8, 0.3),
    ] {
        let g = ngspice(name);
        let mut stack = PassiveToneStack::new(components);
        stack.set_controls(PassiveToneControlPositions::new(treble, bass, midrange));
        let worst_db = g
            .time_s
            .iter()
            .zip(&g.value)
            .map(|(&hz, &spice)| (20.0 * (stack.circuit_magnitude_at(hz) / spice).log10()).abs())
            .fold(0.0, f64::max);
        assert!(worst_db < 1e-9, "{name}: {worst_db} dB");
    }
}

/// The coupling cap into its grid leak, from rest.
#[test]
fn the_coupler_matches_spice() {
    use stave_dsp::{Coupler, Stage};
    let g = ngspice("coupler");
    let corner_hz = 1.0 / (std::f64::consts::TAU * 22e-9 * 1e6);
    let mut coupler = Coupler::new(corner_hz as f32);
    coupler.prepare(48_000.0);
    let render: Vec<f64> = g
        .time_s
        .iter()
        .map(|&t| f64::from(coupler.tick((std::f64::consts::TAU * 50.0 * t).sin() as f32)))
        .collect();
    let d = g.difference(&render);
    assert!(d.normalized_rms < 1e-5, "{d:?}");
}

/// The triode stage's tabulated half (Koren 12AX7 plate, Dempwolf grid
/// current through the 68 kΩ stopper, fixed bias) against an ngspice DC
/// sweep. The operating point must agree absolutely; the curve agrees up
/// to the stage's level calibration, one fitted scale factor.
#[test]
fn the_triode_stage_static_transfer_matches_spice() {
    use stave_dsp::Triode;
    let g = ngspice("triode_stage");
    let mut stage = Triode::new();
    stage.prepare(48_000.0);
    stage.set_grid_drive(1.0);
    let input = |t: f64| -10.0 + 1_000.0 * t;
    let bias = stage.tube().typical_grid_bias;
    let quiescent = (0..g.time_s.len())
        .min_by(|&a, &b| {
            let off = |i: usize| (input(g.time_s[i]) - bias).abs();
            off(a).total_cmp(&off(b))
        })
        .unwrap();
    assert!((input(g.time_s[quiescent]) - bias).abs() < 1e-6);
    assert!(
        (stage.operating_point() - g.value[quiescent]).abs() < 1e-6,
        "operating point {} vs {}",
        stage.operating_point(),
        g.value[quiescent]
    );
    let spice_swing: Vec<f64> = g.value.iter().map(|v| g.value[quiescent] - v).collect();
    let ours: Vec<f64> = g
        .time_s
        .iter()
        .map(|&t| stage.static_transfer(input(t) - bias))
        .collect();
    let scale = ours
        .iter()
        .zip(&spice_swing)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        / spice_swing.iter().map(|b| b * b).sum::<f64>();
    let fitted = stave_measure::waveform::Waveform {
        time_s: g.time_s.clone(),
        value: spice_swing.iter().map(|b| scale * b).collect(),
    };
    let d = fitted.difference(&ours);
    assert!(scale > 0.0);
    assert!(d.normalized_rms < 1e-6, "{d:?}");
}

/// The push-pull pair's static transfer — two halves at the class point,
/// grids driven in antiphase through the phase inverter's 40 kΩ — against
/// the ngspice plate difference, up to the pair's level calibration.
#[test]
fn the_push_pull_static_transfer_matches_spice() {
    use stave_dsp::PushPull;
    let g = ngspice("push_pull");
    let mut pair = PushPull::new();
    pair.prepare(48_000.0);
    let swing = |t: f64| -16.0 + 1_600.0 * t;
    let ours: Vec<f64> = g
        .time_s
        .iter()
        .map(|&t| pair.static_transfer(swing(t)))
        .collect();
    let scale = ours.iter().zip(&g.value).map(|(a, b)| a * b).sum::<f64>()
        / g.value.iter().map(|b| b * b).sum::<f64>();
    let fitted = stave_measure::waveform::Waveform {
        time_s: g.time_s.clone(),
        value: g.value.iter().map(|b| scale * b).collect(),
    };
    let d = fitted.difference(&ours);
    assert!(scale > 0.0);
    assert!(d.normalized_rms < 1e-5, "{d:?}");
}

/// The op-amp diode clipper (TS808 values, ideal op-amp) against ngspice.
/// Its feedback pole (51 kΩ · 51 pF = 2.6 µs) is faster than a 4× step, so
/// the limit is set at 16× and the error must fall as the trapezoidal rule's
/// h² — converging to the reference, not to a model of its own.
#[test]
fn the_opamp_clipper_matches_spice() {
    use stave_dsp::{OpAmpClipper, OpAmpClipperConfig, Stage};
    let g = ngspice("opamp_clipper");
    let deviation = |oversample: usize| {
        let rate = 48_000.0 * oversample as f64;
        let mut stage = OpAmpClipper::new(OpAmpClipperConfig::TS808);
        stage.prepare(rate as f32);
        let render: Vec<f64> = (0..g.value.len() * oversample)
            .map(|n| {
                stage.process_volts(0.5 * (std::f64::consts::TAU * 1_000.0 * n as f64 / rate).sin())
            })
            .step_by(oversample)
            .collect();
        assert_eq!(stage.stats().capped, 0);
        g.difference(&render).normalized_rms
    };
    let (coarse, fine) = (deviation(16), deviation(32));
    assert!(coarse < 5e-5, "16x: {coarse}");
    assert!(
        fine < coarse / 3.0,
        "not converging: 16x {coarse}, 32x {fine}"
    );
}
