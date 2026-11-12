//! A system's response to a step of `level` from silence: where it
//! settles, how far it overshoots, how fast it rises and settles. A
//! system that blocks DC settles near zero; rise, overshoot and
//! settling are then undefined and reported as `None`.

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StepAnalysis {
    pub level: f64,
    pub sample_rate: f64,
    /// The share of the response averaged for the settled value.
    pub settled_fraction: f64,
    /// Settled means within this fraction of the settled value.
    pub settle_band: f64,
    /// Below this settled gain the system blocks DC, and rise,
    /// overshoot and settling are undefined.
    pub min_dc_gain: f64,
}

impl Default for StepAnalysis {
    fn default() -> Self {
        Self {
            level: 0.5,
            sample_rate: 48_000.0,
            settled_fraction: 0.1,
            settle_band: 0.02,
            min_dc_gain: 0.01,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct StepResponse {
    /// Settled output over the step level.
    pub final_gain: f64,
    /// Largest output over the step level.
    pub peak_gain: f64,
    pub overshoot_pct: Option<f64>,
    /// From 10% to 90% of the settled value.
    pub rise_s: Option<f64>,
    pub settle_s: Option<f64>,
}

impl StepAnalysis {
    pub fn analyze(&self, response: &[f32]) -> StepResponse {
        let y: Vec<f64> = response
            .iter()
            .map(|&v| f64::from(v) / self.level)
            .collect();
        let tail = ((y.len() as f64 * self.settled_fraction) as usize).max(1);
        let settled = y[y.len().saturating_sub(tail)..].iter().sum::<f64>() / tail as f64;
        let peak = y
            .iter()
            .copied()
            .fold(0.0, |m: f64, v| if v.abs() > m.abs() { v } else { m });
        let seconds = |i: usize| i as f64 / self.sample_rate;
        let min_dc_gain = settled.abs() >= self.min_dc_gain;
        let toward = |fraction: f64| {
            y.iter()
                .position(|&v| v * settled.signum() >= fraction * settled.abs())
        };
        let rise = match (toward(0.1), toward(0.9)) {
            (Some(a), Some(b)) if min_dc_gain => Some(seconds(b - a)),
            _ => None,
        };
        let band = self.settle_band * settled.abs();
        let settle = min_dc_gain.then(|| {
            let last_outside = y.iter().rposition(|&v| (v - settled).abs() > band);
            seconds(last_outside.map_or(0, |i| i + 1))
        });
        StepResponse {
            final_gain: settled,
            peak_gain: peak,
            overshoot_pct: min_dc_gain.then(|| 100.0 * ((peak - settled) / settled).max(0.0)),
            rise_s: rise,
            settle_s: settle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond(mut system: impl FnMut(f32) -> f32, analysis: &StepAnalysis) -> StepResponse {
        let y: Vec<f32> = (0..48_000).map(|_| system(analysis.level as f32)).collect();
        analysis.analyze(&y)
    }

    #[test]
    fn a_one_pole_lowpass_rises_in_2_2_time_constants_without_overshoot() {
        let analysis = StepAnalysis::default();
        let tau_s = 0.001;
        let a = 1.0 - (-1.0 / (tau_s * analysis.sample_rate)).exp();
        let mut y = 0.0f32;
        let r = respond(
            |x| {
                y += a as f32 * (x - y);
                y
            },
            &analysis,
        );
        assert!((r.final_gain - 1.0).abs() < 1e-4);
        assert!(r.overshoot_pct.unwrap() < 1e-3);
        assert!((r.rise_s.unwrap() - 2.197 * tau_s).abs() < 5e-5, "{r:?}");
        assert!((r.settle_s.unwrap() - 3.912 * tau_s).abs() < 5e-5, "{r:?}");
    }

    #[test]
    fn a_dc_blocker_settles_to_zero_with_no_rise_or_overshoot() {
        let (mut x1, mut y1) = (0.0f32, 0.0f32);
        let r = respond(
            |x| {
                y1 = x - x1 + 0.99 * y1;
                x1 = x;
                y1
            },
            &StepAnalysis::default(),
        );
        assert!(r.final_gain.abs() < StepAnalysis::default().min_dc_gain);
        assert!((r.peak_gain - 1.0).abs() < 1e-6);
        assert_eq!((r.rise_s, r.overshoot_pct, r.settle_s), (None, None, None));
    }

    #[test]
    fn an_underdamped_system_overshoots() {
        let (mut v, mut p) = (0.0f32, 0.0f32);
        let r = respond(
            |x| {
                v += 0.002 * (x - p) - 0.002 * v;
                p += v;
                p
            },
            &StepAnalysis::default(),
        );
        assert!(r.overshoot_pct.unwrap() > 50.0, "{r:?}");
    }
}
