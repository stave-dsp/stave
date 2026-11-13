use super::*;
use crate::circuit::methods::wdf::elements::{
    sample, Capacitor, Parallel, ResistiveSource, Resistor, Series, VoltageSource,
};

fn boxed(p: impl OnePort + Send + 'static) -> Box<dyn OnePort + Send> {
    Box::new(p)
}

/// Two ports on one node pair: the R-type adaptor is the parallel
/// adaptor, wave for wave; two ports in a loop through a middle node,
/// the series adaptor.
#[test]
fn reduces_to_the_series_and_parallel_adaptors() {
    let source = || ResistiveSource { ohms: 1e3, volts: 0.7 };
    let mut parallel_r = RType::new(2, (1, 0), vec![((1, 0), boxed(source())), ((1, 0), boxed(Resistor { ohms: 3.3e3 }))]).unwrap();
    let mut parallel = Parallel::new(source(), Resistor { ohms: 3.3e3 });
    assert!((parallel_r.resistance() - parallel.resistance()).abs() < 1e-9);
    assert!((parallel_r.reflected() - parallel.reflected()).abs() < 1e-12);

    let mut series_r = RType::new(3, (1, 0), vec![((1, 2), boxed(source())), ((2, 0), boxed(Resistor { ohms: 3.3e3 }))]).unwrap();
    let mut series = Series::new(source(), Resistor { ohms: 3.3e3 });
    assert!((series_r.resistance() - series.resistance()).abs() < 1e-9);
    assert!((series_r.reflected() - series.reflected()).abs() < 1e-12);
    // And down, shorted at the root: the source's terminal voltage is
    // 0.7 V less its 1 kΩ's share of the loop.
    let a = series_r.reflected();
    series_r.incident(-a);
    let volts = 0.7 - 0.7 * 1e3 / 4.3e3;
    assert!((series_r.port_volts(1) - volts).abs() < 1e-12, "{} vs {volts}", series_r.port_volts(1));
}

/// The parent's port is adapted (`S₀₀ = 0`), and the adaptor is lossless:
/// `Sᵀ·G·S = G` (wave power in equals wave power out).
#[test]
fn adapted_and_lossless() {
    let r = |ohms| boxed(Resistor { ohms });
    let bridge = RType::new(4, (1, 0), vec![((1, 2), r(1e3)), ((2, 0), r(2.2e3)), ((1, 3), r(4.7e3)), ((3, 0), r(680.0)), ((2, 3), r(10e3))]).unwrap();
    let s = bridge.scattering();
    let p = 6;
    assert!(s[0].abs() < 1e-12, "S00 = {}", s[0]);
    let g: Vec<f64> = bridge.resistances().iter().map(|r| 1.0 / r).collect();
    for i in 0..p {
        for j in 0..p {
            let sgs: f64 = (0..p).map(|k| s[k * p + i] * g[k] * s[k * p + j]).sum();
            let want = if i == j { g[i] } else { 0.0 };
            assert!((sgs - want).abs() < 1e-12 * g[i].max(g[j]), "({i},{j}): {sgs} vs {want}");
        }
    }
}

/// A Wheatstone bridge driven by 1 V: every arm's voltage as nodal
/// analysis gives it.
#[test]
fn a_bridge_solves_as_nodal_analysis() {
    let (r1, r2, r3, r4, r5) = (1e3, 2.2e3, 4.7e3, 680.0, 10e3);
    let r = |ohms| boxed(Resistor { ohms });
    let mut bridge = RType::new(4, (1, 0), vec![((1, 2), r(r1)), ((2, 0), r(r2)), ((1, 3), r(r3)), ((3, 0), r(r4)), ((2, 3), r(r5))]).unwrap();
    sample(&mut bridge, &mut VoltageSource { volts: 1.0 });
    // Nodes 2 and 3 by hand: (g1 + g2 + g5)·v2 − g5·v3 = g1; −g5·v2 + (g3 + g4 + g5)·v3 = g3.
    let (g1, g2, g3, g4, g5) = (1.0 / r1, 1.0 / r2, 1.0 / r3, 1.0 / r4, 1.0 / r5);
    let (a, b, c, d) = (g1 + g2 + g5, -g5, -g5, g3 + g4 + g5);
    let det = a * d - b * c;
    let v2 = (g1 * d - b * g3) / det;
    let v3 = (a * g3 - c * g1) / det;
    for (port, want) in [(0, 1.0), (1, 1.0 - v2), (2, v2), (3, 1.0 - v3), (4, v3), (5, v2 - v3)] {
        let got = bridge.port_volts(port);
        assert!((got - want).abs() < 1e-12, "port {port}: {got} vs {want}");
    }
}

/// An inverting amplifier with the op-amp absorbed as a nullor:
/// v(out) = −(Rf/Rin)·v(in), whatever the load.
#[test]
fn an_absorbed_op_amp_inverts_with_its_feedback_ratio() {
    // Nodes: in 1, minus 2, out 3. Ports: Rin (1,2), Rf (2,3), load (3,0).
    let r = |ohms| boxed(Resistor { ohms });
    let mut amp = RType::with_op_amps(4, (1, 0), vec![((1, 2), r(10e3)), ((2, 3), r(47e3)), ((3, 0), r(2e3))], vec![(0, 2, 3)]).unwrap();
    sample(&mut amp, &mut VoltageSource { volts: 0.1 });
    assert!((amp.port_volts(3) + 0.47).abs() < 1e-12, "{}", amp.port_volts(3));
}

/// A bridged-T notch (C, C in series across the input to the output,
/// R bridging them, R to ground from their middle, a load) built as an
/// R-type adaptor of capacitors and resistors under an ideal source:
/// its steady-state response at the notch matches the analog one,
/// prewarped, to the bilinear transform's exactness.
#[test]
fn a_bridged_t_notch_matches_its_analog_response() {
    let rate = 48_000.0f32;
    let (c, rb, rg, rl) = (10e-9, 100e3, 1e3, 100e3);
    let cap = || {
        let mut k = Capacitor::new(c);
        k.prepare(rate);
        boxed(k)
    };
    let r = |ohms| boxed(Resistor { ohms });
    // Nodes: in 1, middle 2, out 3.
    let mut notch = RType::new(4, (1, 0), vec![((1, 2), cap()), ((2, 3), cap()), ((1, 3), r(rb)), ((2, 0), r(rg)), ((3, 0), r(rl))]).unwrap();
    let analog = |w: f64| {
        // Nodal analysis at s = jw, solved by Cramer's rule on 2×2 complex.
        use crate::circuit::methods::dk::Phasor;
        let y = Phasor::new(0.0, w * c);
        let (gb, gg, gl) = (Phasor::new(1.0 / rb, 0.0), Phasor::new(1.0 / rg, 0.0), Phasor::new(1.0 / rl, 0.0));
        // Middle: (2y + gg)·vm − y·vo = y;  out: −y·vm + (y + gb + gl)·vo = gb.
        let a = y.add(y).add(gg);
        let b = Phasor::new(-y.re, -y.im);
        let d = y.add(gb).add(gl);
        let det = a.mul(d).sub(b.mul(b));
        a.mul(gb).sub(b.mul(y)).div(det).magnitude()
    };
    let hz = 1_591.549_430_918_953_4; // 1/(2π·C·√(Rb·Rg))
    let w = std::f64::consts::TAU * hz / f64::from(rate);
    // Run 0.2 s to settle, then fit the output's amplitude over 0.2 s.
    let n = 19_200;
    let (mut si, mut co) = (0.0, 0.0);
    for k in 0..n {
        let mut src = VoltageSource { volts: (w * k as f64).sin() };
        sample(&mut notch, &mut src);
        if k >= n / 2 {
            let v = notch.port_volts(5);
            si += v * (w * k as f64).sin();
            co += v * (w * k as f64).cos();
        }
    }
    let peak = 2.0 * (si * si + co * co).sqrt() / (n / 2) as f64;
    let warped = 2.0 * f64::from(rate) * (std::f64::consts::PI * hz / f64::from(rate)).tan();
    let want = analog(warped);
    assert!(want < 0.1, "a notch: {want}");
    assert!((peak - want).abs() < 1e-3 * want + 1e-6, "{peak} vs {want}");
}
