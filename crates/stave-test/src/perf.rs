//! The perf check (DSP.md P6): a plugin's cost a sample a channel at
//! 48 kHz, in the cases it declares and in its worst parameter state,
//! held to the [`Budget`] the plugin declares as `Plugin::CPU_BUDGET`.
//!
//! How it measures, so a number means something:
//! * each case is a fresh instance prepared at 48 kHz with the case's
//!   values restored before `prepare` (an oversampling choice is in
//!   force from the first sample), effects fed noise on every input,
//!   instruments sounding a held chord;
//! * warm-up first (caches, ramps, voices), then REPEATED timed runs and
//!   the MEDIAN taken, the spread printed so a noisy run discredits
//!   itself;
//! * the time is the thread's CPU time (wall time on Windows), so being
//!   preempted is not counted as the plugin's cost; where Linux's
//!   counters can be read the instructions a sample are printed too, a
//!   number that is the same on every run;
//! * the worst parameter state is searched for, not guessed: every
//!   parameter but the bypass is moved alone from its default to each
//!   candidate value (every step of a stepped parameter of up to 16
//!   steps, else its ends and middle), each in a short run, and the
//!   costliest value of each parameter that cost more than the defaults
//!   is combined into the case `WORST KNOBS`, measured in full;
//! * per channel is the frame's cost over the larger of the main input
//!   and output channel counts.
//!
//! The verdict: with a declared budget, a release build fails when any
//! case's median is over it, naming the case, its values, and the stage
//! that took the time (from the plugin's P5 cost taps, `CostTaps`,
//! averaged over a further untimed run). With no budget declared the
//! check measures and prints against real time and fails nothing, and
//! says so. Debug builds skip: an unoptimised build misstates release
//! cost.
//!
//! ```sh
//! cargo test --release -p <crate> --test checks perf -- --nocapture
//! ```

use std::time::Duration;

use stave::{Budget, Event, EventKind, NoteEvent, NoteKind, Plugin};
use stave_host::counters::{thread_cpu_time, Instructions};
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
    /// The rate cases are measured at; a [`Budget`] is stated at 48 kHz.
    pub sample_rate: f32,
    pub block_size: usize,
    /// Untimed, before the first timed run: caches, parameter ramps.
    pub warmup_blocks: usize,
    /// Timed runs per case; the median is reported.
    pub repeats: usize,
    pub seconds_per_repeat: f32,
    /// Search for the worst parameter state and measure it as
    /// `WORST KNOBS`.
    pub worst_case: bool,
    /// The search's runs: untimed blocks, then this many seconds timed,
    /// once per candidate value.
    pub scan_warmup_blocks: usize,
    pub scan_seconds: f32,
    /// The untimed run whose stage cost taps name the hotspot.
    pub stage_seconds: f32,
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
            worst_case: true,
            scan_warmup_blocks: 40,
            scan_seconds: 0.25,
            stage_seconds: 1.0,
            input: Noise {
                seed: 7,
                amplitude: 0.3,
            },
            chord: vec![36, 43, 48, 55, 60, 64, 67, 72],
            velocity: 0.8,
        }
    }
}

/// The name of the searched worst case.
pub const WORST_KNOBS: &str = "WORST KNOBS";

/// One stage's average cost over the hotspot run, from its cost taps.
#[derive(Clone, Debug, PartialEq)]
pub struct StageCost {
    /// The tap's name without `_ns_per_sample`.
    pub name: String,
    pub ns_per_sample: f64,
}

/// One measured case.
#[derive(Clone, Debug)]
pub struct PerfRow {
    pub name: String,
    /// The values the case set, in their own units.
    pub set: Vec<(String, f64)>,
    /// Median CPU ns a frame (all channels).
    pub ns_per_frame: f64,
    /// The median over the channel count: what the budget limits.
    pub ns_per_sample_per_channel: f64,
    /// (slowest − fastest) / median, percent.
    pub spread_percent: f64,
    /// Median instructions a sample a channel, where counted.
    pub instructions_per_sample_per_channel: Option<f64>,
    /// The plugin's stage cost taps, costliest first (empty: none).
    pub stages: Vec<StageCost>,
}

/// What [`perf_report`] measured.
#[derive(Clone, Debug)]
pub struct PerfReport {
    pub project: String,
    pub budget: Budget,
    /// The channel count a frame's cost is divided by.
    pub channels: usize,
    pub rows: Vec<PerfRow>,
}

impl PerfReport {
    /// The rows over the budget (none when it is undeclared).
    pub fn over_budget(&self) -> impl Iterator<Item = &PerfRow> {
        let limit = self.budget.ns_per_sample();
        self.rows.iter().filter(move |r| limit.is_some_and(|b| r.ns_per_sample_per_channel > b))
    }

    /// The costliest row.
    pub fn worst(&self) -> Option<&PerfRow> {
        self.rows.iter().max_by(|a, b| a.ns_per_sample_per_channel.total_cmp(&b.ns_per_sample_per_channel))
    }

    /// The failure message for the rows over budget; `None` when within
    /// it or undeclared.
    pub fn verdict(&self) -> Option<String> {
        let limit = self.budget.ns_per_sample()?;
        let over: Vec<&PerfRow> = self.over_budget().collect();
        if over.is_empty() {
            return None;
        }
        let mut msg = format!(
            "{}: over its CPU budget of {limit:.1} ns a sample a channel at 48 kHz ({:.2}% of real time):",
            self.project,
            limit / Budget::REALTIME_NS * 100.0
        );
        for r in over {
            msg += &format!(
                "\n  {} costs {:.1} ns ({:.0}% of budget, spread {:.0}%){}",
                r.name,
                r.ns_per_sample_per_channel,
                r.ns_per_sample_per_channel / limit * 100.0,
                r.spread_percent,
                values(&r.set)
            );
            msg += &format!("\n    hotspot: {}", hotspot(r, self.channels));
        }
        msg += "\n  Make it cheaper, or raise `const CPU_BUDGET` in the plugin's `impl Plugin` with the reason beside it.";
        Some(msg)
    }
}

fn values(set: &[(String, f64)]) -> String {
    if set.is_empty() {
        return String::new();
    }
    let v: Vec<String> = set.iter().map(|(n, v)| format!("{n}={v}")).collect();
    format!(" at {}", v.join(", "))
}

fn hotspot(r: &PerfRow, channels: usize) -> String {
    let Some(top) = r.stages.first() else {
        return "no stage cost taps — wrap each stage's block in `CostTaps::measure` (DSP.md P5) to have it named here".into();
    };
    let frame = r.ns_per_frame.max(f64::MIN_POSITIVE);
    let rest: Vec<String> =
        r.stages.iter().skip(1).take(3).map(|s| format!("{} {:.0}", s.name, s.ns_per_sample)).collect();
    format!(
        "{} {:.0} ns a sample ({:.0}% of the frame's {:.0} ns over {channels} channel(s)){}",
        top.name,
        top.ns_per_sample,
        top.ns_per_sample / frame * 100.0,
        frame,
        if rest.is_empty() { String::new() } else { format!("; then {}", rest.join(", ")) }
    )
}

/// The input and the chord a run feeds.
struct Feed {
    input: Vec<f32>,
    chord: Vec<Event>,
}

impl Feed {
    fn new(config: &PerfConfig) -> Self {
        let chord = config
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
        let longest = config.seconds_per_repeat.max(config.scan_seconds).max(config.stage_seconds);
        let input = config.input.samples(0, (longest * config.sample_rate) as usize + 2 * config.block_size);
        Feed { input, chord }
    }
}

/// One instance in one state, with its feed position.
struct Run<'f, P: Plugin> {
    h: PluginHarness<P>,
    feed: &'f Feed,
    block: usize,
    at: usize,
    chord_sent: bool,
}

impl<'f, P: Plugin> Run<'f, P> {
    fn new(set: &[(String, f64)], config: &PerfConfig, feed: &'f Feed) -> Self {
        let mut b = PluginHarness::<P>::builder().sample_rate(config.sample_rate).block_size(config.block_size);
        for (name, value) in set {
            b = b.set(name, *value);
        }
        Run { h: b.build(), feed, block: config.block_size, at: 0, chord_sent: false }
    }

    fn blocks(&mut self, n: usize) {
        let block = self.block;
        let span = self.feed.input.len() - block;
        for _ in 0..n {
            let at = self.at % span;
            for ch in self.h.inputs_mut() {
                ch.copy_from_slice(&self.feed.input[at..at + block]);
            }
            let events: &[Event] = if self.chord_sent || self.h.input_channels() != 0 { &[] } else { &self.feed.chord };
            self.h.process(events);
            self.chord_sent = true;
            self.at += block;
        }
    }

    /// CPU ns a frame over `n` blocks, and instructions a frame when
    /// counted.
    fn timed(&mut self, n: usize, counter: &mut Option<Instructions>) -> (f64, Option<f64>) {
        let frames = (n * self.block) as f64;
        let (cpu, instructions) = match counter {
            Some(c) => {
                let (cpu, count) = c.count(|| cpu_time(|| self.blocks(n)));
                (cpu, (count > 0).then_some(count as f64 / frames))
            }
            None => (cpu_time(|| self.blocks(n)), None),
        };
        (cpu.as_secs_f64() * 1e9 / frames, instructions)
    }

    /// The stage cost taps' average over `n` blocks, costliest first.
    fn stages(&mut self, n: usize) -> Vec<StageCost> {
        let telemetry = self.h.telemetry().clone();
        let mut sums: Vec<(String, f64, u64)> = Vec::new();
        for _ in 0..n {
            telemetry.reset_taps();
            self.blocks(1);
            for t in telemetry.taps() {
                let Some(name) = t.name.strip_suffix("_ns_per_sample") else { continue };
                if t.hits == 0 {
                    continue;
                }
                match sums.iter_mut().find(|s| s.0 == name) {
                    Some(s) => {
                        s.1 += t.latest;
                        s.2 += 1;
                    }
                    None => sums.push((name.to_string(), t.latest, 1)),
                }
            }
        }
        let mut stages: Vec<StageCost> =
            sums.into_iter().map(|(name, sum, n)| StageCost { name, ns_per_sample: sum / n as f64 }).collect();
        stages.sort_by(|a, b| b.ns_per_sample.total_cmp(&a.ns_per_sample));
        stages
    }
}

/// CPU time of `work` on this thread (wall time on Windows, whose
/// per-thread clock is too coarse: [`thread_cpu_time`]).
fn cpu_time(work: impl FnOnce()) -> Duration {
    let t0 = thread_cpu_time();
    work();
    thread_cpu_time().saturating_sub(t0)
}

fn blocks_for(seconds: f32, config: &PerfConfig) -> usize {
    ((seconds * config.sample_rate) as usize / config.block_size).max(1)
}

/// The channel count a frame's cost is divided by.
fn channels<P: Plugin>() -> usize {
    let l = P::LAYOUTS[0];
    l.input_channels().max(l.output_channels()).max(1)
}

/// Search for the costliest parameter state (see the module docs): each
/// parameter's costliest candidate that beat the defaults.
pub fn worst_knobs<P: Plugin>(config: &PerfConfig) -> Vec<(String, f64)> {
    let feed = Feed::new(config);
    let scan = |set: &[(String, f64)]| {
        let mut run = Run::<P>::new(set, config, &feed);
        run.blocks(config.scan_warmup_blocks);
        run.timed(blocks_for(config.scan_seconds, config), &mut None).0
    };
    let base = scan(&[]);
    let table = PluginHarness::<P>::new(config.sample_rate, config.block_size).table();
    let mut worst = Vec::new();
    for i in 0..table.count() {
        let Some(p) = table.by_index(i) else { continue };
        if p.meta().flags.bypass {
            continue;
        }
        let steps = p.step_count();
        let candidates: Vec<f64> = if steps > 0 && steps <= 16 {
            (0..=steps).map(|s| f64::from(s) / f64::from(steps)).collect()
        } else {
            vec![0.0, 0.5, 1.0]
        };
        let default = p.default_normalized();
        let mut best: Option<(f64, f64)> = None;
        for n in candidates.into_iter().filter(|n| (n - default).abs() > 1e-9) {
            let value = p.denormalize(n);
            let ns = scan(&[(p.meta().name.to_string(), value)]);
            if best.is_none_or(|(_, b)| ns > b) {
                best = Some((value, ns));
            }
        }
        if let Some((value, _)) = best.filter(|&(_, ns)| ns > base) {
            worst.push((p.meta().name.to_string(), value));
        }
    }
    worst
}

/// Measure every case, and the worst parameter state, and PRINT the
/// report: never fails. `None` in a debug build, which skips.
pub fn perf_report<P: Plugin>(project: &str, cases: &[PerfCase], config: &PerfConfig) -> Option<PerfReport> {
    if cfg!(debug_assertions) {
        eprintln!("{project}: perf check SKIPPED (debug build — run under `cargo test --release`)");
        return None;
    }
    let report = measure::<P>(project, cases, config);
    print(&report, config, Instructions::open().is_some());
    Some(report)
}

/// [`perf_report`]'s measurement, in any build and printing nothing.
pub fn measure<P: Plugin>(project: &str, cases: &[PerfCase], config: &PerfConfig) -> PerfReport {
    let budget = P::CPU_BUDGET;
    let channels = channels::<P>();
    let feed = Feed::new(config);
    let mut counter = Instructions::open();
    let mut states: Vec<(String, Vec<(String, f64)>)> = cases
        .iter()
        .map(|c| (c.name.to_string(), c.set.iter().map(|(n, v)| (n.to_string(), *v)).collect()))
        .collect();
    if config.worst_case {
        states.push((WORST_KNOBS.to_string(), worst_knobs::<P>(config)));
    }
    let mut rows = Vec::new();
    for (name, set) in states {
        let mut run = Run::<P>::new(&set, config, &feed);
        run.blocks(config.warmup_blocks);
        let blocks = blocks_for(config.seconds_per_repeat, config);
        let mut times = Vec::with_capacity(config.repeats);
        let mut counts = Vec::with_capacity(config.repeats);
        for _ in 0..config.repeats.max(1) {
            let (ns, instructions) = run.timed(blocks, &mut counter);
            times.push(ns);
            counts.extend(instructions);
        }
        times.sort_by(f64::total_cmp);
        counts.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        let stages = run.stages(blocks_for(config.stage_seconds, config));
        rows.push(PerfRow {
            name,
            set,
            ns_per_frame: median,
            ns_per_sample_per_channel: median / channels as f64,
            spread_percent: (times[times.len() - 1] - times[0]) / median.max(f64::MIN_POSITIVE) * 100.0,
            instructions_per_sample_per_channel: (!counts.is_empty())
                .then(|| counts[counts.len() / 2] / channels as f64),
            stages,
        });
    }
    PerfReport { project: project.to_string(), budget, channels, rows }
}

fn print(r: &PerfReport, config: &PerfConfig, counted: bool) {
    let limit = r.budget.ns_per_sample();
    match limit {
        Some(b) => eprintln!(
            "{}: perf check — budget {b:.1} ns a sample a channel at 48 kHz ({:.2}% of real time); {} channel(s), CPU time, median of {} × {} s",
            r.project,
            b / Budget::REALTIME_NS * 100.0,
            r.channels,
            config.repeats,
            config.seconds_per_repeat
        ),
        None => eprintln!(
            "{}: perf check — NO BUDGET DECLARED (`const CPU_BUDGET`): reported, not checked; {} channel(s), CPU time, median of {} × {} s",
            r.project, r.channels, config.repeats, config.seconds_per_repeat
        ),
    }
    eprintln!(
        "  {:<24} {:>10} {:>11} {:>9} {:>11} {:>7}",
        "case",
        "ns/frame",
        "ns/smp/ch",
        if limit.is_some() { "budget" } else { "realtime" },
        "instr/smp/ch",
        "spread"
    );
    for row in &r.rows {
        let share = match limit {
            Some(b) => row.ns_per_sample_per_channel / b,
            None => row.ns_per_sample_per_channel / Budget::REALTIME_NS,
        };
        let instructions =
            row.instructions_per_sample_per_channel.map_or_else(|| "—".to_string(), |n| format!("{n:.0}"));
        eprintln!(
            "  {:<24} {:>10.1} {:>11.1} {:>8.1}% {:>11} {:>6.0}%{}",
            row.name,
            row.ns_per_frame,
            row.ns_per_sample_per_channel,
            share * 100.0,
            instructions,
            row.spread_percent,
            if limit.is_some_and(|b| row.ns_per_sample_per_channel > b) { "  OVER" } else { "" }
        );
        if row.name == WORST_KNOBS {
            eprintln!("    {}", if row.set.is_empty() { "the defaults: no parameter cost more alone".into() } else { values(&row.set).trim_start().to_string() });
        }
        if let Some(top) = row.stages.first() {
            eprintln!("    hotspot: {} {:.0} ns a sample", top.name, top.ns_per_sample);
        }
    }
    if !counted {
        eprintln!("  (instruction counts unavailable here: Linux perf counters only, where the kernel grants them)");
    }
}

/// The perf check: [`perf_report`], then, in a release build, fail when
/// the plugin's declared [`Budget`] is exceeded in any case.
pub fn perf_check<P: Plugin>(project: &str, cases: &[PerfCase], config: &PerfConfig) {
    if let Some(msg) = perf_report::<P>(project, cases, config).and_then(|r| r.verdict()) {
        panic!("{msg}");
    }
}
