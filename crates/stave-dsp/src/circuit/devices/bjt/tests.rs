use super::*;

const PARTS: [GummelPoon; 5] = [
    GummelPoon::N2N3904,
    GummelPoon::N2N5088,
    GummelPoon::N2N3392,
    GummelPoon::AC128,
    GummelPoon::OC44,
];

/// Junction voltages through cutoff, active, saturation and reverse
/// active, in the part's own sense.
fn points(part: &impl Bjt) -> Vec<(f64, f64)> {
    let s = part.polarity().sign();
    let mut out = Vec::new();
    for vbe in [-2.0, -0.2, 0.1, 0.35, 0.6, 0.72] {
        for vbc in [-9.0, -1.0, -0.1, 0.2, 0.5, 0.68] {
            out.push((s * vbe, s * vbc));
        }
    }
    out
}

fn check_partials(part: &impl Bjt) {
    for (vbe, vbc) in points(part) {
        let c = part.currents(vbe, vbc);
        // A germanium junction at silicon's forward voltages carries
        // hundreds of amps: outside any card's fit, and only roundoff.
        if c.collector.abs().max(c.base.abs()) > 0.5 {
            continue;
        }
        let h = 1e-7;
        let (pe, me) = (part.currents(vbe + h, vbc), part.currents(vbe - h, vbc));
        let (pc, mc) = (part.currents(vbe, vbc + h), part.currents(vbe, vbc - h));
        let close = |analytic: f64, numeric: f64| {
            (analytic - numeric).abs() <= 1e-5 * analytic.abs().max(numeric.abs()) + 1e-9
        };
        let d = |p: f64, m: f64| (p - m) / (2.0 * h);
        assert!(close(c.collector_per_vbe, d(pe.collector, me.collector)), "{vbe} {vbc} {c:?}");
        assert!(close(c.collector_per_vbc, d(pc.collector, mc.collector)), "{vbe} {vbc} {c:?}");
        assert!(close(c.base_per_vbe, d(pe.base, me.base)), "{vbe} {vbc} {c:?}");
        assert!(close(c.base_per_vbc, d(pc.base, mc.base)), "{vbe} {vbc} {c:?}");
    }
}

#[test]
fn every_part_s_partials_match_finite_differences() {
    for part in PARTS {
        check_partials(&part);
    }
    check_partials(&EbersMoll::DEFAULT_NPN);
}

#[test]
fn a_pnp_is_its_npn_twin_mirrored() {
    for part in PARTS {
        let (n, p) = (
            GummelPoon { polarity: Polarity::N, ..part },
            GummelPoon { polarity: Polarity::P, ..part },
        );
        for (vbe, vbc) in points(&n) {
            let (a, b) = (n.currents(vbe, vbc), p.currents(-vbe, -vbc));
            assert_eq!(a.collector, -b.collector);
            assert_eq!(a.base, -b.base);
            assert_eq!(a.collector_per_vbe, b.collector_per_vbe);
        }
    }
}

#[test]
fn ebers_moll_is_gummel_poon_s_limit() {
    for polarity in [Polarity::N, Polarity::P] {
        let em = EbersMoll {
            polarity,
            saturation_amps: 5e-15,
            forward_beta: 250.0,
            reverse_beta: 3.0,
            thermal_volts: 0.025_85,
        };
        let gp = GummelPoon::from(em);
        for (vbe, vbc) in points(&em) {
            let (a, b) = (em.currents(vbe, vbc), gp.currents(vbe, vbc));
            let close = |x: f64, y: f64| (x - y).abs() <= 1e-12 * x.abs().max(y.abs()) + 1e-30;
            assert!(close(a.collector, b.collector), "{a:?} {b:?}");
            assert!(close(a.base, b.base), "{a:?} {b:?}");
            assert!(close(a.collector_per_vbc, b.collector_per_vbc), "{a:?} {b:?}");
            assert!(close(a.base_per_vbe, b.base_per_vbe), "{a:?} {b:?}");
        }
    }
}

#[test]
fn the_2n3904_s_beta_and_early_effect_read_as_its_card() {
    let q = GummelPoon::N2N3904;
    // 1 mA-ish, Vce = 5 V: β near BF / (1 + leakage), Ic rising with Vce by ~1/VAF.
    let vbe = 0.65;
    let at = |vce: f64| q.currents(vbe, vbe - vce);
    let (a, b) = (at(5.0), at(10.0));
    let beta = a.collector / a.base;
    assert!((100.0..420.0).contains(&beta), "β {beta} at {} A", a.collector);
    let slope = (b.collector - a.collector) / 5.0 / a.collector;
    assert!((slope - 1.0 / (74.03 + 5.0)).abs() < 2e-3, "{slope}");
    // High injection: β falls well past IKF.
    let hot = q.currents(0.85, 0.85 - 5.0);
    assert!(hot.collector / hot.base < 0.6 * beta);
}

#[test]
fn a_germanium_pnp_conducts_with_negative_terminal_voltages() {
    let q = GummelPoon::AC128;
    let c = q.currents(-0.15, 4.0);
    assert!(c.collector < -1e-4 && c.base < 0.0, "{c:?}");
    let off = q.currents(0.3, 5.0);
    assert!(off.collector.abs() < 1e-4, "{off:?}");
}

#[test]
fn limiting_bounds_both_junctions_in_the_part_s_own_sense() {
    let q = GummelPoon::AC128;
    let l = q.limit_step([-3.0, -3.0], [-0.1, -0.1]);
    assert!(l[0] > -3.0 && l[1] > -3.0, "{l:?}");
    let n = GummelPoon::N2N3904.limit_step([3.0, 0.1], [0.6, 0.0]);
    assert!(n[0] < 3.0 && n[1] == 0.1, "{n:?}");
}

/// The charges' partials — the base–emitter charge's dependence on vbc
/// through `qb` and `VTF` included — against finite differences, in
/// every region, for each part with a card's charge storage.
#[test]
fn every_part_s_charge_partials_match_finite_differences() {
    for part in PARTS {
        for (vbe, vbc) in points(&part) {
            if part.currents(vbe, vbc).collector.abs() > 0.5 {
                continue;
            }
            let q = part.charges(vbe, vbc);
            let h = 1e-7;
            let (pe, me) = (part.charges(vbe + h, vbc), part.charges(vbe - h, vbc));
            let (pc, mc) = (part.charges(vbe, vbc + h), part.charges(vbe, vbc - h));
            let d = |p: f64, m: f64| (p - m) / (2.0 * h);
            let close = |analytic: f64, numeric: f64| {
                (analytic - numeric).abs() <= 1e-5 * analytic.abs().max(numeric.abs()) + 1e-18
            };
            assert!(close(q.base_emitter_per_vbe, d(pe.base_emitter, me.base_emitter)), "{vbe} {vbc} {q:?}");
            assert!(close(q.base_emitter_per_vbc, d(pc.base_emitter, mc.base_emitter)), "{vbe} {vbc} {q:?}");
            assert!(close(q.base_collector_per_vbc, d(pc.base_collector, mc.base_collector)), "{vbe} {vbc} {q:?}");
            assert!(close(0.0, d(pe.base_collector, me.base_collector)), "{vbe} {vbc} {q:?}");
        }
    }
}
