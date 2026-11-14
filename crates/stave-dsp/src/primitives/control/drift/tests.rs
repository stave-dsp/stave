use super::*;
use crate::units::Seconds;

#[test]
fn ornstein_uhlenbeck_has_its_variance_and_correlation() {
    // τ = 50 samples at either rate, 2²¹ samples: ~20 000 correlation
    // times, so the variance's standard error is about 1 %.
    for (rate, seed) in [(1_000.0f32, 3u64), (48_000.0, 4)] {
        let tau_s = 50.0 / f64::from(rate);
        let mut p = OrnsteinUhlenbeck::new(2.0, 0.5, Seconds::new(tau_s), seed);
        p.prepare(rate);
        let n = 1 << 21;
        let x: Vec<f64> = (0..n).map(|_| p.tick_f64()).collect();
        let mean = x.iter().sum::<f64>() / n as f64;
        let var = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        assert!((mean - 2.0).abs() < 0.02, "{rate}: mean {mean}");
        assert!((var / 0.25 - 1.0).abs() < 0.04, "{rate}: variance {var}");
        for lag in [25usize, 50, 100] {
            let c = x[..n - lag]
                .iter()
                .zip(&x[lag..])
                .map(|(a, b)| (a - mean) * (b - mean))
                .sum::<f64>()
                / (n - lag) as f64
                / var;
            let expect = (-(lag as f64) / 50.0).exp();
            assert!((c - expect).abs() < 0.03, "{rate}: lag {lag}: {c} against {expect}");
        }
    }
}

#[test]
fn wiener_spreads_as_the_root_of_time() {
    let rate = 100.0f32;
    let (paths, drift, diffusion) = (4000, 0.3, 0.2);
    for seconds in [1usize, 4] {
        let ends: Vec<f64> = (0..paths)
            .map(|seed| {
                let mut w = Wiener::new(1.0, drift, diffusion, seed as u64 + 1);
                w.prepare(rate);
                (0..seconds * 100).for_each(|_| {
                    w.tick_f64();
                });
                w.tick_f64()
            })
            .collect();
        let t = seconds as f64;
        let mean = ends.iter().sum::<f64>() / paths as f64;
        let var = ends.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / paths as f64;
        let expect_var = diffusion * diffusion * t;
        let standard_error = (expect_var / paths as f64).sqrt();
        assert!((mean - (1.0 + drift * t)).abs() < 3.0 * standard_error, "{t} s: mean {mean}");
        assert!((var / expect_var - 1.0).abs() < 0.06, "{t} s: variance {var} against {expect_var}");
    }
    // Seeded: one path replays after a reset.
    let mut w = Wiener::new(0.0, 0.1, 0.1, 9);
    let a: Vec<f64> = (0..100).map(|_| w.tick_f64()).collect();
    w.reset();
    let b: Vec<f64> = (0..100).map(|_| w.tick_f64()).collect();
    assert_eq!(a, b);
}
