use crate::circuit::methods::adaa::Antialiasing;
use crate::circuit::methods::dk::{Dk, Mean};
use crate::circuit::methods::wdf::Wdf;
use crate::circuit::netlist::Netlist;

const CLIPPER: &str = "clipper
V1 in 0 0
R1 in out 2.2k
C1 out 0 10n
D1 out 0 DI
D2 0 out DI
.model DI D(Is=2.52n N=1.752)
.control
wrdata out v(out)
.endc";

/// The fuzz face, its RC left out as in the deck: two germanium PNPs,
/// four controlling voltages, quadrature.
const FUZZ: &str = include_str!("../../../../../../../stave-test/spice/fuzz_face_static.cir");

fn both(text: &str, source: &str) -> (Wdf, Dk) {
    let netlist = Netlist::parse_spice(text).unwrap();
    let mut wdf = Wdf::new(&netlist).unwrap();
    wdf.drive(source).unwrap();
    wdf.set_antialiasing(Antialiasing::Adaa);
    let mut dk = Dk::new(&netlist).unwrap();
    dk.drive(source).unwrap();
    dk.set_antialiasing(Antialiasing::Adaa);
    (wdf, dk)
}

/// Under ADAA the wave digital form and the DK method discretise the
/// same circuit the same way (`1.5·T`, every term at `n − ½`, the same
/// mean of the same root): they agree to the solves' tolerance, in
/// closed form and by quadrature.
#[test]
fn adaa_on_the_r_type_root_is_the_dk_method_s() {
    for (text, source, gain, mean) in [
        (CLIPPER, "v1", 6.0, Mean::ClosedForm),
        (FUZZ, "vin", 0.5, Mean::Quadrature { nodes: 3 }),
    ] {
        let (mut wdf, mut dk) = both(text, source);
        assert_eq!(wdf.adaa_mean(), Some(mean));
        assert_eq!(dk.adaa_mean(), Some(mean));
        wdf.prepare(96_000.0);
        dk.prepare(96_000.0);
        let (mut worst, mut scale): (f64, f64) = (0.0, 0.0);
        for i in 0..4_800 {
            let x = gain * ((i as f64 * 0.031).sin() + 0.3 * (i as f64 * 0.47).sin());
            let (a, b) = (wdf.step(x), dk.step(x));
            worst = worst.max((a - b).abs());
            scale = scale.max(b.abs());
        }
        assert!(worst < 1e-6 * scale.max(1.0), "{mean:?}: {worst} of {scale}");
        assert_eq!(wdf.health().newton.capped, 0);
    }
}

/// A linear circuit's response under ADAA is Holters' eq. (13), on both
/// methods.
#[test]
fn a_linear_circuit_has_the_same_response_on_both() {
    let (mut wdf, mut dk) = both(
        "rc
V1 in 0 0
R1 in out 1k
C1 out 0 22n
R2 out 0 10k
.control
wrdata out v(out)
.endc",
        "v1",
    );
    wdf.prepare(48_000.0);
    dk.prepare(48_000.0);
    for hz in [100.0, 3_000.0, 9_000.0, 15_000.0] {
        let (a, b) = (wdf.frequency_response(hz).unwrap(), dk.frequency_response(hz).unwrap());
        assert!((a.re - b.re).abs() < 1e-9 && (a.im - b.im).abs() < 1e-9, "{hz}: {a:?} vs {b:?}");
    }
}
