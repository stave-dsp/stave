use super::*;

#[test]
fn linear_cubic_is_c1_and_inverts() {
    for law in [LinearCubic::ALPHA_05A, LinearCubic::ALPHA_15A, LinearCubic::ALPHA_30A] {
        let h = 1e-7;
        for &[x, y] in law.points() {
            assert!((law.fraction(x) - y).abs() < 1e-12, "{x}");
            if x > h && x < 1.0 - h {
                let left = (law.fraction(x) - law.fraction(x - h)) / h;
                let right = (law.fraction(x + h) - law.fraction(x)) / h;
                assert!((left - right).abs() < 1e-4 * (1.0 + left.abs()), "slope jumps at {x}: {left} {right}");
            }
        }
        // The inverse lands on the fraction asked for (where the paper's
        // cubics dip a little, on one of the rotations giving it).
        for i in 0..=100 {
            let f = f64::from(i) / 100.0;
            assert!((law.fraction(law.rotation(f)) - f).abs() < 1e-9, "{f}");
        }
    }
}

#[test]
fn tracking_exact_is_the_identity() {
    for f in [0.0, 1e-4, 0.1, 0.5, 1.0] {
        assert_eq!(Tracking::EXACT.fraction(f), f);
    }
    let t = Tracking {
        error_db: 2.0,
        reference_db: 40.0,
    };
    // At −40 dB, +2 dB; at −20 dB, +1 dB; never past the end.
    assert!((20.0 * (t.fraction(0.01) / 0.01).log10() - 2.0).abs() < 1e-9);
    assert!((20.0 * (t.fraction(0.1) / 0.1).log10() - 1.0).abs() < 1e-9);
    assert!(t.fraction(1.0) <= 1.0);
}

#[test]
fn bounce_edges_pair_up_inside_their_span() {
    let b = Bounce {
        seconds: 0.004,
        count: 6,
        seed: 3,
    };
    let mut edges = [0u32; 2 * MAX_BOUNCES];
    let n = b.edges(48_000.0, 0, &mut edges);
    assert!(n % 2 == 0 && n > 0 && n <= 12, "{n}");
    for pair in edges[..n].chunks(2) {
        assert!(pair[0] < pair[1] && pair[1] <= 192, "{pair:?}");
    }
    for w in edges[..n].windows(2) {
        assert!(w[0] < w[1]);
    }
    let mut again = [0u32; 2 * MAX_BOUNCES];
    assert_eq!(b.edges(48_000.0, 0, &mut again), n);
    assert_eq!(edges, again);
}
