#[cfg(test)]
use super::Outside;
use super::{LevelMeasure, LevelMeter, StreamSpec};

/// One measure's reading: over all channels, and per channel.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Reading {
    pub measure: &'static str,
    pub unit: &'static str,
    pub program: Option<f64>,
    pub channels: Vec<Option<f64>>,
}

/// Several measures run side by side over one stream.
pub struct MeterSet {
    meters: Vec<Meters>,
}

struct Meters {
    measure: &'static str,
    unit: &'static str,
    program: Box<dyn LevelMeter>,
    channels: Vec<Box<dyn LevelMeter>>,
}

impl MeterSet {
    pub fn new(measures: &[Box<dyn LevelMeasure>], stream: &StreamSpec) -> Self {
        let mono = StreamSpec {
            channels: 1,
            ..*stream
        };
        let meters = measures
            .iter()
            .map(|m| Meters {
                measure: m.name(),
                unit: m.unit(),
                program: m.meter(stream),
                channels: (0..stream.channels).map(|_| m.meter(&mono)).collect(),
            })
            .collect();
        Self { meters }
    }

    pub fn push(&mut self, block: &[&[f32]]) {
        for m in &mut self.meters {
            m.program.push(block);
            for (meter, channel) in m.channels.iter_mut().zip(block) {
                meter.push(&[channel]);
            }
        }
    }

    pub fn readings(&self) -> Vec<Reading> {
        self.meters
            .iter()
            .map(|m| Reading {
                measure: m.measure,
                unit: m.unit,
                program: m.program.level(),
                channels: m.channels.iter().map(|c| c.level()).collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loudness::{Peak, Rms};

    #[test]
    fn reads_every_measure_over_the_program_and_each_channel() {
        let measures: Vec<Box<dyn LevelMeasure>> = vec![Box::new(Peak), Box::new(Rms::default())];
        let mut set = MeterSet::new(
            &measures,
            &StreamSpec {
                channels: 2,
                sample_rate: 48_000.0,
                outside: Outside::Silence,
            },
        );
        set.push(&[&[0.5, -0.5], &[0.25, 0.25]]);
        let r = set.readings();
        assert_eq!((r[0].measure, r[1].measure), ("peak", "rms"));
        let db = |x: f64| 20.0 * x.log10();
        assert!((r[0].program.unwrap() - db(0.5)).abs() < 1e-9);
        assert!((r[0].channels[1].unwrap() - db(0.25)).abs() < 1e-9);
        assert_eq!(r[1].unit, "dBFS");
    }
}
