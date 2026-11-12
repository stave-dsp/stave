//! The bench: one instance, a stimulus, and everything measurable about
//! the pair — CPU, latency (reported vs measured), output levels, tone
//! gain/THD, silence residual, the plugin's own taps. Pure library:
//! `stave-bench` parses its own arguments and prints [`BenchReport`]s.

pub mod describe;
mod phases;
pub use phases::{PhaseTimings, StepPhase};
pub mod report;
pub mod stimulus;
pub mod text;

use std::time::Instant;

use crate::instance::Instance;
use crate::note::{Note, NoteSchedule};
use crate::drive::Driver;
use stave_measure::loudness;

pub use report::BenchReport;
pub use stimulus::{Stimulus, StimulusError};

/// The level measures a run reports unless told otherwise.
pub const DEFAULT_MEASURES: [&str; 4] = ["peak", "true-peak", "rms", "lufs-i"];

#[derive(Clone, Debug, PartialEq)]
pub struct BenchConfig {
    pub stimulus: Stimulus,
    /// Length of the timed run.
    pub seconds: f64,
    /// Scheduled notes (instruments; note-aware effects).
    pub notes: Vec<Note>,
    /// Keep every output frame of the timed run.
    pub capture: bool,
    /// Level measures to report, by name (see `stave_measure::loudness`).
    pub measures: Vec<String>,
    /// Frames in the tone-analysis window; sines snap to its bins.
    pub analysis_window: usize,
    pub phases: PhaseTimings,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            stimulus: Stimulus::default(),
            seconds: 3.0,
            notes: Vec::new(),
            capture: false,
            measures: DEFAULT_MEASURES.map(String::from).to_vec(),
            analysis_window: 32_768,
            phases: PhaseTimings::default(),
        }
    }
}

/// A finished run: the report, plus the captured audio when asked for.
#[derive(Debug)]
pub struct BenchRun {
    pub report: BenchReport,
    pub capture: Option<Vec<Vec<f32>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error(transparent)]
    Stimulus(#[from] StimulusError),
    #[error(transparent)]
    Measures(#[from] loudness::UnknownMeasures),
}

/// Times each phase in wall-clock time and frames processed.
#[derive(Default)]
struct PhaseClock {
    phases: Vec<report::PhaseTiming>,
}

impl PhaseClock {
    fn time<T>(
        &mut self,
        d: &mut Driver,
        phase: &'static str,
        run: impl FnOnce(&mut Driver) -> T,
    ) -> T {
        let (started, frames_before) = (Instant::now(), d.processed());
        let result = run(d);
        self.phases.push(report::PhaseTiming {
            phase,
            frames: d.processed() - frames_before,
            wall_ms: started.elapsed().as_secs_f64() * 1e3,
        });
        result
    }
}

pub fn run(inst: &mut dyn Instance, cfg: &BenchConfig) -> Result<BenchRun, BenchError> {
    let layout = inst.layout();
    let rate = layout.sample_rate;
    let built = cfg.stimulus.build(rate, cfg.analysis_window, cfg.seconds)?;
    let measures = loudness::by_names(&cfg.measures)?;
    let notes = NoteSchedule::new(&cfg.notes, rate);
    let mut warnings: Vec<String> = built.warning.into_iter().collect();
    if layout.is_instrument() && cfg.notes.is_empty() {
        warnings.push("instrument with no notes runs silence — add --notes".into());
    }
    let reported = inst.latency();
    let mut d = Driver::new(inst);
    let block = d.block();
    let mut clock = PhaseClock::default();
    let measured = (!layout.is_instrument())
        .then(|| clock.time(&mut d, "latency", |d| phases::latency(d, &cfg.phases)));
    let frames = d.frames_for(cfg.seconds).max(8 * block as u64);
    if frames < cfg.analysis_window as u64 {
        warnings.push(format!(
            "timed run ({frames} frames) is shorter than the analysis window \
             ({}): tone, stereo and DC are not measured",
            cfg.analysis_window
        ));
    }
    let (timed, taps) = clock.time(&mut d, "timed", |d| {
        let timed = phases::timed(
            d,
            &phases::TimedRun {
                signal: built.signal.as_ref(),
                notes: &notes,
                frames,
                tone: built.tone,
                analysis_window: cfg.analysis_window,
                measures: &measures,
                capture: cfg.capture,
            },
        );
        (timed, d.instance().taps())
    });
    let residual = clock.time(&mut d, "silence", |d| {
        phases::silence_residual(d, &cfg.phases)
    });
    let step = (!layout.is_instrument())
        .then(|| clock.time(&mut d, "step", |d| phases::step(d, &cfg.phases.step)));
    let phases = clock.phases;
    let report = BenchReport {
        sample_rate: rate,
        block,
        stimulus: cfg.stimulus.clone(),
        cpu: timed.cpu,
        latency: report::LatencyReport { reported, measured },
        output: report::OutputReport {
            levels: timed.levels,
            non_finite_samples: timed.non_finite_samples,
            silence_residual_dbfs: residual,
            tone: timed.tone,
            stereo: timed.stereo,
            dc: timed.dc,
            step,
        },
        taps,
        phases,
        warnings,
    };
    Ok(BenchRun {
        report,
        capture: timed.capture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Delay, Synth};

    fn short() -> BenchConfig {
        BenchConfig {
            seconds: 0.2,
            analysis_window: 4_096,
            ..BenchConfig::default()
        }
    }

    #[test]
    fn a_run_shorter_than_its_analysis_window_says_what_it_skipped() {
        let mut dl = Delay::new(2, 1);
        let cfg = BenchConfig {
            analysis_window: 32_768,
            ..short()
        };
        let r = run(&mut dl, &cfg).unwrap().report;
        assert!(r.output.tone.is_none() && r.output.stereo.is_none());
        assert_eq!(r.warnings.len(), 1);
        assert!(
            r.warnings[0].contains("analysis window"),
            "{:?}",
            r.warnings
        );
    }

    #[test]
    fn effect_run_reports_every_phase() {
        let mut dl = Delay::new(2, 12);
        let r = run(&mut dl, &short()).unwrap().report;
        let phases: Vec<_> = r.phases.iter().map(|p| p.phase).collect();
        assert_eq!(phases, ["latency", "timed", "silence", "step"]);
        assert_eq!(r.latency.reported, 12);
        assert_eq!(r.latency.measured.unwrap()[1].unwrap().onset, 12);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn instrument_skips_latency_and_warns_without_notes() {
        let mut s = Synth::new();
        let r = run(&mut s, &short()).unwrap().report;
        assert!(r.latency.measured.is_none());
        assert_eq!(r.phases[0].phase, "timed");
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn instrument_with_notes_makes_sound() {
        let mut s = Synth::new();
        let cfg = BenchConfig {
            notes: vec![Note {
                pitch: 60,
                velocity: 1.0,
                on_s: 0.0,
                off_s: 10.0,
            }],
            stimulus: Stimulus::Silence,
            ..short()
        };
        let r = run(&mut s, &cfg).unwrap().report;
        let peak = r
            .output
            .levels
            .iter()
            .find(|l| l.measure == "peak")
            .unwrap();
        assert!(peak.program.unwrap() > -7.0, "{peak:?}");
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn reports_exactly_the_measures_asked_for() {
        let mut dl = Delay::new(1, 1);
        let cfg = BenchConfig {
            measures: vec!["lufs-i".into()],
            ..short()
        };
        let r = run(&mut dl, &cfg).unwrap().report;
        let names: Vec<_> = r.output.levels.iter().map(|l| l.measure).collect();
        assert_eq!(names, ["lufs-i"]);
        let bad = BenchConfig {
            measures: vec!["vu".into()],
            ..short()
        };
        assert!(matches!(run(&mut dl, &bad), Err(BenchError::Measures(m)) if m.unknown == ["vu"]));
    }

    #[test]
    fn bad_stimulus_is_an_error_not_a_panic() {
        let mut dl = Delay::new(1, 1);
        let cfg = BenchConfig {
            stimulus: Stimulus::Wav {
                path: "/nope.wav".into(),
            },
            ..short()
        };
        assert!(run(&mut dl, &cfg).is_err());
    }
}
