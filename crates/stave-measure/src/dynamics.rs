//! Dynamics: how gain moves over time, how long it takes, and what a
//! system leaves behind after its input stops.

use crate::system::System;
use std::f64::consts::TAU;
use std::ops::Range;

/// A carrier whose level steps low → high → low, each step held for
/// `hold_seconds`; gain is read in RMS windows of `window` samples.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GainStepTest {
    pub frequency_hz: f64,
    pub sample_rate: f64,
    pub low_db: f64,
    pub high_db: f64,
    pub hold_seconds: f64,
    pub window: usize,
}

/// Output/input gain per window; the high step spans
/// `hold_windows..2 * hold_windows`.
#[derive(Clone, Debug, PartialEq)]
pub struct GainTrajectory {
    pub window_seconds: f64,
    pub gains_db: Vec<f64>,
    pub hold_windows: usize,
}

impl GainStepTest {
    pub fn run(&self, mut system: impl System) -> GainTrajectory {
        let hold_windows = ((self.hold_seconds * self.sample_rate) as usize / self.window).max(1);
        let level = |db: f64| 10f64.powf(db / 20.0);
        let mut frame = 0usize;
        let gains_db = (0..3 * hold_windows)
            .map(|w| {
                let high = (hold_windows..2 * hold_windows).contains(&w);
                let amplitude = level(if high { self.high_db } else { self.low_db });
                let (mut input_power, mut output_power) = (0.0f64, 0.0f64);
                for _ in 0..self.window {
                    let x = amplitude
                        * (TAU * self.frequency_hz * frame as f64 / self.sample_rate).sin();
                    let y = f64::from(system.tick(x as f32));
                    input_power += x * x;
                    output_power += y * y;
                    frame += 1;
                }
                10.0 * (output_power / input_power.max(1e-30)).log10()
            })
            .collect();
        GainTrajectory {
            window_seconds: self.window as f64 / self.sample_rate,
            gains_db,
            hold_windows,
        }
    }
}

impl GainTrajectory {
    /// Seconds to travel 63.2% (one time constant) from the gain at
    /// `span.start` toward the settled gain at `span.end`.
    pub fn time_constant_s(&self, span: Range<usize>) -> Option<f64> {
        let start = *self.gains_db.get(span.start)?;
        let settled = *self.gains_db.get(span.end)?;
        let target = start + 0.632 * (settled - start);
        let rising = settled > start;
        self.gains_db[span.start..]
            .iter()
            .position(|&g| if rising { g >= target } else { g <= target })
            .map(|k| k as f64 * self.window_seconds)
    }
}

/// A tone burst then silence: the AC energy left in the last quarter of
/// the silence, dBFS. A self-oscillating or limit-cycling loop leaves
/// energy behind; a DC offset (a legitimately asymmetric curve) doesn't
/// count.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SilenceResidualTest {
    pub frequency_hz: f64,
    pub sample_rate: f64,
    pub amplitude: f64,
    pub burst_seconds: f64,
    pub silence_seconds: f64,
}

impl SilenceResidualTest {
    pub fn residual_db(&self, mut system: impl System) -> f64 {
        let burst = (self.burst_seconds * self.sample_rate) as usize;
        for i in 0..burst {
            let x = self.amplitude * (TAU * self.frequency_hz * i as f64 / self.sample_rate).sin();
            system.tick(x as f32);
        }
        let silence = (self.silence_seconds * self.sample_rate) as usize;
        let tail_start = silence - silence / 4;
        let tail: Vec<f64> = (0..silence)
            .map(|_| f64::from(system.tick(0.0)))
            .skip(tail_start)
            .collect();
        let mean = tail.iter().sum::<f64>() / tail.len() as f64;
        let ac_power = tail.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / tail.len() as f64;
        10.0 * ac_power.max(1e-30).log10()
    }
}

/// Index at which `values` first decays to `fraction` of `values[0]`
/// (magnitudes); `None` if it never does. For a one-pole exponential,
/// `fraction = 1/e` gives τ in samples.
pub fn time_to_fraction(values: &[f32], fraction: f32) -> Option<usize> {
    let first = values.first()?.abs();
    if first <= 0.0 {
        return None;
    }
    values.iter().position(|v| v.abs() <= first * fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_to_fraction_measures_a_one_pole() {
        let tau = 480.0f32;
        let v: Vec<f32> = (0..4800).map(|i| (-(i as f32) / tau).exp()).collect();
        let k = time_to_fraction(&v, (-1.0f32).exp()).unwrap();
        assert!(
            (k as f32 - tau).abs() < 2.0,
            "τ measured {k}, expected {tau}"
        );
    }

    #[test]
    fn a_fixed_gain_has_a_flat_trajectory() {
        let test = GainStepTest {
            frequency_hz: 1_000.0,
            sample_rate: 48_000.0,
            low_db: -30.0,
            high_db: -6.0,
            hold_seconds: 0.1,
            window: 480,
        };
        let t = test.run(|x: f32| 0.5 * x);
        assert_eq!((t.hold_windows, t.gains_db.len()), (10, 30));
        assert!(t.gains_db.iter().all(|g| (g + 6.0206).abs() < 1e-3));
    }

    #[test]
    fn time_constant_of_a_one_pole_gain_glide() {
        let t = GainTrajectory {
            window_seconds: 0.001,
            gains_db: (0..200)
                .map(|k| -10.0 * (1.0 - (-(k as f64) / 20.0).exp()))
                .collect(),
            hold_windows: 0,
        };
        let tau = t.time_constant_s(0..199).unwrap();
        assert!((tau - 0.020).abs() <= 0.001, "{tau}");
    }

    #[test]
    fn a_memoryless_system_leaves_nothing_after_silence() {
        let test = SilenceResidualTest {
            frequency_hz: 500.0,
            sample_rate: 48_000.0,
            amplitude: 0.8,
            burst_seconds: 0.05,
            silence_seconds: 0.1,
        };
        assert!(test.residual_db(|x: f32| x.tanh()) < -200.0);
        let mut phase = 0.0f32;
        let ringing = test.residual_db(|_: f32| {
            phase += 0.1;
            0.1 * phase.sin()
        });
        assert!(ringing > -30.0, "{ringing}");
    }
}
