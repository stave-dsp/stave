use super::{LevelMeasure, LevelMeter, StreamSpec};
use crate::level::db;

/// The largest absolute sample over every channel, dBFS.
#[derive(Copy, Clone, Debug, Default)]
pub struct Peak;

impl LevelMeasure for Peak {
    fn name(&self) -> &'static str {
        "peak"
    }

    fn meter(&self, _stream: &StreamSpec) -> Box<dyn LevelMeter> {
        Box::new(PeakMeter::default())
    }
}

#[derive(Default)]
struct PeakMeter {
    loudest: Option<f64>,
}

impl LevelMeter for PeakMeter {
    fn push(&mut self, block: &[&[f32]]) {
        for &x in block
            .iter()
            .flat_map(|c| c.iter())
            .filter(|x| x.is_finite())
        {
            let a = f64::from(x).abs();
            self.loudest = Some(self.loudest.map_or(a, |m| m.max(a)));
        }
    }

    fn level(&self) -> Option<f64> {
        self.loudest.map(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudest_sample_of_any_channel() {
        let l = [0.25, -0.5];
        let r = [0.1, 0.0];
        assert!((Peak.measure(&[&l, &r], 48_000.0).unwrap() + 6.0206).abs() < 1e-3);
        assert_eq!(Peak.measure(&[], 48_000.0), None);
    }

    #[test]
    fn non_finite_samples_are_not_a_level() {
        assert!((Peak.measure(&[&[f32::NAN, 0.5]], 1.0).unwrap() + 6.0206).abs() < 1e-3);
    }
}
