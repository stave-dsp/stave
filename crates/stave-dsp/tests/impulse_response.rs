//! [`ImpulseResponse`] loading (DSP.md F4): resampling against an
//! analytic band-limited response, the gain it keeps, normalisation,
//! minimum phase, the channel layouts, and the convolver resampling at
//! `prepare`.

use stave_dsp::primitives::spectral::{Fir, Head, IrError, IrLayout, Normalization, Passband};
use stave_dsp::units::SampleRate;
use stave_dsp::{Convolver, ImpulseResponse, Stage, Tail};

use core::f64::consts::TAU;
use stave_dsp::units::db;

/// A band-limited response: tones at 1, 7 and 15 kHz under a Gaussian
/// envelope (σ = 0.5 ms, so nothing above ~17 kHz), centred at 6 ms, in
/// seconds; plus a plain Gaussian pulse for DC.
fn analytic(t: f64) -> f64 {
    let envelope = (-0.5 * ((t - 0.006) / 0.0005).powi(2)).exp();
    envelope
        * (0.4
            + 0.8 * (TAU * 1_000.0 * t).cos()
            + 0.5 * (TAU * 7_000.0 * t + 0.3).sin()
            + 0.3 * (TAU * 15_000.0 * t + 1.1).cos())
}

/// 12 ms of the analytic response at `rate`, as taps (`h[n] = f(n/rate)`).
fn sampled(rate: f64) -> Vec<f32> {
    let n = (0.012 * rate).ceil() as usize;
    (0..n).map(|i| analytic(i as f64 / rate) as f32).collect()
}

fn gain_at(h: &[f32], frequency: f64, rate: f64) -> f64 {
    let (re, im) = h.iter().enumerate().fold((0.0, 0.0), |(re, im), (j, &v)| {
        let a = -TAU * frequency * j as f64 / rate;
        (re + f64::from(v) * a.cos(), im + f64::from(v) * a.sin())
    });
    f64::hypot(re, im)
}

/// Resampled between 44.1, 48, 96 and 192 kHz, the taps land on the
/// analytic response at the new rate, scaled by `from/to` (the gain a
/// tap carries), within 1e-4 of the peak; the gain at DC and at each
/// tone is kept within 1e-4.
#[test]
#[cfg_attr(miri, ignore)]
fn resampling_lands_on_the_analytic_response_and_keeps_the_gain() {
    let mut worst = 0.0f64;
    for (from, to) in [
        (44_100.0, 48_000.0),
        (48_000.0, 44_100.0),
        (48_000.0, 96_000.0),
        (96_000.0, 48_000.0),
        (44_100.0, 192_000.0),
        (192_000.0, 44_100.0),
    ] {
        let ir = ImpulseResponse::mono(&sampled(from)).recorded_at(SampleRate::new(from as f32));
        let out = ir.resampled(SampleRate::new(to as f32));
        assert_eq!(out.sample_rate(), Some(SampleRate::new(to as f32)));
        let h = out.path(0, 0).unwrap();
        let scale = from / to;
        let peak = (0..h.len())
            .map(|m| (scale * analytic(m as f64 / to)).abs())
            .fold(0.0, f64::max);
        for (m, &got) in h.iter().enumerate() {
            let want = scale * analytic(m as f64 / to);
            let err = (f64::from(got) - want).abs() / peak;
            assert!(err < 1e-4, "{from} → {to}: tap {m} is {got}, want {want} ({err:e})");
            worst = worst.max(err);
        }
        let source = ir.path(0, 0).unwrap();
        for f in [0.0, 1_000.0, 7_000.0, 15_000.0] {
            let (a, b) = (gain_at(source, f, from), gain_at(h, f, to));
            assert!(
                (a - b).abs() < 1e-4 * a.max(1e-3),
                "{from} → {to}: gain at {f} Hz {b}, was {a}"
            );
        }
    }
    println!("worst resampling error: {worst:.2e} of the peak");
}

#[test]
fn a_response_without_a_rate_is_marked_not_resampled() {
    let ir = ImpulseResponse::mono(&[1.0, 0.5]);
    assert_eq!(ir.sample_rate(), None);
    let out = ir.resampled(SampleRate::new(96_000.0));
    assert_eq!(out.path(0, 0), Some(&[1.0, 0.5][..]));
    assert_eq!(out.sample_rate(), Some(SampleRate::new(96_000.0)));
}

#[test]
fn normalisation_scales_every_path_by_one_gain() {
    let ir = ImpulseResponse::stereo(&[0.5, -0.25], &[0.1, 0.2, 0.2]);
    let peak = ir.normalized(Normalization::Peak);
    assert_eq!(peak.path(0, 0), Some(&[1.0, -0.5][..]));
    let r = peak.path(1, 1).unwrap();
    assert!((r[0] - 0.2).abs() < 1e-7 && (r[2] - 0.4).abs() < 1e-7);
    let energy = ir.normalized(Normalization::Energy);
    let l2 = |h: &[f32]| h.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt();
    let (left, right) = (energy.path(0, 0).unwrap(), energy.path(1, 1).unwrap());
    assert!((l2(left) - 1.0).abs() < 1e-6, "the louder path is at unity");
    assert!((l2(right) / l2(left) - 0.3 / 0.5590169943749474).abs() < 1e-6);
    let silence = ImpulseResponse::mono(&[0.0; 4]);
    assert_eq!(silence.normalized(Normalization::Energy), silence);
}

/// Minimum phase, per path: the magnitude within 0.01 dB wherever the
/// response is within 60 dB of its peak, the energy no later than the
/// original's at any tap, and the original's delay gone.
#[test]
#[cfg_attr(miri, ignore)]
fn minimum_phase_keeps_each_path_magnitude_and_moves_its_energy_first() {
    let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.2 }, db(70.0), 0.05);
    let delayed = |d: usize| -> Vec<f32> {
        let mut h = vec![0.0f32; d];
        h.extend(fir.taps().iter().map(|&v| v as f32));
        h.extend([0.3, -0.2, 0.1]);
        h
    };
    let ir = ImpulseResponse::stereo(&delayed(30), &delayed(5));
    let min = ir.minimum_phase();
    assert_eq!(min.layout(), IrLayout::Stereo);
    let mut worst = 0.0f64;
    for lane in 0..2 {
        let (h, m) = (ir.path(lane, lane).unwrap(), min.path(lane, lane).unwrap());
        assert_eq!(h.len(), m.len());
        let peak = (0..=512)
            .map(|i| gain_at(h, i as f64 / 1024.0, 1.0))
            .fold(0.0, f64::max);
        for i in 0..=512 {
            let f = i as f64 / 1024.0;
            let (a, b) = (gain_at(h, f, 1.0), gain_at(m, f, 1.0));
            if a > peak * 1e-3 {
                let db = 20.0 * (b / a).log10();
                assert!(db.abs() < 0.01, "lane {lane} at {f}: {db} dB");
                worst = worst.max(db.abs());
            }
        }
        let energy = |h: &[f32]| -> Vec<f64> {
            h.iter()
                .scan(0.0, |acc, &v| {
                    *acc += f64::from(v).powi(2);
                    Some(*acc)
                })
                .collect()
        };
        let (eh, em) = (energy(h), energy(m));
        let total = eh[eh.len() - 1];
        assert!(eh.iter().zip(&em).all(|(a, b)| *b >= a - 1e-5 * total));
        assert!(em[10] > 0.5 * total, "lane {lane}: the energy arrives at once");
    }
    println!("minimum phase: magnitude within {worst:.1e} dB");
}

#[test]
fn channels_map_onto_layouts() {
    let rate = SampleRate::new(48_000.0);
    let (a, b, c, d): (&[f32], &[f32], &[f32], &[f32]) = (&[1.0], &[2.0], &[3.0], &[4.0]);
    let mono = ImpulseResponse::from_channels(&[a], rate).unwrap();
    assert_eq!(mono.layout(), IrLayout::Mono);
    let ts = ImpulseResponse::from_channels(&[a, b, c, d], rate).unwrap();
    assert_eq!(ts.layout(), IrLayout::TrueStereo);
    assert_eq!(ts.path(0, 1), Some(b)); // left source, right microphone
    assert_eq!(ts.path(1, 0), Some(c));
    for n in [0, 3, 5] {
        let err = ImpulseResponse::from_channels(&vec![a; n], rate).unwrap_err();
        assert_eq!(err, IrError::Channels(n));
        assert!(err.to_string().contains("1, 2 or 4"));
    }
}

/// A response recorded at 96 kHz, prepared at 48 kHz: the convolver
/// re-plans from the resampled response (an impulse returns it, and the
/// tail is its length), and back at 96 kHz it returns the original.
#[test]
#[cfg_attr(miri, ignore)]
fn the_convolver_resamples_a_recorded_response_at_prepare() {
    let ir = ImpulseResponse::from_channels(&[&sampled(96_000.0)], SampleRate::new(96_000.0))
        .unwrap();
    let mut conv = Convolver::non_uniform(&ir, 16, 128, Head::Direct);
    for rate in [48_000.0f32, 96_000.0, 44_100.0, 48_000.0] {
        conv.prepare(rate);
        let want = ir.resampled(SampleRate::new(rate));
        let want = want.path(0, 0).unwrap();
        assert_eq!(conv.tail(), Tail::Samples(want.len() as u32 - 1), "at {rate}");
        for (i, &w) in want.iter().enumerate() {
            let y = conv.tick(if i == 0 { 1.0 } else { 0.0 });
            assert!((y - w).abs() < 1e-5, "at {rate}: sample {i} is {y}, want {w}");
        }
    }
}
