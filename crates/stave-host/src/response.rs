//! Frequency response of an instance: a log sweep in, the H1 transfer
//! estimate out, per output channel. Deterministic and offline — the
//! same answer at any block size, on any machine, with nothing else
//! playing.

use crate::instance::Instance;
use crate::opened::OpenError;
use crate::note::NoteSchedule;
use crate::drive::Driver;
use stave_measure::signal::{db_to_amp, Gate, LogSweep, Signal};
use stave_measure::transfer::{self, TransferFunction};
use stave_measure::{AUDIBLE_MAX_HZ, AUDIBLE_MIN_HZ};

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct ResponseConfig {
    pub start_hz: f64,
    /// Lowered to `max_fraction_of_nyquist` of the instance's Nyquist
    /// frequency, where resamplers and anti-alias filters roll off.
    pub end_hz: f64,
    pub max_fraction_of_nyquist: f64,
    pub seconds: f64,
    pub level_db: f64,
    pub fft_size: usize,
}

impl Default for ResponseConfig {
    fn default() -> Self {
        Self {
            start_hz: AUDIBLE_MIN_HZ,
            end_hz: AUDIBLE_MAX_HZ,
            max_fraction_of_nyquist: 0.9,
            seconds: 2.0,
            level_db: -12.0,
            fft_size: 8192,
        }
    }
}

/// One transfer function per output channel (input: the sweep on every
/// input channel). The output is aligned by the REPORTED latency before
/// estimating — H1 averages per window, so a delay that is a sizeable
/// fraction of the window would otherwise bias the gain low. (A plugin
/// that misreports its latency keeps a small residual bias; the bench's
/// latency phase measures the truth.)
pub fn measure(inst: &mut dyn Instance, cfg: &ResponseConfig) -> Vec<TransferFunction> {
    let rate = inst.layout().sample_rate;
    let latency = u64::from(inst.latency());
    let sweep = LogSweep {
        start_hz: cfg.start_hz,
        end_hz: cfg.end_hz.min(cfg.max_fraction_of_nyquist * rate / 2.0),
        seconds: cfg.seconds,
        amplitude: db_to_amp(cfg.level_db),
        sample_rate: rate,
    };
    let frames = sweep.frames() + latency + cfg.fft_size as u64;
    let input = Gate {
        inner: sweep,
        until: sweep.frames(),
    };
    let mut d = Driver::new(inst);
    d.reset_clock();
    let cap = d.capture(frames, &input, &NoteSchedule::default());
    let aligned = (frames - latency) as usize;
    let x: Vec<f32> = (0..aligned as u64).map(|f| input.at(0, f)).collect();
    cap.channels
        .iter()
        .map(|y| {
            transfer::estimate(
                &x,
                &y[latency as usize..],
                &transfer::TransferConfig {
                    sample_rate: rate,
                    fft_size: cfg.fft_size,
                    ..transfer::TransferConfig::default()
                },
            )
        })
        .collect()
}

/// A response per input level: where a nonlinear system's tone moves
/// with drive. Each level gets a fresh instance so no state carries over.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct LevelResponseConfig {
    pub levels_db: Vec<f64>,
    pub response: ResponseConfig,
    /// Where the report reads each response.
    pub read_hz: Vec<f64>,
}

impl Default for LevelResponseConfig {
    fn default() -> Self {
        Self {
            levels_db: vec![-36.0, -24.0, -12.0, -6.0, 0.0],
            response: ResponseConfig::default(),
            read_hz: OCTAVES.to_vec(),
        }
    }
}

pub const OCTAVES: [f64; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct LevelResponse {
    pub read_hz: Vec<f64>,
    /// `gain_db[level][point]` for channel 0; `None` where the sweep had
    /// no energy.
    pub levels_db: Vec<f64>,
    pub gain_db: Vec<Vec<Option<f64>>>,
    /// Per point: the largest gain minus the smallest across levels.
    pub spread_db: Vec<Option<f64>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    #[error(transparent)]
    Load(#[from] OpenError),
    #[error("the instance has no outputs")]
    NoOutputs,
}

pub fn measure_levels(
    fresh: &mut dyn FnMut() -> Result<Box<dyn Instance>, OpenError>,
    cfg: &LevelResponseConfig,
) -> Result<LevelResponse, ResponseError> {
    let mut gain_db = Vec::with_capacity(cfg.levels_db.len());
    for &level_db in &cfg.levels_db {
        let mut inst = fresh()?;
        let tfs = measure(
            inst.as_mut(),
            &ResponseConfig {
                level_db,
                ..cfg.response
            },
        );
        let tf = tfs.first().ok_or(ResponseError::NoOutputs)?;
        gain_db.push(
            cfg.read_hz
                .iter()
                .map(|&hz| tf.gain_at(hz))
                .collect::<Vec<_>>(),
        );
    }
    let spread_db = (0..cfg.read_hz.len())
        .map(|p| {
            let gains: Option<Vec<f64>> = gain_db.iter().map(|g: &Vec<Option<f64>>| g[p]).collect();
            gains.map(|g| {
                let max = g.iter().copied().fold(f64::MIN, f64::max);
                let min = g.iter().copied().fold(f64::MAX, f64::min);
                max - min
            })
        })
        .collect();
    Ok(LevelResponse {
        read_hz: cfg.read_hz.clone(),
        levels_db: cfg.levels_db.clone(),
        gain_db,
        spread_db,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Delay, Gain};

    fn quick() -> ResponseConfig {
        ResponseConfig {
            seconds: 0.5,
            fft_size: 4096,
            ..ResponseConfig::default()
        }
    }

    #[test]
    fn gain_reads_flat_on_every_channel() {
        let mut g = Gain::new(2, 2);
        g.set_control("gain", 0.5).unwrap();
        let tfs = measure(&mut g, &quick());
        assert_eq!(tfs.len(), 2);
        for tf in &tfs {
            for hz in [100.0, 1_000.0, 10_000.0] {
                let got = tf.gain_at(hz).unwrap();
                assert!((got + 6.0206).abs() < 0.05, "{hz} Hz: {got}");
            }
        }
    }

    #[test]
    fn a_linear_system_has_no_level_dependence_and_a_clipper_does() {
        let cfg = LevelResponseConfig {
            levels_db: vec![-30.0, 0.0],
            response: quick(),
            read_hz: vec![1_000.0],
        };
        let mut gain = || Ok(Box::new(Gain::new(1, 1)) as Box<dyn Instance>);
        let flat = measure_levels(&mut gain, &cfg).unwrap();
        assert!(flat.spread_db[0].unwrap() < 0.01, "{flat:?}");

        let stage = crate::stages::find("clipper/diode").unwrap();
        let mut clipper = || {
            let mono = crate::config::HostConfig {
                channels: std::num::NonZeroUsize::new(1).unwrap(),
                ..crate::config::HostConfig::default()
            };
            Ok(Box::new(crate::stages::CatalogInstance::new(stage, &mono)) as Box<dyn Instance>)
        };
        let driven = measure_levels(&mut clipper, &cfg).unwrap();
        assert!(driven.spread_db[0].unwrap() > 1.0, "{driven:?}");
    }

    #[test]
    fn a_delayed_plugin_still_reads_unity() {
        let mut d = Delay::new(1, 480);
        let tf = &measure(&mut d, &quick())[0];
        assert!(tf.gain_at(1_000.0).unwrap().abs() < 0.1);
    }
}
