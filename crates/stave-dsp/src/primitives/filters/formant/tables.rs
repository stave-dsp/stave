//! The vowel table: Peterson & Barney's formant averages, and a
//! bandwidth for each formant.

use super::Formant;
use crate::units::Hz;

/// Who speaks the vowel: Peterson & Barney averaged 33 men, 28 women
/// and 15 children.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Speaker {
    Man,
    Woman,
    Child,
}

/// The ten American English vowels of Peterson & Barney 1952, named by
/// their ARPAbet symbols; the doc gives the word they were spoken in
/// (/h_d/) and the IPA.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Vowel {
    /// "heed", /i/.
    Iy,
    /// "hid", /ɪ/.
    Ih,
    /// "head", /ɛ/.
    Eh,
    /// "had", /æ/.
    Ae,
    /// "hod", /ɑ/.
    Aa,
    /// "hawed", /ɔ/.
    Ao,
    /// "hood", /ʊ/.
    Uh,
    /// "who'd", /u/.
    Uw,
    /// "hud", /ʌ/.
    Ah,
    /// "heard", /ɝ/.
    Er,
}

impl Vowel {
    /// Every vowel, in the table's order.
    pub const ALL: [Vowel; 10] = [
        Vowel::Iy,
        Vowel::Ih,
        Vowel::Eh,
        Vowel::Ae,
        Vowel::Aa,
        Vowel::Ao,
        Vowel::Uh,
        Vowel::Uw,
        Vowel::Ah,
        Vowel::Er,
    ];

    /// F1–F3 in Hz as Peterson & Barney measured them (averages).
    pub const fn centres(self, speaker: Speaker) -> [f32; 3] {
        let row = match self {
            Vowel::Iy => 0,
            Vowel::Ih => 1,
            Vowel::Eh => 2,
            Vowel::Ae => 3,
            Vowel::Aa => 4,
            Vowel::Ao => 5,
            Vowel::Uh => 6,
            Vowel::Uw => 7,
            Vowel::Ah => 8,
            Vowel::Er => 9,
        };
        match speaker {
            Speaker::Man => MEN[row],
            Speaker::Woman => WOMEN[row],
            Speaker::Child => CHILDREN[row],
        }
    }

    /// The vowel's three formants: Peterson & Barney's centres with
    /// Fant's bandwidths for them ([`bandwidths`]).
    pub fn formants(self, speaker: Speaker) -> [Formant; 3] {
        let centres = self.centres(speaker).map(Hz::new);
        let widths = bandwidths(centres, speaker);
        [0, 1, 2].map(|i| Formant {
            centre: centres[i],
            bandwidth: widths[i],
        })
    }
}

/// Peterson, G. E. & Barney, H. L. (1952), "Control methods used in a
/// study of the vowels", JASA 24(2):175–184, Table II ("Averages of
/// fundamental and formant frequencies and formant amplitudes of vowels
/// by 76 speakers", p. 183): average formant frequencies (Hz), men. All
/// 90 values of the three tables were checked against the printed
/// Table II.
const MEN: [[f32; 3]; 10] = [
    [270.0, 2290.0, 3010.0],
    [390.0, 1990.0, 2550.0],
    [530.0, 1840.0, 2480.0],
    [660.0, 1720.0, 2410.0],
    [730.0, 1090.0, 2440.0],
    [570.0, 840.0, 2410.0],
    [440.0, 1020.0, 2240.0],
    [300.0, 870.0, 2240.0],
    [640.0, 1190.0, 2390.0],
    [490.0, 1350.0, 1690.0],
];

/// The same table, women.
const WOMEN: [[f32; 3]; 10] = [
    [310.0, 2790.0, 3310.0],
    [430.0, 2480.0, 3070.0],
    [610.0, 2330.0, 2990.0],
    [860.0, 2050.0, 2850.0],
    [850.0, 1220.0, 2810.0],
    [590.0, 920.0, 2710.0],
    [470.0, 1160.0, 2680.0],
    [370.0, 950.0, 2670.0],
    [760.0, 1400.0, 2780.0],
    [500.0, 1640.0, 1960.0],
];

/// The same table, children.
const CHILDREN: [[f32; 3]; 10] = [
    [370.0, 3200.0, 3730.0],
    [530.0, 2730.0, 3600.0],
    [690.0, 2610.0, 3570.0],
    [1010.0, 2320.0, 3320.0],
    [1030.0, 1370.0, 3170.0],
    [680.0, 1060.0, 3180.0],
    [560.0, 1410.0, 3310.0],
    [430.0, 1170.0, 3260.0],
    [850.0, 1590.0, 3360.0],
    [560.0, 1820.0, 2160.0],
];

/// Fant's average fourth formant `F4a` (Hz), which his B3 formula
/// measures F3's proximity to: 3400 for men, 3700 for women (Fant 1972,
/// p. 47). Fant measured no children; theirs is *our extrapolation*:
/// the women's value scaled by the ratio of Peterson & Barney's mean
/// child F3 to mean woman F3 (3266/2783), 4342 Hz.
const F4A: [f64; 3] = [3400.0, 3700.0, 3700.0 * 3266.0 / 2783.0];

/// The bandwidths (Hz) of formants `[F1, F2, F3]` spoken by `speaker`,
/// Peterson & Barney giving none: Fant's empirical formulas from the
/// formant pattern (G. Fant 1972, "Vocal tract wall effects, losses,
/// and resonance bandwidths", STL-QPSR 13(2-3):28–52, eqs. 56–58, p. 47),
/// with `x = F/500`:
///
/// - `B1 = 15·w/x1² + 20·x1^½ + 5·x1²` — cavity wall, surface and
///   radiation losses; `w` = 1 for men, 1.3 for women (Fant's female
///   rule, p. 47).
/// - `B2 = 22 + 16·x1² + 12000/(F3 − F2)`.
/// - `B3 = 25·x1² + 4·x2² + 10·F3/(F4a − F3)`, with `F4a` 3400 Hz for
///   men, 3700 for women. The paper prints the last term as
///   `10 F3(F4a−F3)`, which is not dimensionally a bandwidth; the
///   quotient is our reading, chosen because it reproduces Fant's own
///   predicted column (Tables I-C-II and I-C-III) — see Verified.
///
/// Fant fitted Swedish vowels from two speakers per sex; applying the
/// fit to Peterson & Barney's American English averages is our choice.
/// Children are not in Fant's data: they get the women's formulas with
/// an extrapolated `F4a` (4342 Hz) — a choice, not a measurement.
///
/// # Verified
///
/// `crates/stave-dsp/src/primitives/filters/formant/tests.rs::bandwidths_are_fants`
/// recomputes Fant's predicted bandwidths for the 16 male and 13 female
/// vowels of his Tables I-C-II and I-C-III, in dB as Fant scores them:
/// every men's value within 1 dB (0.97 worst, B3 of /i/) and every
/// women's within 0.35 dB (0.31 worst), except five printed
/// predictions no reading of the formulas reproduces, taken to be
/// table misprints: men's θ B1 (+1.6 dB), ɛ B2 (−2.5), ɔ B3 (+3.3);
/// women's u B3 (−3.9), æ B3 (+1.4).
///
/// # Valid ranges
///
/// `0 < F1`, `F2 < F3 < F4a`: the formulas divide by the gaps, which
/// are floored at 1 Hz so a degenerate pattern gives a very wide
/// formant rather than an infinite or negative one.
pub fn bandwidths(formants: [Hz; 3], speaker: Speaker) -> [Hz; 3] {
    let [f1, f2, f3] = formants.map(|f| f64::from(f.get()).max(1.0));
    let (wall, f4a) = match speaker {
        Speaker::Man => (1.0, F4A[0]),
        Speaker::Woman => (1.3, F4A[1]),
        Speaker::Child => (1.3, F4A[2]),
    };
    let (x1, x2) = (f1 / 500.0, f2 / 500.0);
    let b1 = 15.0 * wall / (x1 * x1) + 20.0 * x1.sqrt() + 5.0 * x1 * x1;
    let b2 = 22.0 + 16.0 * x1 * x1 + 12_000.0 / (f3 - f2).max(1.0);
    let b3 = 25.0 * x1 * x1 + 4.0 * x2 * x2 + 10.0 * f3 / (f4a - f3).max(1.0);
    [b1, b2, b3].map(|b| Hz::new(b as f32))
}
