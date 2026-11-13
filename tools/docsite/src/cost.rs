//! The Cost reference page (DSP.md P1): what each stave-dsp stage costs,
//! from the two committed measurements, never from the machine building
//! the site (a shared CI runner's clock is noise, and not a machine
//! anyone ships on):
//!
//! * `crates/stave-dsp/datasheet.json` — ns/sample on the reference
//!   machine it names, written by `scripts/datasheet.sh`; the datasheet
//!   test fails when its stages and the file's differ;
//! * `crates/stave-dsp/benches/stages.icount` — instructions/sample, the
//!   baseline CI's `perf` workflow holds every change to (P2).
//!
//! The build fails on a file that is missing, malformed, unmeasured (a
//! zero time, an unnamed machine) or out of step with the stage harness.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use serde_json::Value;

use crate::html::esc;
use crate::reference::{src_link, Ctx, Generated};
use crate::Result;

const DATASHEET: &str = "crates/stave-dsp/datasheet.json";
const ICOUNT: &str = "crates/stave-dsp/benches/stages.icount";
const HARNESS: &str = "crates/stave-dsp/benches/stages.rs";
/// One sample at 48 kHz, in ns: the realtime budget a row is priced in.
const BUDGET_NS: f64 = 1e9 / 48_000.0;
/// A spread above this says the run was disturbed.
const NOISY_PCT: f64 = 25.0;

struct Row {
    stage: String,
    ns: f64,
    spread: f64,
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty() && *s != "unknown").ok_or_else(|| format!("{DATASHEET}: no \"{key}\" (refresh it with scripts/datasheet.sh)").into())
}

fn datasheet(cx: &Ctx<'_>) -> Result<(Value, Vec<Row>)> {
    let json: Value = serde_json::from_str(&fs::read_to_string(cx.repo.join(DATASHEET))?).map_err(|e| format!("{DATASHEET}: {e}"))?;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for s in json.get("stages").and_then(Value::as_array).ok_or(format!("{DATASHEET}: no stages"))? {
        let stage = text(s, "stage")?.to_string();
        let ns = s.get("ns_per_sample").and_then(Value::as_f64).unwrap_or(0.0);
        let spread = s.get("spread_pct").and_then(Value::as_f64).unwrap_or(0.0);
        if ns <= 0.0 {
            return Err(format!("{DATASHEET}: {stage} has no time — the data was written by an unmeasured run (scripts/datasheet.sh runs it in release)").into());
        }
        if !seen.insert(stage.clone()) {
            return Err(format!("{DATASHEET}: {stage} appears twice").into());
        }
        rows.push(Row { stage, ns, spread });
    }
    if rows.is_empty() {
        return Err(format!("{DATASHEET}: no stages").into());
    }
    Ok((json, rows))
}

/// The baseline's rows, held to the harness's stage ids (read from its
/// source: every `id: "…"`).
fn icount(cx: &Ctx<'_>) -> Result<(String, Vec<(String, f64)>)> {
    let src = fs::read_to_string(cx.repo.join(ICOUNT))?;
    let env = src.lines().filter_map(|l| l.strip_prefix("# valgrind")).map(|l| format!("valgrind{l}")).next().unwrap_or_default();
    let mut rows = Vec::new();
    for line in src.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (id, n) = line.split_once('\t').ok_or_else(|| format!("{ICOUNT}: malformed line {line:?}"))?;
        let n: f64 = n.trim().parse().map_err(|_| format!("{ICOUNT}: {id}: {n:?} is not a count"))?;
        rows.push((id.to_string(), n));
    }
    let harness = fs::read_to_string(cx.repo.join(HARNESS))?;
    let ids: BTreeSet<&str> = harness.lines().filter_map(|l| l.trim().strip_prefix("id: \"")).filter_map(|l| l.split('"').next()).collect();
    let counted: BTreeSet<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    if ids != counted || ids.is_empty() {
        return Err(format!("{ICOUNT} counts {counted:?} but {HARNESS} has {ids:?}: re-bless it (scripts/icount.sh bless)").into());
    }
    Ok((env, rows))
}

pub fn page(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/cost/index.html";
    let (json, rows) = datasheet(cx)?;
    let machine = json.get("machine").ok_or(format!("{DATASHEET}: no machine"))?;
    let (name, cpu) = (text(machine, "name")?, text(machine, "cpu")?);
    let (os, arch) = (text(machine, "os")?, text(machine, "arch")?);
    let (commit, date, rustc) = (text(&json, "commit")?, text(&json, "date")?, text(&json, "rustc")?);
    let samples = json.get("samples").and_then(Value::as_u64).unwrap_or(0);
    let repeats = json.get("repeats").and_then(Value::as_u64).unwrap_or(0);

    let mut body = format!(
        "<p>What each stave-dsp stage costs per sample, so a design can be priced before it is written (can it afford four of these per voice?). The numbers are measured on one stated reference machine and committed, never measured while this site builds: a shared CI runner's clock is noise and no one ships on it. The datasheet test fails whenever the stages it benches and the committed data differ, so a stage cannot be added without its row.</p>\
         <table class=\"kv\"><tbody>\
         <tr><td>Machine</td><td><strong>{name}</strong> — {cpu}, {os} {arch}</td></tr>\
         <tr><td>Measured</td><td>{date}, commit <code>{commit}</code>, {rustc}</td></tr>\
         <tr><td>Load</td><td>{load} when the run started (a busy machine widens the spreads)</td></tr>\
         <tr><td>Method</td><td>median of {repeats} passes of {samples} samples of noise after a warm-up, at 48 kHz, release profile; spread = (slowest − fastest) ÷ median, the steadiest of up to three attempts when it exceeds 10%</td></tr>\
         <tr><td>Refresh</td><td><code>scripts/datasheet.sh</code> (runs {} on the reference machine)</td></tr>\
         </tbody></table>",
        src_link("crates/stave-dsp/tests/datasheet.rs"),
        name = esc(name),
        cpu = esc(cpu),
        os = esc(os),
        arch = esc(arch),
        date = esc(date),
        commit = esc(commit),
        rustc = esc(rustc),
        load = esc(json.get("load").and_then(Value::as_str).unwrap_or("not recorded")),
    );
    let _ = write!(
        body,
        "<h2 id=\"datasheet\">ns per sample on {}</h2><p>“Of 48 kHz” is the share of one core a single instance takes in realtime at 48 kHz (one sample is {BUDGET_NS:.0} ns); “per core” is how many instances would fill it. A spread over {NOISY_PCT:.0}% marks a disturbed measurement: read that row as approximate until the next refresh.</p>\
         <table class=\"cost\"><thead><tr><th>Stage</th><th>ns/sample</th><th>spread</th><th>of 48 kHz</th><th>per core</th></tr></thead><tbody>",
        esc(name)
    );
    for r in &rows {
        let noisy = if r.spread > NOISY_PCT { " <span class=\"meta\">noisy</span>" } else { "" };
        let _ = write!(
            body,
            "<tr><td>{}</td><td><code>{:.2}</code></td><td><code>{:.0}%</code>{noisy}</td><td><code>{:.3}%</code></td><td><code>{:.0}×</code></td></tr>",
            esc(&r.stage),
            r.ns,
            r.spread,
            r.ns / BUDGET_NS * 100.0,
            BUDGET_NS / r.ns
        );
    }
    body.push_str("</tbody></table>");

    let (env, counts) = icount(cx)?;
    let _ = write!(
        body,
        "<h2 id=\"instructions\">Instructions per sample (CI)</h2><p>The deterministic measure: instructions executed per sample by each stage of the harness {}, counted under valgrind's cachegrind on x86_64 Linux as the difference of two run lengths (so set-up cancels). CI's <code>perf</code> workflow counts them on every change to stave-dsp or stave-core and fails when a stage grows more than 5% over this committed baseline; <code>scripts/icount.sh bless</code> moves it on purpose. Counted with {}.</p>\
         <table class=\"cost\"><thead><tr><th>Stage</th><th>instructions/sample</th></tr></thead><tbody>",
        src_link(HARNESS),
        esc(&env)
    );
    for (id, n) in &counts {
        let _ = write!(body, "<tr><td><code>{}</code></td><td><code>{n:.2}</code></td></tr>", esc(id));
    }
    body.push_str("</tbody></table>");

    Ok(Generated {
        path: PATH,
        title: "Cost",
        description: format!("ns/sample of all {} datasheet stages on {name} ({cpu}), and the instruction-count baseline CI holds stave-dsp to.", rows.len()),
        from: format!("Generated from {} and {}", src_link(DATASHEET), src_link(ICOUNT)),
        body,
        wide: false,
    })
}
