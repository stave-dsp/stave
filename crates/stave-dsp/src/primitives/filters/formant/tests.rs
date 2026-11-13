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
                    if e > 0.015 {
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
    // Every formant peaks within 2 % of its centre (1.26 % worst).
    assert!(worst < 0.02, "a vowel's peak {:.2} % off the table", 100.0 * worst);
    assert!(missing <= 2, "{missing} formants show no peak");
    assert!(loudest < 0.3, "the strongest formant at {loudest:.2} dB, not 0");
}

#[test]
fn the_table_is_peterson_and_barney() {
    // Peterson & Barney 1952, Table II (p. 183), F1/F2/F3 rows in the
    // table's vowel order i ɪ ɛ æ ɑ ɔ ʊ u ʌ ɝ, transcribed from the
    // printed page independently of `tables.rs`.
    let printed: [(Speaker, [[f32; 10]; 3]); 3] = [
        (
            Speaker::Man,
            [
                [270., 390., 530., 660., 730., 570., 440., 300., 640., 490.],
                [2290., 1990., 1840., 1720., 1090., 840., 1020., 870., 1190., 1350.],
                [3010., 2550., 2480., 2410., 2440., 2410., 2240., 2240., 2390., 1690.],
            ],
        ),
        (
            Speaker::Woman,
            [
                [310., 430., 610., 860., 850., 590., 470., 370., 760., 500.],
                [2790., 2480., 2330., 2050., 1220., 920., 1160., 950., 1400., 1640.],
                [3310., 3070., 2990., 2850., 2810., 2710., 2680., 2670., 2780., 1960.],
            ],
        ),
        (
            Speaker::Child,
            [
                [370., 530., 690., 1010., 1030., 680., 560., 430., 850., 560.],
                [3200., 2730., 2610., 2320., 1370., 1060., 1410., 1170., 1590., 1820.],
                [3730., 3600., 3570., 3320., 3170., 3180., 3310., 3260., 3360., 2160.],
            ],
        ),
    ];
    for (speaker, rows) in printed {
        for (v, vowel) in Vowel::ALL.iter().enumerate() {
            let want = [rows[0][v], rows[1][v], rows[2][v]];
            assert_eq!(vowel.centres(speaker), want, "{speaker:?} {vowel:?}");
        }
    }
    // Slaney's Auditory Toolbox (1998), MakeVowel, quotes the men's too.
    assert_eq!(Vowel::Aa.centres(Speaker::Man), [730.0, 1090.0, 2440.0]);
}

/// Fant 1972, Table I-C-II (men): measured F1–F3 and the bandwidths
/// his eqs. 56–58 predict for them, Hz.
const FANT_MEN: [(&str, [f32; 3], [f32; 3]); 16] = [
    ("u", [315., 605., 2450.], [57., 35., 39.]),
    ("o", [410., 690., 2600.], [43., 39., 54.]),
    ("ɔ", [505., 900., 2950.], [40., 43., 71.]),
    ("ɑ", [600., 1030., 2700.], [40., 52., 86.]),
    ("a", [710., 1150., 2700.], [42., 62., 104.]),
    ("æ", [700., 1430., 2450.], [42., 65., 106.]),
    ("ɛ", [450., 1825., 2425.], [42., 73., 96.]),
    ("e", [325., 1900., 2450.], [56., 51., 92.]),
    ("ɪ", [290., 2025., 2875.], [63., 41., 120.]),
    ("i", [230., 2000., 3000.], [86., 37., 129.]),
    ("y", [245., 1875., 2075.], [84., 86., 78.]),
    ("ʉ", [265., 1480., 2060.], [68., 46., 56.]),
    ("ø", [400., 1650., 2250.], [45., 52., 77.]),
    ("œ", [510., 1150., 2350.], [40., 47., 68.]),
    ("θ", [420., 1000., 2250.], [36., 42., 52.]),
    ("ɜ", [520., 1500., 2500.], [39., 51., 87.]),
];

/// The same, Table I-C-III (women).
const FANT_WOMEN: [(&str, [f32; 3], [f32; 3]); 13] = [
    ("u", [365., 690., 2700.], [56., 36., 75.]),
    ("o", [445., 780., 2850.], [48., 41., 64.]),
    ("ɔ", [610., 1100., 2800.], [43., 53., 87.]),
    ("ɑ", [700., 1100., 3000.], [45., 59., 111.]),
    ("a", [760., 1360., 3000.], [46., 64., 130.]),
    ("æ", [730., 1660., 2850.], [44., 66., 111.]),
    ("e", [395., 2200., 2850.], [52., 51., 128.]),
    ("i", [345., 2060., 3100.], [61., 42., 132.]),
    ("y", [295., 2000., 2650.], [73., 47., 98.]),
    ("ʉ", [305., 1770., 2380.], [70., 48., 77.]),
    ("ø", [410., 1740., 2350.], [51., 53., 84.]),
    ("œ", [525., 1490., 2600.], [44., 51., 87.]),
    ("θ", [490., 1240., 2650.], [45., 47., 74.]),
];

#[test]
fn bandwidths_are_fants() {
    // Printed predictions that no reading of eqs. 56–58 reproduces
    // (by row and formant index), most likely misprints in the tables.
    let misprints = [
        (Speaker::Man, "θ", 0),
        (Speaker::Man, "ɛ", 1),
        (Speaker::Man, "ɔ", 2),
        (Speaker::Woman, "u", 2),
        (Speaker::Woman, "æ", 2),
    ];
    let mut worst = [[0.0f32; 3]; 2];
    for (s, (speaker, table)) in [(Speaker::Man, &FANT_MEN[..]), (Speaker::Woman, &FANT_WOMEN[..])]
        .into_iter()
        .enumerate()
    {
        for &(vowel, f, printed) in table {
            let got = bandwidths(f.map(Hz::new), speaker).map(Hz::get);
            for k in 0..3 {
                if misprints.contains(&(speaker, vowel, k)) {
                    continue;
                }
                // In dB, Fant's own error measure: the men's within 1 dB
                // (his stated accuracy; 0.97 worst, B3 of /i/), the
                // women's within 0.35 dB (0.31 worst).
                let err = (20.0 * (got[k] / printed[k]).log10()).abs();
                let budget = if speaker == Speaker::Man { 1.0 } else { 0.35 };
                assert!(err <= budget, "{speaker:?} {vowel} B{}: {} vs printed {}", k + 1, got[k], printed[k]);
                worst[s][k] = worst[s][k].max(err);
            }
        }
    }
    eprintln!("Fant, worst dB per formant: men {:?}, women {:?}", worst[0], worst[1]);
}

#[test]
fn bandwidths_stay_positive_and_finite() {
    for speaker in SPEAKERS {
        for vowel in Vowel::ALL {
            for f in vowel.formants(speaker) {
                let b = f.bandwidth.get();
                assert!(b.is_finite() && b > 0.0 && b < 400.0, "{speaker:?} {vowel:?}: {b} Hz");
            }
        }
    }
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
