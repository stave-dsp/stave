//! Offline time stretching: the phase vocoder over a whole signal.

use super::vocoder::{Reconstruction, Vocoder};

/// A time-stretched signal and the stretch it got.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretched {
    /// The stretched signal, `ratio` times the input's length (rounded).
    pub samples: Vec<f32>,
    /// The stretch applied: the synthesis hop over the analysis hop,
    /// the asked ratio rounded to whole analysis hops.
    pub ratio: f64,
}

/// The analysis hop and the stretch it gives, for `ratio` at frame
/// `size`: the synthesis hop is a quarter frame, the analysis hop the
/// nearest whole number to it over the ratio.
pub(super) fn hops(ratio: f64, size: usize) -> (usize, usize, f64) {
    let synthesis = size / 4;
    let analysis = ((synthesis as f64 / ratio).round() as usize).max(1);
    (analysis, synthesis, synthesis as f64 / analysis as f64)
}

/// Stretches `input` in time by `ratio` (2 is twice as long) without
/// changing its pitch, with a phase vocoder of `size`-sample Hann frames
/// (a power of two; 2048 suits 44.1 and 48 kHz) and the phase built by
/// `reconstruction`.
///
/// This is the vocoder as Průša & Holighaus evaluate it (*Phase vocoder
/// done right*, EUSIPCO 2017): synthesis hop `size/4`, analysis hop
/// `size/(4·ratio)` rounded, the centred phase derivatives with one
/// frame of look-ahead, weighted overlap-add normalised by the summed
/// squared window. Input frame `n` is centred at `n·a_a`, output frame
/// `n` at `n·a_s`, so the output starts with the input's start: no
/// latency to trim.
///
/// Its quality is measured by spectral convergence, the distance
/// between the output's STFT magnitude on the synthesis grid and the
/// input's on the analysis grid (Průša, Balazs & Søndergaard 2017):
/// `crates/stave-dsp/src/effects/pitch/tests.rs::stretch_spectral_convergence`,
/// at 22.05 kHz with 1024-sample frames, Průša against identity
/// locking at ×0.75 / ×1.5 / ×2 — PVDR's figure-1 mix (sinusoid,
/// exponential chirp, impulse): −19.1/−15.7, −17.8/−16.2, −14.2/−14.2 dB;
/// a click train: −12.8/−7.2, −10.0/−6.8, −6.2/−5.9 dB; a harmonic tone
/// with vibrato, identity locking's best case: −29.7/−29.2,
/// −25.5/−34.0, −22.5/−34.9 dB (PVDR's own caveat on voiced sound).
/// Offline only: it allocates the output.
///
/// ```
/// use stave_dsp::effects::pitch::{time_stretch, Reconstruction};
///
/// let x: Vec<f32> = (0..8_000).map(|n| (n as f32 * 0.05).sin()).collect();
/// let y = time_stretch(&x, 1.5, Reconstruction::Prusa, 1_024);
/// assert_eq!(y.samples.len(), (8_000.0 * y.ratio).round() as usize);
/// assert!((y.ratio - 1.5).abs() < 0.01);
/// ```
///
/// # Panics
///
/// If `size` is not a power of two of at least 16, or `ratio` is not
/// positive and finite.
pub fn time_stretch(input: &[f32], ratio: f64, reconstruction: Reconstruction, size: usize) -> Stretched {
    assert!(size.is_power_of_two() && size >= 16, "time_stretch: size {size} is not a power of two ≥ 16");
    assert!(ratio > 0.0 && ratio.is_finite(), "time_stretch: ratio {ratio} is not positive");
    let (aa, as_, ratio) = hops(ratio, size);
    let out_len = (input.len() as f64 * ratio).round() as usize;
    let half = (size / 2) as i64;
    let first = -(half / as_ as i64) - 1;
    let last = ((out_len as i64 + half) / as_ as i64) + 1;
    let pad = size as i64;
    let mut acc = vec![0.0f32; out_len + 2 * size + as_];
    let mut norm = vec![0.0f32; acc.len()];
    let mut vocoder = Vocoder::new(size, reconstruction);
    let mut add = |vocoder: &Vocoder, frame: i64| {
        let start = frame * as_ as i64 - half + pad;
        for (k, (&y, &w)) in vocoder.output().iter().zip(vocoder.window()).enumerate() {
            let i = start + k as i64;
            if (0..acc.len() as i64).contains(&i) {
                acc[i as usize] += y;
                norm[i as usize] += w * w;
            }
        }
    };
    for n in first..=last {
        let begin = n * aa as i64 - half;
        vocoder.analyse(
            |frame| {
                for (k, f) in frame.iter_mut().enumerate() {
                    let i = begin + k as i64;
                    *f = if (0..input.len() as i64).contains(&i) { input[i as usize] } else { 0.0 };
                }
            },
            aa,
        );
        if vocoder.waiting() >= 2 {
            vocoder.synthesise(as_, size / 2 + 1, false);
            add(&vocoder, n - 1);
        }
    }
    vocoder.synthesise(as_, size / 2 + 1, true);
    add(&vocoder, last);
    let floor = 0.05;
    let samples = (0..out_len)
        .map(|s| {
            let i = s + pad as usize;
            acc[i] / norm[i].max(floor)
        })
        .collect();
    Stretched { samples, ratio }
}
