//! [`Fir`] designs against their specifications (DSP.md F6): passband
//! ripple and stopband attenuation measured on a dense grid by the
//! direct DTFT in `f64`, least squares against its optimality, minimum
//! phase against the magnitude it must keep, and a design run through
//! the [`Convolver`].

use stave_dsp::primitives::spectral::{Band, Fir, Head, Passband, Window};
use stave_dsp::{Convolver, Stage};

/// `(passband ripple: half the peak-to-peak of |H| as a linear
/// fraction, worst stopband gain in dB)` over `passbands` and
/// `stopbands`, 2000 points each.
fn measure(fir: &Fir, passbands: &[(f64, f64)], stopbands: &[(f64, f64)]) -> (f64, f64) {
    let grid = |(a, b): (f64, f64)| (0..=2000).map(move |i| a + (b - a) * i as f64 / 2000.0);
    let (lo, hi) = passbands
        .iter()
        .flat_map(|&band| grid(band))
        .map(|f| fir.response(f).norm())
        .fold((f64::INFINITY, 0.0f64), |(lo, hi), a| (lo.min(a), hi.max(a)));
    let ripple = if passbands.is_empty() {
        0.0
    } else {
        (hi - lo) / 2.0
    };
    let stop = stopbands
        .iter()
        .flat_map(|&band| grid(band))
        .map(|f| fir.gain_db(f))
        .fold(f64::NEG_INFINITY, f64::max);
    (ripple, stop)
}

/// Kaiser designs meet the attenuation asked for (within 1 dB of
/// Kaiser's estimate) in the stopband and the matching ripple
/// `±10^(−(A − 1)/20)` in the passband (half its peak-to-peak), for
/// lowpass, highpass and bandpass at 40 to 100 dB.
#[test]
#[cfg_attr(miri, ignore)]
fn kaiser_designs_meet_ripple_and_attenuation() {
    let mut report = Vec::new();
    for attenuation in [40.0, 60.0, 80.0, 100.0] {
        let delta = 10f64.powf(-(attenuation - 1.0) / 20.0);
        for (passband, pass, stop) in [
            (
                Passband::Lowpass { cutoff: 0.1 },
                vec![(0.0, 0.1)],
                vec![(0.14, 0.5)],
            ),
            (
                Passband::Lowpass { cutoff: 0.3 },
                vec![(0.0, 0.3)],
                vec![(0.34, 0.5)],
            ),
            (
                Passband::Highpass { cutoff: 0.25 },
                vec![(0.25, 0.5)],
                vec![(0.0, 0.21)],
            ),
            (
                Passband::Bandpass {
                    low: 0.1,
                    high: 0.2,
                },
                vec![(0.1, 0.2)],
                vec![(0.0, 0.06), (0.24, 0.5)],
            ),
        ] {
            let fir = Fir::kaiser(passband, attenuation, 0.04);
            let (ripple, worst) = measure(&fir, &pass, &stop);
            assert!(
                worst <= -(attenuation - 1.0),
                "{passband:?} at {attenuation} dB: stopband reaches {worst:.2} dB"
            );
            assert!(
                ripple <= delta,
                "{passband:?} at {attenuation} dB: ripple {ripple:e} > {delta:e}"
            );
            report.push(format!(
                "{attenuation} dB {passband:?}: {} taps, stopband {worst:.1} dB, ripple {:.2e}",
                fir.len(),
                ripple
            ));
        }
    }
    println!("{}", report.join("\n"));
}

#[test]
fn windowed_designs_are_linear_phase_with_unity_reference_gain() {
    for window in [
        Window::Rectangular,
        Window::Hann,
        Window::Hamming,
        Window::Blackman,
        Window::Kaiser { beta: 6.0 },
    ] {
        let lp = Fir::windowed_sinc(Passband::Lowpass { cutoff: 0.2 }, 51, window);
        let hp = Fir::windowed_sinc(Passband::Highpass { cutoff: 0.2 }, 51, window);
        let bp = Fir::windowed_sinc(
            Passband::Bandpass {
                low: 0.1,
                high: 0.3,
            },
            50,
            window,
        );
        for fir in [&lp, &hp, &bp] {
            let h = fir.taps();
            assert!((0..h.len()).all(|n| h[n] == h[h.len() - 1 - n]));
        }
        assert!(lp.gain_db(0.0).abs() < 1e-9);
        assert!(hp.gain_db(0.5).abs() < 1e-9);
        assert!(bp.gain_db(0.2).abs() < 1e-9);
        // The edge sits near −6 dB (half amplitude) for every window.
        assert!((lp.response(0.2).norm() - 0.5).abs() < 0.05, "{window:?}");
    }
}

/// Least squares minimises the weighted squared error, so it beats any
/// other type-I design of its length on that measure — here the Kaiser
/// design with the same edges — and more stopband weight buys more
/// attenuation.
#[test]
#[cfg_attr(miri, ignore)]
fn least_squares_is_optimal_for_its_error_and_follows_its_weights() {
    let bands = |stop_weight: f64| {
        [
            Band {
                start: 0.0,
                end: 0.1,
                gain: 1.0,
                weight: 1.0,
            },
            Band {
                start: 0.15,
                end: 0.5,
                gain: 0.0,
                weight: stop_weight,
            },
        ]
    };
    let error = |fir: &Fir, bands: &[Band]| -> f64 {
        bands
            .iter()
            .map(|b| {
                let n = 4000;
                let df = (b.end - b.start) / n as f64;
                (0..n)
                    .map(|i| {
                        let f = b.start + (i as f64 + 0.5) * df;
                        let a = fir.response(f).norm();
                        b.weight * (a - b.gain).powi(2) * df
                    })
                    .sum::<f64>()
            })
            .sum()
    };
    let ls = Fir::least_squares(61, &bands(1.0));
    let windowed = Fir::windowed_sinc(
        Passband::Lowpass { cutoff: 0.125 },
        61,
        Window::kaiser_for(50.0),
    );
    let (e_ls, e_win) = (error(&ls, &bands(1.0)), error(&windowed, &bands(1.0)));
    assert!(e_ls < e_win, "least squares {e_ls:e} vs windowed {e_win:e}");
    let (ripple, stop) = measure(&ls, &[(0.0, 0.1)], &[(0.15, 0.5)]);
    let heavy = Fir::least_squares(61, &bands(100.0));
    let (_, stop_heavy) = measure(&heavy, &[(0.0, 0.1)], &[(0.15, 0.5)]);
    assert!(ripple < 0.05 && stop < -30.0, "ripple {ripple}, stop {stop}");
    assert!(stop_heavy < stop - 10.0, "{stop_heavy} vs {stop}");
    let h = ls.taps();
    assert!((0..61).all(|n| h[n] == h[60 - n]));
    println!(
        "least squares 61 taps: error {e_ls:.2e} (Kaiser {e_win:.2e}); stopband {stop:.1} dB, \
         {stop_heavy:.1} dB at weight 100; ripple {ripple:.2e}"
    );
}

/// The minimum-phase version of a linear-phase lowpass keeps its
/// magnitude (passband within 0.01 dB, stopband within 3 dB of the
/// design's attenuation), and is causal and front-loaded: its partial
/// energy is never behind the linear-phase design's, and its peak comes
/// long before the linear-phase delay.
#[test]
#[cfg_attr(miri, ignore)]
fn minimum_phase_keeps_the_magnitude_and_moves_energy_first() {
    let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.1 }, 80.0, 0.04);
    let min = fir.minimum_phase();
    assert_eq!(min.len(), fir.len());
    let mut worst_pass = 0.0f64;
    for i in 0..=1000 {
        let f = 0.1 * i as f64 / 1000.0;
        worst_pass = worst_pass.max((min.gain_db(f) - fir.gain_db(f)).abs());
    }
    let (_, stop) = measure(&min, &[], &[(0.14, 0.5)]);
    assert!(worst_pass < 0.01, "passband moved {worst_pass} dB");
    assert!(stop < -77.0, "stopband {stop} dB");
    let energy = |h: &[f64]| -> Vec<f64> {
        h.iter()
            .scan(0.0, |acc, v| {
                *acc += v * v;
                Some(*acc)
            })
            .collect()
    };
    let (e_lin, e_min) = (energy(fir.taps()), energy(min.taps()));
    let total = e_lin[e_lin.len() - 1];
    for (k, (a, b)) in e_lin.iter().zip(&e_min).enumerate() {
        assert!(*b >= a - 1e-6 * total, "tap {k}: {b} < {a}");
    }
    assert!((e_min[e_min.len() - 1] - total).abs() < 1e-6 * total);
    let peak = |h: &[f64]| (0..h.len()).max_by(|&a, &b| h[a].abs().total_cmp(&h[b].abs()));
    let (p_lin, p_min) = (peak(fir.taps()).unwrap(), peak(min.taps()).unwrap());
    assert!(p_min * 4 < p_lin, "peak at {p_min}, linear phase at {p_lin}");
    println!(
        "minimum phase {} taps: passband within {worst_pass:.1e} dB, stopband {stop:.1} dB, \
         peak at tap {p_min} (linear phase {p_lin})",
        fir.len()
    );
}

/// A design run through the convolver: a passband tone passes at unity,
/// a stopband tone comes out at least the design's attenuation down
/// (less the f32 floor).
#[test]
#[cfg_attr(miri, ignore)]
fn a_design_runs_through_the_convolver() {
    let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.1 }, 70.0, 0.05);
    let ir = (&fir).into();
    let mut conv = Convolver::uniform(&ir, 64, Head::Direct);
    conv.prepare(48_000.0);
    let level = |conv: &mut Convolver, f: f64| -> f64 {
        conv.reset();
        let n = 4096;
        let y: Vec<f32> = (0..n)
            .map(|t| conv.tick((core::f64::consts::TAU * f * t as f64).sin() as f32))
            .collect();
        let settled = &y[fir.len()..];
        let rms = (settled.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>()
            / settled.len() as f64)
            .sqrt();
        20.0 * (rms * 2f64.sqrt()).log10()
    };
    let pass = level(&mut conv, 0.05);
    let stop = level(&mut conv, 0.3);
    assert!(pass.abs() < 0.01, "passband {pass} dB");
    assert!(stop < -69.0, "stopband {stop} dB");
}
