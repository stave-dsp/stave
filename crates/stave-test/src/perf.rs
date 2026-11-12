//! The perf report — colocated like the no-alloc checks, but it
//! DOCUMENTS instead of blocking (the house decision: perf gates on
//! desktop hardware cry wolf; the numbers belong in front of eyes).
//! Every project declares its representative configurations; the
//! report measures them the way the doctrine demands (single-run
//! numbers on a desktop carry ±5–8% noise — measured):
//! * warm-up first, then REPEATED timed runs, MEDIAN taken;
//! * ns/frame printed beside the realtime budget so the number
//!   means something (48 kHz = 20 833 ns/frame);
//! * spread printed so a noisy run discredits itself.
//!
//! Debug builds SKIP (opt-level lies about release cost); measure
//! with `cargo test --release -p <crate> --test perf -- --nocapture`
//! when touching DSP, and update the baselines recorded in each
//! project's test header when they legitimately move.

use stave::{Event, EventKind, NoteEvent, NoteKind, Plugin};
use stave_measure::signal::{Noise, Signal};

use crate::PluginHarness;

/// One measured configuration: a name for the table and the plain
/// param values that put the engine in that state.
pub struct PerfCase {
    pub name: &'static str,
    pub set: &'static [(&'static str, f64)],
}

#[derive(Clone, Debug)]
pub struct PerfConfig {
    pub sample_rate: f32,
    pub block_size: usize,
    /// Untimed, before the first timed run: caches, parameter ramps.
    pub warmup_blocks: usize,
    /// Timed runs per case; the median is reported.
    pub repeats: usize,
    pub seconds_per_repeat: f32,
    /// Every input channel, effects only.
    pub input: Noise,
    /// Instruments are measured SOUNDING, this chord held from the
    /// first block: an idle rack's cost is not the number that matters
    /// in a session.
    pub chord: Vec<i16>,
    pub velocity: f64,
}

impl Default for PerfConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_size: 256,
            warmup_blocks: 200,
            repeats: 5,
            seconds_per_repeat: 3.0,
            input: Noise {
                seed: 7,
                amplitude: 0.3,
            },
            chord: vec![36, 43, 48, 55, 60, 64, 67, 72],
            velocity: 0.8,
        }
    }
}

/// Measure every case and PRINT the report — no assertions, by
/// decision.
pub fn perf_report<P: Plugin>(project: &str, cases: &[PerfCase], config: &PerfConfig) {
    if cfg!(debug_assertions) {
        eprintln!(
            "{project}: perf report SKIPPED (debug build — run under `cargo test --release`)"
        );
        return;
    }
    let block = config.block_size;
    let budget_ns = 1e9 / f64::from(config.sample_rate);
    eprintln!(
        "{project}: perf report (budget {budget_ns:.0} ns/frame at {} Hz)",
        config.sample_rate
    );
    let chord: Vec<Event> = config
        .chord
        .iter()
        .enumerate()
        .map(|(i, &key)| Event {
            frame: 0,
            kind: EventKind::Note(NoteEvent {
                kind: NoteKind::On,
                key,
                channel: 0,
                velocity: config.velocity,
                note_id: i as i32,
            }),
        })
        .collect();
    let input: Vec<f32> = config.input.samples(
        0,
        (config.seconds_per_repeat * config.sample_rate) as usize + block,
    );
    for case in cases {
        let mut h = PluginHarness::<P>::new(config.sample_rate, block);
            let table = h.table();
        for (name, value) in case.set {
            table
                .by_name(name)
                .unwrap_or_else(|| panic!("{project}: perf case param '{name}'"))
                .set_value(*value);
        }
        let is_instrument = h.input_channels() == 0;
        let mut events: &[Event] = if is_instrument { &chord } else { &[] };
        let mut run = |h: &mut PluginHarness<P>, blocks: usize| {
            for b in 0..blocks {
                let at = (b * block) % (input.len() - block);
                for ch in h.inputs_mut() {
                    ch.copy_from_slice(&input[at..at + block]);
                }
                h.process(events);
                events = &[];
            }
        };
        run(&mut h, config.warmup_blocks);
        let blocks = (config.seconds_per_repeat * config.sample_rate) as usize / block;
        let mut samples: Vec<f64> = (0..config.repeats)
            .map(|_| {
                let t0 = std::time::Instant::now();
                run(&mut h, blocks);
                t0.elapsed().as_secs_f64() / (blocks * block) as f64 * 1e9
            })
            .collect();
        samples.sort_by(f64::total_cmp);
        let median = samples[samples.len() / 2];
        let spread = (samples[samples.len() - 1] - samples[0]) / median * 100.0;
        eprintln!(
            "  {:<20} {median:>8.1} ns/frame  {:>5.1}% of budget  (spread {spread:.0}%)",
            case.name,
            median / budget_ns * 100.0
        );
    }
}
