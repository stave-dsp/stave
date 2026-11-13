use super::*;
use crate::primitives::spectral::{Complex, Fft};

const LEN: usize = 1 << 17;
const SPEAKERS: [Speaker; 3] = [Speaker::Man, Speaker::Woman, Speaker::Child];

/// The measured magnitude (linear) per bin of `stage`'s impulse
/// response at `rate`.
fn measured(stage: &mut impl Stage, rate: f32) -> Vec<f64> {
    stage.prepare(rate);
    let h: Vec<f64> = (0..LEN)
        .map(|i| f64::from(stage.tick(if i == 0 { 1.0 } else { 0.0 })))
        .collect();
    let fft = Fft::<f64>::new(LEN);
    let mut bins = vec![Complex::new(0.0, 0.0); LEN / 2 + 1];
    fft.forward_real(&h, &mut bins);
    bins.iter().map(|z| z.norm()).collect()
}

/// The bin (fractional, by a parabola through the log magnitudes) of the
/// largest value in `lo..hi`.
fn peak(m: &[f64], lo: usize, hi: usize) -> (f64, f64) {
    let k = (lo..hi).max_by(|&a, &b| m[a].total_cmp(&m[b])).unwrap();
    let (a, b, c) = (m[k - 1].ln(), m[k].ln(), m[k + 1].ln());
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c);
    (k as f64 + shift, m[k])
}

/// Where `m` falls through `level` walking from bin `from` by `dir`,
/// interpolated between bins.
fn crossing(m: &[f64], from: usize, dir: isize, level: f64) -> f64 {
    let mut k = from as isize;
    loop {
        let next = k + dir;
        if m[next as usize] < level {
            let (a, b) = (m[k as usize], m[next as usize]);
            return k as f64 + dir as f64 * (a - level) / (a - b);
        }
        k = next;
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn resonators_peak_at_their_formant() {
    let (mut centre_err, mut width_err) = (0.0f64, 0.0f64);
    for rate in [44_100.0f32, 48_000.0, 96_000.0] {
        let per_hz = LEN as f64 / f64::from(rate);
        for speaker in SPEAKERS {
            for vowel in Vowel::ALL {
                for f in vowel.formants(speaker) {
                    let mut r = Resonator::new(f);
                    let m = measured(&mut r, rate);
                    let c = f64::from(f.centre.get()) * per_hz;
                    let (at, top) = peak(&m, (0.5 * c) as usize, (1.5 * c) as usize);
                    let half = top / 2f64.sqrt();
                    let width = crossing(&m, at as usize, 1, half) - crossing(&m, at as usize, -1, half);
                    centre_err = centre_err.max((at / c - 1.0).abs());
                    let bw = f64::from(f.bandwidth.get()) * per_hz;
                    width_err = width_err.max((width / bw - 1.0).abs());
                }
            }
        }
    }
    eprintln!("resonator: centre {centre_err:.5}, width {width_err:.5}");
    assert!(centre_err < 0.01, "peak {:.3} % off its centre", 100.0 * centre_err);
    assert!(width_err < 0.02, "width {:.2} % off its bandwidth", 100.0 * width_err);
}

#[test]
#[cfg_attr(miri, ignore)]
fn vowels_peak_at_the_table() {
    let rate = 48_000.0;
    let per_hz = LEN as f64 / f64::from(rate);
    let (mut worst, mut shown, mut missing) = (0.0f64, 0, 0);
    let mut loudest = 0.0f64;
    for speaker in SPEAKERS {
        for vowel in Vowel::ALL {
            let formants = vowel.formants(speaker);
            let mut f = Formants::new(formants);
            let m = measured(&mut f, rate);
            let top = m[1..].iter().fold(0.0f64, |a, &b| a.max(b));
            loudest = loudest.max((20.0 * top.log10()).abs());
            for (i, formant) in formants.iter().enumerate() {
                // Search halfway to the neighbouring formants.
                let c = f64::from(formant.centre.get()) * per_hz;
                let below = if i == 0 { 0.5 * c } else { f64::from(formants[i - 1].centre.get()) * per_hz };
                let above = formants.get(i + 1).map_or(1.5 * c, |n| f64::from(n.centre.get()) * per_hz);
                let (lo, hi) = (((c + below) / 2.0) as usize, ((c + above) / 2.0) as usize);
                let (at, _) = peak(&m, lo, hi);
                // A formant shows as a peak when the maximum is inside
                // its search range, not on an edge.
                if at > lo as f64 + 1.0 && at < hi as f64 - 1.0 {
                    shown += 1;
                    let e = (at / c - 1.0).abs();
                    if e > 0.025 {
                        eprintln!("{speaker:?} {vowel:?} F{}: {:.2} %", i + 1, 100.0 * (at / c - 1.0));
                    }
                    worst = worst.max(e);
                } else {
                    missing += 1;
                    eprintln!("{speaker:?} {vowel:?} F{} merges with a neighbour", i + 1);
                }
            }
        }
    }
    eprintln!("cascade: worst {worst:.4} over {shown} peaks, {missing} merged, loudest {loudest:.3} dB");
    // The crowded F2/F3 of /i/ pull its F3 peak 4–6 % below the pole;
    // every other formant peaks within 2.5 % of its centre.
    assert!(worst < 0.06, "a vowel's peak {:.2} % off the table", 100.0 * worst);
    assert!(missing <= 2, "{missing} formants show no peak");
    assert!(loudest < 0.3, "the strongest formant at {loudest:.2} dB, not 0");
}

#[test]
fn the_table_is_peterson_and_barney() {
    // Slaney's Auditory Toolbox (1998), MakeVowel, quotes these.
    assert_eq!(Vowel::Aa.centres(Speaker::Man), [730.0, 1090.0, 2440.0]);
    assert_eq!(Vowel::Iy.centres(Speaker::Man), [270.0, 2290.0, 3010.0]);
    assert_eq!(Vowel::Uw.centres(Speaker::Man), [300.0, 870.0, 2240.0]);
    // Fant's bandwidth law at its anchors.
    let b = |f: f32| bandwidth(Hz::new(f)).get();
    assert!((b(500.0) - 40.0).abs() < 1e-4);
    assert!(b(270.0) > b(500.0) && b(3_000.0) > 200.0);
}

#[test]
fn morphing_ends_on_each_vowel() {
    let (a, i) = (Vowel::Aa.formants(Speaker::Woman), Vowel::Iy.formants(Speaker::Woman));
    let mut f = Formants::new(a);
    f.morph(a, i, 0.0);
    assert_eq!(f.formants(), a);
    f.morph(a, i, 1.0);
    for (got, want) in f.formants().iter().zip(i) {
        assert!((got.centre.get() / want.centre.get() - 1.0).abs() < 1e-6);
    }
}
