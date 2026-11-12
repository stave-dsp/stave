//! Level measures as interchangeable strategies. A measure turns a
//! multichannel capture into one number in dB (dBFS, or LUFS for the
//! BS.1770 family); callers pick which by value or by name.

mod k_weighting;
mod lufs;
mod meter_set;
mod peak;
mod rms;
mod true_peak;

pub use k_weighting::KWeighting;
pub use lufs::{Lufs, LufsWindow};
pub use meter_set::{MeterSet, Reading};
pub use peak::Peak;
pub use rms::Rms;
pub use true_peak::TruePeak;

/// The shape of a stream being metered.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StreamSpec {
    pub channels: usize,
    pub sample_rate: f64,
    pub outside: Outside,
}

/// What lies outside the samples a meter is given.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Outside {
    /// Silence, as when a clip is played: its abrupt edges are signal.
    #[default]
    Silence,
    /// More of the same stream, unseen: the meter sees a window cut
    /// from it, and the cut edges are not signal.
    Unknown,
}

/// A way of measuring level: the configuration that makes meters.
pub trait LevelMeasure {
    /// The name `by_name` knows this measure by.
    fn name(&self) -> &'static str;

    /// "dBFS", or "LUFS" for the BS.1770 family.
    fn unit(&self) -> &'static str {
        "dBFS"
    }

    fn meter(&self, stream: &StreamSpec) -> Box<dyn LevelMeter>;

    /// The level of a whole capture (equal-length channels).
    fn measure(&self, channels: &[&[f32]], sample_rate: f64) -> Option<f64> {
        let mut meter = self.meter(&StreamSpec {
            channels: channels.len(),
            sample_rate,
            outside: Outside::Silence,
        });
        meter.push(channels);
        meter.level()
    }
}

/// A running measurement: push blocks as they arrive, read the level
/// any time. `None` where the level is undefined so far (nothing pushed,
/// not a full block yet, nothing above a gate).
pub trait LevelMeter {
    fn push(&mut self, block: &[&[f32]]);
    fn level(&self) -> Option<f64>;
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("unknown level measures {unknown:?}; known: {}", NAMES.join(", "))]
pub struct UnknownMeasures {
    pub unknown: Vec<String>,
}

pub fn by_names<S: AsRef<str>>(names: &[S]) -> Result<Vec<Box<dyn LevelMeasure>>, UnknownMeasures> {
    let unknown: Vec<String> = names
        .iter()
        .map(AsRef::as_ref)
        .filter(|n| by_name(n).is_none())
        .map(String::from)
        .collect();
    if unknown.is_empty() {
        Ok(names.iter().filter_map(|n| by_name(n.as_ref())).collect())
    } else {
        Err(UnknownMeasures { unknown })
    }
}

/// Every built-in measure, by the names reports and command lines use.
pub fn by_name(name: &str) -> Option<Box<dyn LevelMeasure>> {
    let measure: Box<dyn LevelMeasure> = match name {
        "peak" => Box::new(Peak),
        "true-peak" => Box::new(TruePeak::default()),
        "rms" => Box::new(Rms::default()),
        "lufs-m" => Box::new(Lufs::momentary()),
        "lufs-s" => Box::new(Lufs::short_term()),
        "lufs-i" | "lufs" => Box::new(Lufs::integrated()),
        _ => return None,
    };
    Some(measure)
}

pub const NAMES: [&str; 6] = ["peak", "true-peak", "rms", "lufs-m", "lufs-s", "lufs-i"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_in_odd_chunks_matches_one_push() {
        let rate = 48_000.0;
        let x: Vec<f32> = (0..rate as usize * 4)
            .map(|i| (0.3 * (i as f64 * 0.0371).sin() + 0.2 * (i as f64 * 0.00511).sin()) as f32)
            .collect();
        for name in NAMES {
            let measure = by_name(name).unwrap();
            let whole = measure.measure(&[&x, &x], rate);
            let mut meter = measure.meter(&StreamSpec {
                channels: 2,
                sample_rate: rate,
                outside: Outside::Silence,
            });
            for chunk in x.chunks(997) {
                meter.push(&[chunk, chunk]);
            }
            let streamed = meter.level();
            let (a, b) = (whole.unwrap(), streamed.unwrap());
            assert!((a - b).abs() < 1e-9, "{name}: one push {a}, streamed {b}");
        }
    }

    #[test]
    fn every_listed_name_resolves_to_itself() {
        for name in NAMES {
            assert_eq!(by_name(name).map(|m| m.name()), Some(name));
        }
        assert!(by_name("vu").is_none());
    }
}
