//! Commands that open a fresh instance per run: `validate` (the
//! validation suite) and `response` (frequency response against level).

use stave_host::bench::text;
use stave_host::response::{measure_levels, LevelResponseConfig};
use stave_host::validate::{Status as CheckStatus, Suite, SuiteReport};

use crate::command::{CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};
use crate::target::{factory, reopen, BLOCK, PRESET, RATE, SET, TARGET};

pub const VALIDATE: CommandInfo = CommandInfo {
    id: "validate",
    title: "Validate a plugin",
    summary: "Run the validation suite; fails on any failing check",
    description: "Run every check of the validation suite, each on a fresh instance: output sanity, \
                  sweeps of every param, glides, noise, channels, the solver, tails, the deadline, \
                  state round trips and bypass. Fails (exit 1) when any check fails or the plugin \
                  reports misbehaving. Progress names each check as it finishes.",
    inputs: &[TARGET, RATE, BLOCK, SET, PRESET],
    outputs: &[
        Field::flag("checks", Kind::Json, "Each check: name, status, duration_ms, details, plugin_messages").required(),
        Field::flag("failures", Kind::Integer, "How many checks failed").required(),
    ],
    long_running: true,
};

pub fn validate(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let (target, config) = factory(input, cx.workspace)?;
    let suite = Suite::default();
    let total = suite.checks.len() as f64;
    let mut fresh = || reopen(&target, config);
    let mut results = Vec::new();
    for (i, check) in suite.checks.iter().enumerate() {
        cx.checkpoint()?;
        let r = suite.run_check(&mut fresh, check);
        cx.progress((i + 1) as f64, Some(total), format!("{:<18} {:?} ({:.0} ms)", r.check, r.status, r.duration_ms));
        results.push(r);
    }
    let report = SuiteReport::from_results(results);
    let mut text = String::new();
    for r in &report.checks {
        text.push_str(&format!("{:<18} {:<14} {:>7.0} ms\n", r.check, format!("{:?}", r.status), r.duration_ms));
        if r.status == CheckStatus::Fail {
            text.push_str(&format!("    {}\n", r.details));
        }
    }
    text.push_str(&format!(
        "{}: {} checks, {} failed",
        if report.passed() { "PASSED" } else { "FAILED" },
        report.checks.len(),
        report.failures
    ));
    Ok(Outcome::ok(&report, text)?.failed_if(!report.passed()))
}

pub const RESPONSE: CommandInfo = CommandInfo {
    id: "response",
    title: "Frequency response by level",
    summary: "Frequency response at several input levels (fresh instance per level; channel 0)",
    description: "Measure the target's frequency response by log sweep (H1) at each input level, a fresh \
                  instance per level, and read the gain at octave frequencies; the spread across levels \
                  at each frequency shows how the response moves with level.",
    inputs: &[
        TARGET,
        RATE,
        BLOCK,
        SET,
        PRESET,
        Field::flag("levels", Kind::String, "Input levels, dBFS, comma-separated (each at or below 0)").default("-36,-24,-12,-6,0"),
    ],
    outputs: &[
        Field::flag("read_hz", Kind::Json, "The frequencies read").required(),
        Field::flag("levels_db", Kind::Json, "The input levels").required(),
        Field::flag("gain_db", Kind::Json, "Gain per level per frequency (null where undefined)").required(),
        Field::flag("spread_db", Kind::Json, "Spread across levels per frequency").required(),
    ],
    long_running: true,
};

pub fn response(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut cfg = LevelResponseConfig::default();
    if let Some(l) = input.opt_str("levels") {
        cfg.levels_db = l
            .split(',')
            .map(|v| v.trim().parse::<f64>().ok().filter(|l| *l <= 0.0))
            .collect::<Option<Vec<f64>>>()
            .ok_or_else(|| CommandError::Usage(format!("levels: '{l}' is not dBFS levels at or below 0")))?;
    }
    let (target, config) = factory(input, cx.workspace)?;
    let total = cfg.levels_db.len() as f64;
    let levels = cfg.levels_db.clone();
    let mut done = 0;
    let r = {
        let mut fresh = || {
            if let Some(l) = levels.get(done) {
                cx.progress(done as f64, Some(total), format!("sweeping at {l} dBFS"));
            }
            done += 1;
            reopen(&target, config)
        };
        measure_levels(&mut fresh, &cfg).map_err(|e| CommandError::from_error(&e))?
    };
    cx.progress(total, Some(total), "swept");
    Outcome::ok(&r, text::level_response(&r))
}
