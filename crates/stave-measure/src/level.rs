//! Levels: dB conversion, peak, RMS, DC, and a streaming meter.

/// Floor for log conversions (−240 dB): silence reads as a number.
const FLOOR: f64 = 1e-12;

/// Amplitude → dBFS.
pub fn db(amp: f64) -> f64 {
    20.0 * amp.max(FLOOR).log10()
}

/// Power (mean square) → dB.
pub fn power_db(power: f64) -> f64 {
    10.0 * power.max(FLOOR * FLOOR).log10()
}

pub fn peak(x: &[f32]) -> f64 {
    x.iter().fold(0.0f64, |m, &v| m.max(f64::from(v).abs()))
}

pub fn mean_square(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64
}

pub fn rms(x: &[f32]) -> f64 {
    mean_square(x).sqrt()
}

pub fn dc(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|&v| f64::from(v)).sum::<f64>() / x.len() as f64
}

/// The DC level of `x` weighted by a Hann window, so a tone that does not
/// fit a whole number of cycles leaves almost no ripple in the estimate.
pub fn windowed_dc(x: &[f32]) -> f64 {
    let w = crate::window::Window::Hann.coefficients(x.len());
    let total: f64 = w.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    x.iter()
        .zip(&w)
        .map(|(&v, w)| f64::from(v) * w)
        .sum::<f64>()
        / total
}

/// Pearson correlation of two channels, in [−1, 1] (0 when either is
/// constant): +1 mono, −1 polarity-inverted, ~0 decorrelated.
pub fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let (ma, mb) = (dc(&a[..n]), dc(&b[..n]));
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a[..n].iter().zip(&b[..n]) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    let denom = (va * vb).sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        cov / denom
    }
}

/// Peak + RMS over a stream, per channel. Non-finite samples are counted
/// instead of poisoning the sums.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    peak: Vec<f64>,
    sum_sq: Vec<f64>,
    frames: u64,
    non_finite: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ChannelLevel {
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
}

impl Meter {
    pub fn add(&mut self, channels: &[&[f32]]) {
        if self.peak.len() < channels.len() {
            self.peak.resize(channels.len(), 0.0);
            self.sum_sq.resize(channels.len(), 0.0);
        }
        for (c, ch) in channels.iter().enumerate() {
            for &v in *ch {
                if !v.is_finite() {
                    self.non_finite += 1;
                    continue;
                }
                let v = f64::from(v);
                self.peak[c] = self.peak[c].max(v.abs());
                self.sum_sq[c] += v * v;
            }
        }
        self.frames += channels.first().map_or(0, |c| c.len() as u64);
    }

    pub fn levels(&self) -> Vec<ChannelLevel> {
        let n = self.frames.max(1) as f64;
        self.peak
            .iter()
            .zip(&self.sum_sq)
            .map(|(&p, &s)| ChannelLevel {
                peak_dbfs: db(p),
                rms_dbfs: power_db(s / n),
            })
            .collect()
    }

    /// The loudest channel's peak, linear.
    pub fn peak(&self) -> f64 {
        self.peak.iter().copied().fold(0.0, f64::max)
    }

    /// RMS over every channel's samples, linear.
    pub fn rms(&self) -> f64 {
        let samples = self.frames as f64 * self.sum_sq.len().max(1) as f64;
        (self.sum_sq.iter().sum::<f64>() / samples.max(1.0)).sqrt()
    }

    pub fn non_finite(&self) -> u64 {
        self.non_finite
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn windowed_dc_ignores_a_tone_cut_mid_cycle() {
        let x: Vec<f32> = (0..16_384)
            .map(|i| {
                (0.1 + 0.25 * (std::f64::consts::TAU * 1_000.0 * i as f64 / 48_000.0).sin()) as f32
            })
            .collect();
        assert!((dc(&x) - 0.1).abs() > 1e-4);
        assert!((windowed_dc(&x) - 0.1).abs() < 1e-6);
    }

    use super::*;

    #[test]
    fn db_conversions() {
        assert!(db(1.0).abs() < 1e-12);
        assert!((db(0.5) + 6.0206).abs() < 1e-4);
        assert_eq!(db(0.0), -240.0);
        assert!((power_db(0.5) + 3.0103).abs() < 1e-4);
    }

    #[test]
    fn sine_rms_is_peak_over_root_two() {
        let x: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * std::f32::consts::TAU / 48.0).sin())
            .collect();
        assert!((rms(&x) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-4);
        assert!((peak(&x) - 1.0).abs() < 1e-3);
        assert!(dc(&x).abs() < 1e-6);
    }

    #[test]
    fn meter_streams_per_channel_and_counts_non_finite() {
        let mut m = Meter::default();
        m.add(&[&[0.5, -0.5], &[0.0, 0.0]]);
        m.add(&[&[f32::NAN, 0.25], &[1.0, f32::INFINITY]]);
        let l = m.levels();
        assert!((l[0].peak_dbfs - db(0.5)).abs() < 1e-12);
        assert!(l[1].peak_dbfs.abs() < 1e-12);
        assert_eq!((m.non_finite(), m.peak()), (2, 1.0));
    }

    #[test]
    fn correlation_of_mono_inverted_and_constant() {
        let a: Vec<f32> = (0..100).map(|i| (i as f32 * 0.3).sin()).collect();
        let inv: Vec<f32> = a.iter().map(|v| -v).collect();
        assert!((correlation(&a, &a) - 1.0).abs() < 1e-9);
        assert!((correlation(&a, &inv) + 1.0).abs() < 1e-9);
        assert_eq!(correlation(&a, &[0.5; 100]), 0.0);
        assert_eq!(correlation(&[], &[]), 0.0);
    }

    #[test]
    fn meter_rms_spans_channels() {
        let mut m = Meter::default();
        m.add(&[&[1.0, 1.0], &[0.0, 0.0]]);
        assert!((m.rms() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    }

    #[test]
    fn empty_inputs_are_silent_not_nan() {
        assert_eq!((rms(&[]), dc(&[]), peak(&[])), (0.0, 0.0, 0.0));
        assert_eq!(Meter::default().levels(), vec![]);
        assert_eq!(Meter::default().rms(), 0.0);
    }
}
