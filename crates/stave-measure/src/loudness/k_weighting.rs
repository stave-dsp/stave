//! The BS.1770 K-weighting filter: a high-shelf (head diffraction) then
//! the RLB high-pass, derived from their analog prototypes so the same
//! curve holds at every sample rate.

const SHELF_HZ: f64 = 1_681.974_450_955_533;
const SHELF_GAIN_DB: f64 = 3.999_843_853_973_347;
const SHELF_Q: f64 = 0.707_175_236_955_419_6;
const HIGH_PASS_HZ: f64 = 38.135_470_876_024_44;
const HIGH_PASS_Q: f64 = 0.500_327_037_323_877_3;

#[derive(Copy, Clone, Debug, PartialEq)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// One channel's K-weighting state.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    pub fn new(sample_rate: f64) -> Self {
        let k = (std::f64::consts::PI * SHELF_HZ / sample_rate).tan();
        let vh = 10f64.powf(SHELF_GAIN_DB / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / SHELF_Q + k * k;
        let shelf = Biquad {
            b: [
                (vh + vb * k / SHELF_Q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / SHELF_Q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / SHELF_Q + k * k) / a0],
            z: [0.0; 2],
        };
        let k = (std::f64::consts::PI * HIGH_PASS_HZ / sample_rate).tan();
        let a0 = 1.0 + k / HIGH_PASS_Q + k * k;
        let high_pass = Biquad {
            b: [1.0, -2.0, 1.0],
            a: [
                2.0 * (k * k - 1.0) / a0,
                (1.0 - k / HIGH_PASS_Q + k * k) / a0,
            ],
            z: [0.0; 2],
        };
        Self { shelf, high_pass }
    }

    pub fn process(&mut self, x: f32) -> f64 {
        self.high_pass.process(self.shelf.process(f64::from(x)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficients_at_48k_match_the_standard() {
        let k = KWeighting::new(48_000.0);
        let close =
            |got: &[f64], want: &[f64]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-8);
        assert!(close(
            &k.shelf.b,
            &[
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85
            ]
        ));
        assert!(close(
            &k.shelf.a,
            &[-1.690_659_293_182_41, 0.732_480_774_215_85]
        ));
        assert!(close(
            &k.high_pass.a,
            &[-1.990_047_454_833_98, 0.990_072_250_366_21]
        ));
    }
}
