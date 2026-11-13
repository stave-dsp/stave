use super::*;

const PARTS: [ShichmanHodges; 5] = [
    ShichmanHodges::J201,
    ShichmanHodges::N2N5457,
    ShichmanHodges::N2N5458,
    ShichmanHodges::N2N5460,
    ShichmanHodges::J175,
];

fn points(part: &ShichmanHodges) -> Vec<(f64, f64)> {
    let s = part.polarity.sign();
    let mut out = Vec::new();
    for vgs in [-6.0, -2.5, -0.7, -0.3, 0.0, 0.4] {
        for vds in [-3.0, -0.4, 0.05, 0.3, 1.0, 9.0] {
            if vgs - vds <= 0.5 {
                out.push((s * vgs, s * (vgs - vds)));
            }
        }
    }
    out
}

#[test]
fn every_part_s_partials_match_finite_differences() {
    for part in PARTS {
        for (vgs, vgd) in points(&part) {
            let c = part.currents(vgs, vgd);
            let h = 1e-7;
            let d = |f: &dyn Fn(f64) -> f64, x: f64| (f(x + h) - f(x - h)) / (2.0 * h);
            let close = |a: f64, n: f64| (a - n).abs() <= 1e-5 * a.abs().max(n.abs()) + 1e-9;
            let dd_gs = d(&|x| part.currents(x, vgd).drain, vgs);
            let dd_gd = d(&|x| part.currents(vgs, x).drain, vgd);
            let dg_gs = d(&|x| part.currents(x, vgd).gate, vgs);
            let dg_gd = d(&|x| part.currents(vgs, x).gate, vgd);
            assert!(close(c.drain_per_vgs, dd_gs), "{vgs} {vgd} {c:?} {dd_gs}");
            assert!(close(c.drain_per_vgd, dd_gd), "{vgs} {vgd} {c:?} {dd_gd}");
            assert!(close(c.gate_per_vgs, dg_gs), "{vgs} {vgd} {c:?}");
            assert!(close(c.gate_per_vgd, dg_gd), "{vgs} {vgd} {c:?}");
        }
    }
}

#[test]
fn a_p_channel_is_its_n_twin_mirrored() {
    for part in PARTS {
        let n = ShichmanHodges { polarity: Polarity::N, ..part };
        let p = ShichmanHodges { polarity: Polarity::P, ..part };
        for (vgs, vgd) in points(&n) {
            let (a, b) = (n.currents(vgs, vgd), p.currents(-vgs, -vgd));
            assert_eq!(a.drain, -b.drain);
            assert_eq!(a.gate, -b.gate);
            assert_eq!(a.drain_per_vgd, b.drain_per_vgd);
        }
    }
}

#[test]
fn idss_is_beta_times_vto_squared_and_the_channel_is_symmetric() {
    let j = ShichmanHodges {
        channel_modulation: 0.0,
        ..ShichmanHodges::J201
    };
    let idss = j.channel(0.0, 5.0).0;
    assert!((idss - 7.0e-4 * 0.81).abs() < 1e-15, "{idss}");
    // Swapping drain and source: Id(vgs, vds) = −Id(vgd, −vds).
    for (vgs, vds) in [(-0.3, 0.2), (-0.1, 2.0), (0.0, 0.05)] {
        let (a, b) = (j.channel(vgs, vds).0, j.channel(vgs - vds, -vds).0);
        assert!((a + b).abs() < 1e-18, "{a} {b}");
    }
    assert_eq!(j.channel(-1.0, 5.0).0, 0.0);
}
