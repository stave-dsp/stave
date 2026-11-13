use super::*;
use crate::circuit::methods::dk::Dk;
use crate::circuit::netlist::{ElementKind, Netlist, Node, Probe, Source};

/// A unity follower of `amp` on ±15 V, driven by an AC source.
fn follower(amp: Amp) -> Dk {
    let mut n = Netlist::new();
    let (input, out) = (n.add_node("in"), n.add_node("out"));
    let source = Source {
        ac: 1.0,
        ..Source::dc(0.0)
    };
    n.add(
        "v1",
        ElementKind::VoltageSource {
            a: input,
            b: Node::GROUND,
            source,
        },
    )
    .unwrap();
    n.add(
        "u1",
        ElementKind::OpAmp {
            plus: input,
            minus: out,
            output: out,
            amp,
            supply: (-15.0, 15.0),
        },
    )
    .unwrap();
    n.probe = Some(Probe::node(out));
    Dk::new(&n).unwrap()
}

/// Closed in a follower, the macromodel's error at DC is 1/(1 + A0) and
/// its −3 dB point is the gain-bandwidth: the datasheet's figures, read
/// back from the circuit.
#[test]
fn a_follower_reads_back_the_datasheet_gain_and_bandwidth() {
    for part in Macromodel::ALL {
        let dk = follower(Amp::Macromodel(part));
        let error = 1.0 - dk.analog_response(1.0).unwrap().magnitude();
        let expected = 1.0 / (1.0 + part.gain);
        assert!(
            (error / expected - 1.0).abs() < 0.01,
            "{}: {error:e}",
            part.name
        );
        let gbw = f64::from(part.gain_bandwidth.get());
        let at = dk.analog_response(gbw).unwrap().magnitude();
        assert!(
            (at - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "{}: {at}",
            part.name
        );
        // The offset rides through to the output.
        assert!(
            (dk.output() - part.offset.get()).abs() < 1e-6,
            "{}: {}",
            part.name,
            dk.output()
        );
    }
    let ideal = follower(Amp::Ideal(Ideal));
    assert!((ideal.analog_response(1e6).unwrap().magnitude() - 1.0).abs() < 1e-12);
    assert!(Ideal.is_ideal() && !Macromodel::TL072.is_ideal());
}

/// The gain stage's partials are its derivatives, inside and past the
/// rails.
#[test]
fn the_stage_partials_are_the_derivatives() {
    let stage = Macromodel::JRC4558.prepare(0.0, 9.0);
    for (vd, vn) in [
        (0.0, 4.5),
        (0.01, 7.9),
        (-0.2, 1.05),
        (1e-4, 8.2),
        (0.0, -0.3),
    ] {
        let c = stage.current(vd, vn);
        let h = 1e-7;
        let dvd = (stage.current(vd + h, vn).amps - stage.current(vd - h, vn).amps) / (2.0 * h);
        let dvn = (stage.current(vd, vn + h).amps - stage.current(vd, vn - h).amps) / (2.0 * h);
        assert!(
            (c.per_input_volt - dvd).abs() <= 1e-5 * dvd.abs() + 1e-12,
            "{vd} {vn}"
        );
        assert!(
            (c.per_node_volt - dvn).abs() <= 1e-5 * dvn.abs() + 1e-12,
            "{vd} {vn}"
        );
    }
    assert_eq!((stage.low, stage.high), (1.0, 8.0));
}
