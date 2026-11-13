//! The [`Stft`] stage (DSP.md F5): perfect reconstruction for every
//! window and hop [`Window::overlap_add_gain`] claims COLA, the claims
//! against the theory, and the closure acting on the output.

use stave_dsp::primitives::spectral::{Complex, Stft, StftError, Window};
use stave_dsp::{Stage, Tail, Xorshift32};

const WINDOWS: [Window; 6] = [
    Window::Rectangular,
    Window::Hann,
    Window::Hamming,
    Window::Blackman,
    Window::SqrtHann,
    Window::Kaiser { beta: 8.0 },
];

/// `y[t]` against `x[t − latency]`; the worst error.
fn reconstruction_error(x: &[f32], y: &[f32], latency: usize) -> f64 {
    (latency..x.len())
        .map(|t| (f64::from(y[t]) - f64::from(x[t - latency])).abs())
        .fold(0.0, f64::max)
}

/// Every (window, size, hop) the COLA check accepts reconstructs the
/// input exactly (to f32 rounding), delayed by the size. The rounding
/// grows with the frames overlapping each sample (`size/hop` f32
/// additions into the accumulator), so the budget is `1e-6 +
/// 4e-8·size/hop` of full scale.
#[test]
#[cfg_attr(miri, ignore)]
fn every_cola_pair_reconstructs_perfectly() {
    let mut rng = Xorshift32::new(0x57F7);
    let (mut claimed, mut worst, mut quarter) = (0, 0.0f64, 0.0f64);
    for size in [16usize, 64, 256, 1024] {
        let x: Vec<f32> = (0..4 * size + 300).map(|_| rng.next_bipolar()).collect();
        for window in WINDOWS {
            for hop in 1..=size {
                // Every hop for the small sizes; the dividing hops above.
                if size > 64 && size % hop != 0 {
                    continue;
                }
                let Some(_) = window.overlap_add_gain(size, hop) else {
                    assert!(matches!(
                        Stft::new(size, hop, window, |_: &mut [Complex<f32>]| {}),
                        Err(StftError::NotCola { .. })
                    ));
                    continue;
                };
                let mut stft = Stft::new(size, hop, window, |_: &mut [Complex<f32>]| {}).unwrap();
                stft.prepare(48_000.0);
                let y: Vec<f32> = x.iter().map(|&s| stft.tick(s)).collect();
                let err = reconstruction_error(&x, &y, size);
                let budget = 1e-6 + 4e-8 * (size / hop) as f64;
                assert!(err < budget, "{window:?} size {size} hop {hop}: error {err:e}");
                worst = worst.max(err / budget);
                if size / hop == 4 {
                    quarter = quarter.max(err);
                }
                claimed += 1;
            }
        }
    }
    assert!(claimed > 100, "{claimed} pairs claimed COLA");
    println!(
        "{claimed} COLA pairs: worst error {worst:.2} of the budget; {quarter:.2e} of full scale at 75 % overlap"
    );
}

/// The theory: a squared window whose cosine terms reach `m` (Hann² and
/// Hamming² 2, Blackman² 4, √Hann² = Hann 1, rectangular 0) overlap-adds
/// to a constant at `size/R` for every `R > m`; the cosine windows fail
/// at `R ≤ m`; Kaiser is COLA only at hop 1.
#[test]
fn the_cola_claims_match_the_theory() {
    let size = 512;
    for (window, m) in [
        (Window::Rectangular, 0),
        (Window::SqrtHann, 1),
        (Window::Hann, 2),
        (Window::Hamming, 2),
        (Window::Blackman, 4),
    ] {
        for r in [1usize, 2, 4, 8, 16] {
            let cola = window.overlap_add_gain(size, size / r).is_some();
            assert_eq!(cola, r > m, "{window:?} at size/{r}");
        }
    }
    assert!(Window::Kaiser { beta: 8.0 }.overlap_add_gain(size, 1).is_some());
    assert!(Window::Kaiser { beta: 8.0 }.overlap_add_gain(size, 2).is_none());
    // Constants: Σ w² over one period, divided by the hop's overlap.
    let gain = Window::Hann.overlap_add_gain(size, size / 4).unwrap();
    assert!((gain - 1.5).abs() < 1e-12);
}

#[test]
fn bad_sizes_and_hops_are_refused() {
    let id = |_: &mut [Complex<f32>]| {};
    assert_eq!(Stft::new(48, 12, Window::Hann, id).unwrap_err(), StftError::Size(48));
    assert_eq!(Stft::new(1, 1, Window::Hann, id).unwrap_err(), StftError::Size(1));
    assert_eq!(
        Stft::new(64, 0, Window::Hann, id).unwrap_err(),
        StftError::Hop { size: 64, hop: 0 }
    );
    assert_eq!(
        Stft::new(64, 65, Window::Hann, id).unwrap_err(),
        StftError::Hop { size: 64, hop: 65 }
    );
    let err = Stft::new(64, 32, Window::Hann, id).unwrap_err();
    assert!(err.to_string().contains("overlap-add"), "{err}");
}

/// A mask that zeroes the bins above 1/8 of the rate passes a tone
/// below it and removes one above it (by more than 60 dB once settled).
#[test]
#[cfg_attr(miri, ignore)]
fn a_bin_mask_filters_the_output() {
    let size = 512;
    let mut stft = Stft::new(size, size / 4, Window::Hann, move |bins: &mut [Complex<f32>]| {
        for z in &mut bins[size / 16..] {
            *z = Complex::default();
        }
    })
    .unwrap();
    let level = |stft: &mut Stft<_>, cycles_per_sample: f64| {
        stft.reset();
        let n = 8 * size;
        let y: Vec<f32> = (0..n)
            .map(|t| stft.tick((core::f64::consts::TAU * cycles_per_sample * t as f64).sin() as f32))
            .collect();
        let tail = &y[3 * size..];
        (tail.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / tail.len() as f64 * 2.0).sqrt()
    };
    let pass = level(&mut stft, 0.02);
    let stop = level(&mut stft, 0.3);
    assert!((pass - 1.0).abs() < 0.01, "passband level {pass}");
    assert!(stop < 1e-3, "stopband level {stop}");
}

#[test]
fn latency_tail_and_reset_are_reported() {
    let mut stft = Stft::new(256, 64, Window::SqrtHann, |bins: &mut [Complex<f32>]| {
        bins[3] = bins[3].scale(2.0);
    })
    .unwrap();
    assert_eq!(stft.latency(), 256);
    assert_eq!(stft.tail(), Tail::Samples(511));
    assert_eq!((stft.size(), stft.hop(), stft.window()), (256, 64, Window::SqrtHann));
    let x: Vec<f32> = (0..900).map(|n| ((n * 13) % 29) as f32 / 14.0 - 1.0).collect();
    let first: Vec<f32> = x.iter().map(|&s| stft.tick(s)).collect();
    stft.reset();
    let again: Vec<f32> = x.iter().map(|&s| stft.tick(s)).collect();
    assert_eq!(first, again);
    assert!(format!("{stft:?}").contains("hop: 64"));
}
