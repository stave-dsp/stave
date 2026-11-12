//! The bench's phases. Each takes the driver where the last left it and
//! returns what it measured.

use crate::note::NoteSchedule;
use crate::drive::Driver;
use stave_measure::latency::{impulse_latency, ImpulseLatency};
use stave_measure::level::{mean_square, power_db, windowed_dc};
use stave_measure::loudness::{LevelMeasure, MeterSet, Outside, Reading, StreamSpec};
use stave_measure::signal::{Impulse, Signal, Silence, Sine, Step};
use stave_measure::step::{StepAnalysis, StepResponse};
use stave_measure::stereo::{StereoAnalysis, StereoImage};
use stave_measure::tone::{self, ToneReport};
use stave_measure::AUDIBLE_MAX_HZ;

use super::report::{CpuBudget, CpuReport, DcReport};

/// How long each phase listens, and the step it plays.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PhaseTimings {
    /// Silence before the latency impulse, so earlier input has drained.
    pub latency_flush_seconds: f64,
    pub latency_listen_seconds: f64,
    /// The residual is read over the second half of this silence.
    pub silence_seconds: f64,
    pub step: StepPhase,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct StepPhase {
    pub level: f32,
    pub settle_seconds: f64,
    pub listen_seconds: f64,
}

impl Default for PhaseTimings {
    fn default() -> Self {
        Self {
            latency_flush_seconds: 1.0,
            latency_listen_seconds: 0.5,
            silence_seconds: 1.0,
            step: StepPhase {
                level: 0.5,
                settle_seconds: 0.25,
                listen_seconds: 0.5,
            },
        }
    }
}

/// Flush with silence, then a unit impulse on a block boundary; latency
/// per output channel from the response.
pub fn latency(d: &mut Driver, timings: &PhaseTimings) -> Vec<Option<ImpulseLatency>> {
    let quiet = NoteSchedule::default();
    let flush = d.frames_for(timings.latency_flush_seconds);
    let flush = flush.div_ceil(d.block() as u64) * d.block() as u64;
    d.skip(flush, &Silence, &quiet);
    let at = d.frame();
    let listen = d.frames_for(timings.latency_listen_seconds);
    let cap = d.capture(listen, &Impulse { frame: at }, &quiet);
    cap.channels.iter().map(|ch| impulse_latency(ch)).collect()
}

/// What the timed run plays, and what it listens for.
pub struct TimedRun<'a> {
    pub signal: &'a dyn Signal,
    pub notes: &'a NoteSchedule,
    pub frames: u64,
    /// The exact tone fed in, when the stimulus is a sine.
    pub tone: Option<Sine>,
    pub analysis_window: usize,
    pub measures: &'a [Box<dyn LevelMeasure>],
    pub capture: bool,
}

/// What the timed run measured.
pub struct Timed {
    pub cpu: Option<CpuReport>,
    pub levels: Vec<Reading>,
    pub non_finite_samples: u64,
    pub tone: Option<Vec<ToneReport>>,
    /// Channels 0 and 1 over the last analysis window.
    pub stereo: Option<StereoImage>,
    /// Per channel, over the last analysis window.
    pub dc: Vec<DcReport>,
    /// Every output frame, when capture was requested.
    pub capture: Option<Vec<Vec<f32>>>,
}

/// Pre-roll one analysis window of the signal (untimed, no notes) to
/// reach steady state, then run `run.frames` from the stimulus's
/// start, timing every `process` call. The last analysis window of each
/// channel feeds the tone analysis.
///
/// The pre-roll is exactly one window long: a bin-snapped sine repeats
/// every window, so restarting the clock after it continues the tone
/// without a phase jump.
pub fn timed(d: &mut Driver, run: &TimedRun) -> Timed {
    d.reset_clock();
    d.skip(
        run.analysis_window as u64,
        run.signal,
        &NoteSchedule::default(),
    );
    d.reset_clock();
    let channels = d.instance().layout().outputs;
    let (rate, block) = (d.instance().layout().sample_rate, d.block());
    let mut block_ns = Vec::with_capacity((run.frames / block as u64 + 1) as usize);
    let mut meters = MeterSet::new(
        run.measures,
        &StreamSpec {
            channels,
            sample_rate: rate,
            outside: Outside::Unknown,
        },
    );
    let mut non_finite_samples = 0u64;
    let mut window = vec![Tail::new(run.analysis_window); channels];
    let mut captured = run
        .capture
        .then(|| vec![Vec::with_capacity(run.frames as usize); channels]);
    d.run(run.frames, run.signal, run.notes, |b| {
        block_ns.push(b.process_time().as_nanos() as f64);
        let outs: Vec<&[f32]> = b.outputs().collect();
        meters.push(&outs);
        non_finite_samples += outs
            .iter()
            .flat_map(|o| o.iter())
            .filter(|x| !x.is_finite())
            .count() as u64;
        for (w, o) in window.iter_mut().zip(&outs) {
            w.push(o);
        }
        if let Some(cap) = captured.as_mut() {
            cap.iter_mut()
                .zip(&outs)
                .for_each(|(c, o)| c.extend_from_slice(o));
        }
    });
    let tone = run.tone.and_then(|s| {
        window
            .iter()
            .map(|w| {
                w.full().then(|| {
                    tone::analyze(
                        &w.ordered(),
                        &tone::ToneAnalysis {
                            input: Sine {
                                frequency_hz: s.frequency_hz,
                                amplitude: s.amplitude,
                                sample_rate: rate,
                            },
                            max_harmonic_hz: AUDIBLE_MAX_HZ,
                        },
                    )
                })
            })
            .collect()
    });
    let stereo = match window.as_slice() {
        [l, r, ..] if l.full() && r.full() => {
            Some(StereoAnalysis::default().analyze(&l.ordered(), &r.ordered()))
        }
        _ => None,
    };
    let dc = window
        .iter()
        .filter(|w| w.full())
        .map(|w| {
            let x = w.ordered();
            let (first, second) = x.split_at(x.len() / 2);
            DcReport {
                offset: windowed_dc(&x),
                drift: windowed_dc(second) - windowed_dc(first),
            }
        })
        .collect();
    Timed {
        cpu: CpuReport::from_block_ns(
            &block_ns,
            &CpuBudget {
                block_frames: block,
                sample_rate: rate,
            },
        ),
        dc,
        levels: meters.readings(),
        non_finite_samples,
        tone,
        stereo,
        capture: captured,
    }
}

/// A step from silence, per channel, after the output has settled.
pub fn step(d: &mut Driver, phase: &StepPhase) -> Vec<StepResponse> {
    let quiet = NoteSchedule::default();
    d.skip(d.frames_for(phase.settle_seconds), &Silence, &quiet);
    let at = d.frame();
    let step = Step {
        frame: at,
        level: phase.level,
    };
    let cap = d.capture(d.frames_for(phase.listen_seconds), &step, &quiet);
    let analysis = StepAnalysis {
        level: f64::from(phase.level),
        sample_rate: d.instance().layout().sample_rate,
        ..StepAnalysis::default()
    };
    cap.channels.iter().map(|c| analysis.analyze(c)).collect()
}

/// Silence; RMS of its second half per channel (self-noise, denormal
/// hiss, a tail that never ends).
pub fn silence_residual(d: &mut Driver, timings: &PhaseTimings) -> Vec<f64> {
    let quiet = NoteSchedule::default();
    let half = d.frames_for(timings.silence_seconds / 2.0);
    d.skip(half, &Silence, &quiet);
    let cap = d.capture(half, &Silence, &quiet);
    cap.channels
        .iter()
        .map(|c| power_db(mean_square(c)))
        .collect()
}

/// The most recent `cap` samples of a stream, in a ring (no shifting).
#[derive(Clone)]
struct Tail {
    buf: Vec<f32>,
    cap: usize,
    pos: usize,
    seen: usize,
}

impl Tail {
    fn new(cap: usize) -> Self {
        Self {
            buf: vec![0.0; cap.max(1)],
            cap: cap.max(1),
            pos: 0,
            seen: 0,
        }
    }

    fn push(&mut self, xs: &[f32]) {
        for &x in xs {
            self.buf[self.pos] = x;
            self.pos = (self.pos + 1) % self.cap;
        }
        self.seen += xs.len();
    }

    fn full(&self) -> bool {
        self.seen >= self.cap
    }

    /// Oldest first.
    fn ordered(&self) -> Vec<f32> {
        let mut v = self.buf[self.pos..].to_vec();
        v.extend_from_slice(&self.buf[..self.pos]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Instance;
    use crate::testing::{Delay, Gain, BLOCK, RATE};
    use stave_measure::level::db;
    use stave_measure::loudness::Peak;
    use stave_measure::signal::db_to_amp;

    fn run<'a>(
        signal: &'a dyn Signal,
        frames: u64,
        tone: Option<Sine>,
        analysis_window: usize,
        measures: &'a [Box<dyn LevelMeasure>],
    ) -> TimedRun<'a> {
        static QUIET: std::sync::OnceLock<NoteSchedule> = std::sync::OnceLock::new();
        TimedRun {
            signal,
            notes: QUIET.get_or_init(NoteSchedule::default),
            frames,
            tone,
            analysis_window,
            measures,
            capture: true,
        }
    }

    #[test]
    fn tail_keeps_the_last_n_in_order() {
        let mut t = Tail::new(4);
        t.push(&[1.0, 2.0, 3.0]);
        assert!(!t.full());
        t.push(&[4.0, 5.0, 6.0]);
        assert_eq!(t.ordered(), [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn latency_of_a_delay_per_channel() {
        let mut dl = Delay::new(2, 37);
        let got = latency(&mut Driver::new(&mut dl), &PhaseTimings::default());
        let want = Some(ImpulseLatency {
            onset: 37,
            peak: 37,
        });
        assert_eq!(got, vec![want, want]);
    }

    #[test]
    fn timed_measures_gain_cpu_and_levels() {
        let mut g = Gain::new(2, 2);
        g.set_param("gain", 0.5).unwrap();
        let mut d = Driver::new(&mut g);
        let n = 4096;
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(-6.0),
            sample_rate: RATE,
        }
        .bin_snapped(n);
        let peak: Vec<Box<dyn LevelMeasure>> = vec![Box::new(Peak)];
        let t = timed(&mut d, &run(&s, 8192, Some(s), n, &peak));
        let tone = t.tone.unwrap();
        assert!((tone[1].gain_db + 6.0206).abs() < 1e-3, "{:?}", tone[1]);
        assert_eq!(t.cpu.unwrap().blocks, 8192 / BLOCK);
        assert!((t.levels[0].channels[0].unwrap() - db(s.amplitude * 0.5)).abs() < 0.01);
        assert_eq!(t.non_finite_samples, 0);
        assert_eq!(t.capture.unwrap()[0].len(), 8192);
    }

    #[test]
    fn finite_stimuli_play_whole_in_the_timed_run() {
        use stave_measure::signal::LogSweep;
        let mut g = Gain::new(1, 1);
        let mut d = Driver::new(&mut g);
        latency(&mut d, &PhaseTimings::default()); // a phase before advances the clock
        let s = LogSweep {
            start_hz: 100.0,
            end_hz: 1000.0,
            seconds: 0.1,
            amplitude: 0.5,
            sample_rate: RATE,
        };
        let t = timed(&mut d, &run(&s, s.frames(), None, 1024, &[]));
        assert_eq!(
            t.capture.unwrap()[0],
            (0..s.frames()).map(|f| s.at(0, f)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tone_needs_a_full_window() {
        let mut g = Gain::new(1, 1);
        let mut d = Driver::new(&mut g);
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(-6.0),
            sample_rate: RATE,
        };
        let t = timed(&mut d, &run(&s, 100, Some(s), 4096, &[]));
        assert!(t.tone.is_none(), "100 frames can't fill a 4096 window");
    }

    #[test]
    fn silence_residual_of_a_clean_plugin_is_the_floor() {
        let mut g = Gain::new(1, 1);
        let r = silence_residual(&mut Driver::new(&mut g), &PhaseTimings::default());
        assert_eq!(r, vec![power_db(0.0)]);
    }
}
