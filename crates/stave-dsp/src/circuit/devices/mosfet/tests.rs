use super::*;

const PARTS: [ShichmanHodges; 4] = [
    ShichmanHodges::VN10KM,
    ShichmanHodges::BSS138,
    ShichmanHodges::CD4007_N,
    ShichmanHodges::CD4007_P,
];

fn points(part: &ShichmanHodges) -> Vec<(f64, f64, f64)> {
    let s = part.polarity.sign();
    let mut out = Vec::new();
    for vgs in [0.5, 1.9, 2.6, 4.1, 7.3] {
        for vds in [-2.7, -0.3, 0.1, 0.9, 3.3, 9.0] {
            for vbs in [-3.1, -0.4, 0.0, 0.2] {
                out.push((s * vgs, s * vds, s * vbs));
            }
        }
    }
    out
}

#[test]
fn every_part_s_partials_match_finite_differences() {
    for part in PARTS {
        for (vgs, vds, vbs) in points(&part) {
            let c = part.currents(vgs, vds, vbs);
            let h = 1e-7;
            let d = |f: &dyn Fn(f64) -> f64, x: f64| (f(x + h) - f(x - h)) / (2.0 * h);
            let close = |a: f64, n: f64| (a - n).abs() <= 1e-5 * a.abs().max(n.abs()) + 1e-9;
            let g = d(&|x| part.currents(x, vds, vbs).drain, vgs);
            let ds = d(&|x| part.currents(vgs, x, vbs).drain, vds);
            let b = d(&|x| part.currents(vgs, vds, x).drain, vbs);
            assert!(close(c.drain_per_vgs, g), "{vgs} {vds} {vbs} {c:?} {g}");
            assert!(close(c.drain_per_vds, ds), "{vgs} {vds} {vbs} {c:?} {ds}");
            assert!(close(c.drain_per_vbs, b), "{vgs} {vds} {vbs} {c:?} {b}");
        }
    }
}

#[test]
fn a_p_channel_is_its_n_twin_mirrored() {
    for part in PARTS {
        let n = ShichmanHodges { polarity: Polarity::N, threshold_volts: part.threshold_volts.abs(), ..part };
        let p = ShichmanHodges { polarity: Polarity::P, threshold_volts: -part.threshold_volts.abs(), ..part };
        for (vgs, vds, vbs) in points(&n) {
            let (a, b) = (n.currents(vgs, vds, vbs), p.currents(-vgs, -vds, -vbs));
            assert_eq!(a.drain, -b.drain);
            assert_eq!(a.drain_per_vds, b.drain_per_vds);
        }
    }
}

#[test]
fn the_square_law_and_body_effect_read_as_the_card() {
    let m = ShichmanHodges { channel_modulation: 0.0, ..ShichmanHodges::BSS138 };
    // Saturation at vgs = Vto + 1: β/2.
    let id = m.currents(2.2, 5.0, 0.0).drain;
    assert!((id - 7.5e-3 / 2.0).abs() < 1e-15, "{id}");
    // Bulk 1 V below the source raises the threshold by γ(√(φ+1) − √φ).
    let shift = 1.49 * ((0.75f64 + 1.0).sqrt() - 0.75f64.sqrt());
    let lifted = m.currents(2.2 + shift, 5.0, -1.0).drain;
    assert!((lifted - id).abs() < 1e-12, "{lifted} {id}");
    assert_eq!(m.currents(1.1, 5.0, 0.0).drain, 0.0);
}
