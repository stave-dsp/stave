//! Tone windows: a bin-centred stimulus played through a sample-by-sample
//! system, captured as one steady-state window. Because the tone repeats
//! exactly every window, harmonics and alias products land on exact bins
//! and no window function is needed.

use crate::nonlinear::{alias_report, AliasBands, AliasReport};
use crate::spectrum::{fft, goertzel};
use crate::system::System;
use crate::AUDIBLE_MAX_HZ;

/// One sine at `bin` cycles per `window` samples.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ToneWindow {
    pub bin: usize,
    pub window: usize,
    pub sample_rate: f64,
    pub amplitude: f64,
    /// Whole windows played before the capture, so the captured phase
    /// continues the warm-up's.
    pub warmup_windows: usize,
}

impl Default for ToneWindow {
    fn default() -> Self {
        Self {
            bin: 171,
            window: 8192,
            sample_rate: 48_000.0,
            amplitude: 0.5,
            warmup_windows: 1,
        }
    }
}

impl ToneWindow {
    /// The bin of `window` whose tone is nearest `hz`.
    pub(crate) fn bin_near(hz: f64, window: usize, sample_rate: f64) -> usize {
        ((hz * window as f64 / sample_rate).round() as usize).max(1)
    }

    pub fn frequency(&self) -> f64 {
        self.bin as f64 * self.sample_rate / self.window as f64
    }

    /// The system's steady-state response: one window after the warm-up.
    pub fn capture(&self, system: impl System) -> Vec<f64> {
        let tone = |i: usize| {
            let cycles = self.bin as f64 * i as f64 / self.window as f64;
            self.amplitude * (std::f64::consts::TAU * cycles).sin()
        };
        capture_window(self.window, self.warmup_windows, tone, system)
    }

    /// Alias-to-signal ratios of the response, full band and audible.
    pub fn alias(&self, system: impl System) -> AliasReport {
        alias_report(
            &self.capture(system),
            &AliasBands {
                carrier_bin: self.bin,
                sample_rate: self.sample_rate,
                audible_max_hz: AUDIBLE_MAX_HZ,
            },
        )
    }

    /// Gain at the tone's frequency, dB. Magnitude only, so a fixed
    /// latency doesn't bias it; probe through a nonlinearity at a small
    /// amplitude to read its linear response.
    pub fn gain_db(&self, system: impl System) -> f64 {
        let out = self.capture(system);
        20.0 * (goertzel(&out, (self.frequency()) / self.sample_rate) / self.amplitude).log10()
    }
}

/// Two sines, at `low_bin` and `high_bin` cycles per `window`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TwoToneWindow {
    pub low_bin: usize,
    pub high_bin: usize,
    pub window: usize,
    pub amplitude: f64,
    pub warmup_windows: usize,
}

impl TwoToneWindow {
    fn sine(&self, bin: usize, i: usize) -> f64 {
        (std::f64::consts::TAU * bin as f64 * i as f64 / self.window as f64).sin()
    }

    fn power_spectrum(&self, mix: impl Fn(usize) -> f64, system: impl System) -> Vec<f64> {
        let mut re = capture_window(self.window, self.warmup_windows, mix, system);
        let mut im = vec![0.0; self.window];
        fft(&mut re, &mut im);
        re.iter().zip(&im).map(|(r, i)| r * r + i * i).collect()
    }

    /// SMPTE intermodulation: the low tone at four times the high tone's
    /// level; sidebands high ± low and high ± 2·low against the high
    /// carrier, dB.
    pub fn smpte_db(&self, system: impl System) -> f64 {
        let (lo, hi) = (self.low_bin, self.high_bin);
        assert!(
            hi > 2 * lo && hi + 2 * lo < self.window / 2,
            "sidebands must fit below Nyquist"
        );
        let mix = |i| self.amplitude * (0.8 * self.sine(lo, i) + 0.2 * self.sine(hi, i));
        let p = self.power_spectrum(mix, system);
        let sidebands = p[hi - lo] + p[hi + lo] + p[hi - 2 * lo] + p[hi + 2 * lo];
        10.0 * (sidebands / p[hi].max(1e-30)).log10()
    }

    /// CCIF intermodulation: two equal tones; the difference product
    /// (high − low) against one carrier, dB.
    pub fn ccif_db(&self, system: impl System) -> f64 {
        let (lo, hi) = (self.low_bin, self.high_bin);
        assert!(hi > lo, "the high tone must be above the low one");
        let mix = |i| self.amplitude * (self.sine(lo, i) + self.sine(hi, i));
        let p = self.power_spectrum(mix, system);
        10.0 * (p[hi - lo] / p[lo].max(1e-30)).log10()
    }
}

fn capture_window(
    window: usize,
    warmup_windows: usize,
    stimulus: impl Fn(usize) -> f64,
    mut system: impl System,
) -> Vec<f64> {
    let mut input = vec![0.0f32; window];
    let mut output = vec![0.0f32; window];
    for w in 0..=warmup_windows {
        let start = w * window;
        for (i, x) in input.iter_mut().enumerate() {
            *x = stimulus(start + i) as f32;
        }
        system.process(&input, &mut output);
    }
    output.into_iter().map(f64::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_of_a_halving_system_is_minus_six_db() {
        let test = ToneWindow {
            bin: 85,
            window: 4096,
            ..ToneWindow::default()
        };
        assert!((test.gain_db(|x: f32| 0.5 * x) + 6.0206).abs() < 1e-6);
        assert!((test.frequency() - 85.0 * 48_000.0 / 4096.0).abs() < 1e-9);
    }

    #[test]
    fn a_linear_system_has_no_alias_and_a_clipper_does() {
        let test = ToneWindow {
            bin: 41,
            window: 4096,
            amplitude: 0.9,
            ..ToneWindow::default()
        };
        assert!(
            test.alias(|x: f32| x).full_db < -120.0,
            "only f32 rounding remains"
        );
        let hard = test.alias(|x: f32| x.clamp(-0.3, 0.3));
        assert!(
            hard.full_db > -60.0,
            "clipping folds harmonics above Nyquist"
        );
    }

    #[test]
    fn capture_continues_the_warmup_phase() {
        let tone = |warmup_windows| ToneWindow {
            bin: 3,
            window: 64,
            warmup_windows,
            ..ToneWindow::default()
        };
        let (a, b) = (tone(2).capture(|x: f32| x), tone(0).capture(|x: f32| x));
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6));
    }

    #[test]
    fn intermodulation_of_linear_and_squaring_systems() {
        let test = TwoToneWindow {
            low_bin: 8,
            high_bin: 100,
            window: 2048,
            amplitude: 0.5,
            warmup_windows: 1,
        };
        assert!(test.smpte_db(|x: f32| x) < -120.0);
        assert!(test.ccif_db(|x: f32| x) < -120.0);
        assert!(
            test.ccif_db(|x: f32| x + 0.5 * x * x) > -40.0,
            "x² makes the difference tone"
        );
    }
}
