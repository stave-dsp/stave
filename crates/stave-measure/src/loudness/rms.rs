use super::{LevelMeasure, LevelMeter, StreamSpec};
use crate::level::power_db;

/// Root-mean-square over every sample of every channel, dBFS.
/// `sine_referenced` adds 3.01 dB so a full-scale sine reads 0 dBFS (the
/// AES17 convention) instead of −3.01.
#[derive(Copy, Clone, Debug, Default)]
pub struct Rms {
    pub sine_referenced: bool,
}

const SINE_REFERENCE_DB: f64 = 3.010_299_956_639_812;

impl LevelMeasure for Rms {
    fn name(&self) -> &'static str {
        "rms"
    }

    fn meter(&self, _stream: &StreamSpec) -> Box<dyn LevelMeter> {
        let offset_db = if self.sine_referenced {
            SINE_REFERENCE_DB
        } else {
            0.0
        };
        Box::new(RmsMeter {
            sum_of_squares: 0.0,
            samples: 0,
            offset_db,
        })
    }
}

struct RmsMeter {
    sum_of_squares: f64,
    samples: u64,
    offset_db: f64,
}

impl LevelMeter for RmsMeter {
    fn push(&mut self, block: &[&[f32]]) {
        for &x in block
            .iter()
            .flat_map(|c| c.iter())
            .filter(|x| x.is_finite())
        {
            self.sum_of_squares += f64::from(x) * f64::from(x);
            self.samples += 1;
        }
    }

    fn level(&self) -> Option<f64> {
        (self.samples > 0)
            .then(|| power_db(self.sum_of_squares / self.samples as f64) + self.offset_db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_scale_sine() -> Vec<f32> {
        (0..48_000)
            .map(|i| (std::f32::consts::TAU * 1_000.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_reads_minus_three_or_zero_when_sine_referenced() {
        let s = full_scale_sine();
        let plain = Rms::default().measure(&[&s], 48_000.0).unwrap();
        let referenced = Rms {
            sine_referenced: true,
        }
        .measure(&[&s], 48_000.0)
        .unwrap();
        assert!((plain + 3.0103).abs() < 1e-3, "{plain}");
        assert!(referenced.abs() < 1e-3, "{referenced}");
    }

    #[test]
    fn channels_pool_their_samples() {
        let r = Rms::default()
            .measure(&[&[1.0f32; 4], &[0.0f32; 4]], 1.0)
            .unwrap();
        assert!((r + 3.0103).abs() < 1e-3);
    }
}
