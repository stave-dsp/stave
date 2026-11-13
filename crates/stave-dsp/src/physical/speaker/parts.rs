//! Real drivers' parameters: Jensen's published guitar speakers.

use super::ThieleSmall;

/// Jensen guitar speakers, 8 Ω versions, from Jensen Loudspeakers' web
/// specification sheets (jensentone.com/specification-sheet/16, 38, 15
/// and 36; tables in `crates/stave-test/datasheet/jensen_*`).
///
/// Each driver comes twice. The plain constant is the sheet's printed
/// Thiele–Small table (Re, Le at 1 kHz, Bl, Mms, Cms, Sd; `Rms` from the
/// printed Qms at the resonance of Mms and Cms, since the sheets print
/// `Res = Bl²/Rms` and it disagrees with their own Qms in two of the
/// four). The `_LR2` constant is the same driver fitted by
/// `scripts/speaker-fit.py` to the sheet's published free-air impedance
/// curve: Re, fs, Qms, Qes and an LR-2 coil, holding the printed Mms and
/// Sd. A guitar speaker's coil is far from a pure inductor (the
/// printed Le overstates |Z| at 10 kHz by up to 2.5×), and the curve's
/// resonance and DC resistance differ from the printed table by up to 6
/// % and 15 %; the fitted constant is the driver the curve describes.
impl ThieleSmall {
    /// Jensen C12N (12", ceramic, 50 W), 8 Ω, as printed: fs 113 Hz,
    /// Qms 7.52, Qes 1.18, Qts 1.02, Le 0.9 mH.
    pub const JENSEN_C12N: Self = Self {
        re: 6.05,
        le: 0.9e-3,
        l2: 0.0,
        r2: 0.0,
        bl: 10.46,
        mms: 29.9e-3,
        cms: 66e-6,
        rms: 2.830387,
        sd: 490.9e-4,
    };
    /// Jensen C12N, 8 Ω, fitted to its impedance curve: fs 107.4 Hz,
    /// Qms 5.58, Qes 1.08.
    pub const JENSEN_C12N_LR2: Self = Self {
        re: 6.953931,
        le: 4.204099e-4,
        l2: 1.023858e-3,
        r2: 8.420519,
        bl: 11.37876,
        mms: 29.9e-3,
        cms: 7.349034e-5,
        rms: 3.615492,
        sd: 490.9e-4,
    };
    /// Jensen P12N (12", alnico, 50 W), 8 Ω, as printed: fs 90 Hz, Qms
    /// 4.36, Qes 0.94, Qts 0.77, Le 0.87 mH.
    pub const JENSEN_P12N: Self = Self {
        re: 6.03,
        le: 0.87e-3,
        l2: 0.0,
        r2: 0.0,
        bl: 10.62,
        mms: 30.9e-3,
        cms: 101e-6,
        rms: 4.011733,
        sd: 490.9e-4,
    };
    /// Jensen P12N, 8 Ω, fitted to its impedance curve: fs 90.6 Hz,
    /// Qms 4.35, Qes 1.04.
    pub const JENSEN_P12N_LR2: Self = Self {
        re: 6.700087,
        le: 3.712811e-4,
        l2: 7.493536e-4,
        r2: 8.527517,
        bl: 10.67093,
        mms: 30.9e-3,
        cms: 9.995889e-5,
        rms: 4.041773,
        sd: 490.9e-4,
    };
    /// Jensen C12K (12", ceramic, 100 W), 8 Ω, as printed: fs 115 Hz,
    /// Qms 16.4, Qes 0.86, Qts 0.82, Le 0.84 mH.
    pub const JENSEN_C12K: Self = Self {
        re: 6.75,
        le: 0.84e-3,
        l2: 0.0,
        r2: 0.0,
        bl: 13.7,
        mms: 33.4e-3,
        cms: 57e-6,
        rms: 1.476019,
        sd: 490.9e-4,
    };
    /// Jensen C12K, 8 Ω, fitted to its impedance curve: fs 118.6 Hz,
    /// Qms 15.5, Qes 1.03.
    pub const JENSEN_C12K_LR2: Self = Self {
        re: 7.545975,
        le: 4.471070e-4,
        l2: 1.001486e-3,
        r2: 8.205008,
        bl: 13.48952,
        mms: 33.4e-3,
        cms: 5.389901e-5,
        rms: 1.610645,
        sd: 490.9e-4,
    };
    /// Jensen P10R (10", alnico, 25 W), 8 Ω, as printed: fs 97 Hz, Qms
    /// 14.83, Qes 1.82, Qts 1.62, Le 0.54 mH.
    pub const JENSEN_P10R: Self = Self {
        re: 6.68,
        le: 0.54e-3,
        l2: 0.0,
        r2: 0.0,
        bl: 5.83,
        mms: 15.2e-3,
        cms: 177e-6,
        rms: 0.6248762,
        sd: 330.1e-4,
    };
    /// Jensen P10R, 8 Ω, fitted to its impedance curve: fs 96.3 Hz,
    /// Qms 20.8, Qes 1.64.
    pub const JENSEN_P10R_LR2: Self = Self {
        re: 7.282321,
        le: 2.829589e-4,
        l2: 4.702973e-4,
        r2: 5.948369,
        bl: 6.398379,
        mms: 15.2e-3,
        cms: 1.796519e-4,
        rms: 0.4426771,
        sd: 330.1e-4,
    };
}
