//! The datasheet's measurement and its data (DSP.md P1): `bench` and
//! `bench_points` time a stage and record a row; `finish` writes the rows
//! as `crates/stave-dsp/datasheet.json` (under `STAVE_DATASHEET_WRITE`,
//! which `scripts/datasheet.sh` sets on the reference machine) or holds
//! the committed file to the stages benched, so the data cannot drift
//! from the code.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// Samples per timed pass.
pub const N: usize = 1 << 17;
/// Timed passes per stage; the row is their median.
pub const REPEATS: usize = 5;
/// The rate every stage is prepared at.
pub const RATE: f32 = 48_000.0;
/// Samples a stage runs when it is only being named (debug builds).
const NAMING: usize = 256;

/// One measured row of the datasheet.
struct Row {
    stage: String,
    ns_per_sample: f64,
    spread_pct: f64,
}

thread_local! {
    /// True when timing (release); false names each stage only (debug).
    static TIMED: Cell<bool> = const { Cell::new(true) };
    static ROWS: RefCell<Vec<Row>> = const { RefCell::new(Vec::new()) };
}

/// Whether this run times stages. A debug build only names them: its
/// opt-level lies about release cost.
pub fn timed() -> bool {
    TIMED.with(Cell::get)
}

/// Start a datasheet run: timed in release, naming only in debug.
pub fn start() {
    let timed = !cfg!(debug_assertions);
    TIMED.with(|t| t.set(timed));
    if timed {
        println!("stave-dsp primitive datasheet ({N} samples, median of {REPEATS})");
    } else {
        eprintln!(
            "datasheet NOT TIMED (debug build — run under `cargo test --release`); \
             checking every stage is in datasheet.json"
        );
    }
}

/// A spread above this measures again (up to `ATTEMPTS` times in all),
/// keeping the steadiest attempt: a shared machine's bursts of other
/// work disturb whole passes, not single samples.
const STEADY_PCT: f64 = 10.0;
const ATTEMPTS: usize = 3;

/// Record a row from its timed passes (printed as it lands).
fn record(name: &str, mut run: impl FnMut() -> f64) {
    run(); // warm
    let (median, spread) = if timed() {
        let mut best = (0.0, f64::INFINITY);
        for _ in 0..ATTEMPTS {
            let mut samples: Vec<f64> = (0..REPEATS).map(|_| run()).collect();
            samples.sort_by(f64::total_cmp);
            let median = samples[REPEATS / 2];
            let spread = (samples[REPEATS - 1] - samples[0]) / median * 100.0;
            if spread < best.1 {
                best = (median, spread);
            }
            if spread <= STEADY_PCT {
                break;
            }
        }
        let (median, spread) = best;
        println!("  {name:<28} {median:>7.2} ns/sample  (spread {spread:.0}%)");
        best
    } else {
        (0.0, 0.0)
    };
    ROWS.with(|r| {
        r.borrow_mut().push(Row {
            stage: name.to_string(),
            ns_per_sample: median,
            spread_pct: spread,
        })
    });
}

/// Median ns/sample over `REPEATS` timed passes (one warm-up pass),
/// printed with the spread.
pub fn bench(name: &str, input: &[f32], mut tick: impl FnMut(f32) -> f32) {
    let input = if timed() { input } else { &input[..NAMING.min(input.len())] };
    record(name, || {
        let t0 = Instant::now();
        let mut acc = 0.0f32;
        for &x in input {
            acc += tick(black_box(x));
        }
        black_box(acc);
        t0.elapsed().as_secs_f64() / input.len() as f64 * 1e9
    });
}

/// Median ns per point of a whole-buffer operation over `REPEATS`
/// timed batches, for the transforms (cost per transform ÷ length).
pub fn bench_points(name: &str, points: usize, mut run_once: impl FnMut()) {
    let calls = if timed() { (N / points).max(8) } else { 1 };
    record(name, || {
        let t0 = Instant::now();
        for _ in 0..calls {
            run_once();
        }
        t0.elapsed().as_secs_f64() / (calls * points) as f64 * 1e9
    });
}

/// The committed data, beside this crate's manifest.
fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("datasheet.json")
}

/// A command's trimmed stdout, if it ran.
fn output_of(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// The CPU this ran on, as the OS names it.
fn cpu() -> String {
    let named = if cfg!(target_os = "macos") {
        output_of("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
    };
    named.unwrap_or_else(|| "unknown".into())
}

/// The 1-minute load average and the core count, as the OS reports them:
/// a busy machine's rows say so on the Cost page.
fn load() -> String {
    let avg = if cfg!(target_os = "macos") {
        // "{ 1.23 1.10 0.98 }"
        output_of("sysctl", &["-n", "vm.loadavg"]).and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
    } else {
        std::fs::read_to_string("/proc/loadavg").ok().and_then(|l| l.split_whitespace().next().map(str::to_string))
    };
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!("{} on {cores} cores", avg.unwrap_or_else(|| "unknown".into()))
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The rows as the committed JSON: the machine and toolchain, then one
/// entry per stage in datasheet order.
fn to_json(rows: &[Row]) -> String {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let unknown = || "unknown".to_string();
    let machine = env("STAVE_DATASHEET_MACHINE")
        .or_else(|| output_of("hostname", &[]))
        .unwrap_or_else(unknown);
    let mut s = String::from("{\n  \"schema\": 1,\n");
    let _ = writeln!(
        s,
        "  \"machine\": {{ \"name\": {}, \"cpu\": {}, \"os\": {}, \"arch\": {} }},",
        json_str(&machine),
        json_str(&cpu()),
        json_str(std::env::consts::OS),
        json_str(std::env::consts::ARCH)
    );
    for (key, value) in [
        ("commit", env("STAVE_DATASHEET_COMMIT").unwrap_or_else(unknown)),
        ("date", env("STAVE_DATASHEET_DATE").unwrap_or_else(unknown)),
        ("rustc", output_of("rustc", &["--version"]).unwrap_or_else(unknown)),
        ("load", load()),
    ] {
        let _ = writeln!(s, "  \"{key}\": {},", json_str(&value));
    }
    let _ = writeln!(s, "  \"samples\": {N},\n  \"repeats\": {REPEATS},\n  \"stages\": [");
    for (i, r) in rows.iter().enumerate() {
        let comma = if i + 1 < rows.len() { "," } else { "" };
        let _ = writeln!(
            s,
            "    {{ \"stage\": {}, \"ns_per_sample\": {:.3}, \"spread_pct\": {:.1}, \"rate_hz\": {} }}{comma}",
            json_str(&r.stage),
            r.ns_per_sample,
            r.spread_pct,
            RATE
        );
    }
    s.push_str("  ]\n}\n");
    s
}

/// The stage names in the committed data (every `"stage": "…"` value).
fn committed_stages(json: &str) -> Vec<String> {
    let key = "\"stage\": \"";
    let mut names = Vec::new();
    let mut rest = json;
    while let Some(at) = rest.find(key) {
        let mut chars = rest[at + key.len()..].chars();
        let mut name = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => name.extend(chars.next()),
                c => name.push(c),
            }
        }
        rest = chars.as_str();
        names.push(name);
    }
    names
}

/// End the run: write the data under `STAVE_DATASHEET_WRITE`, otherwise
/// fail unless the committed data names exactly the stages benched.
pub fn finish() {
    let rows = ROWS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let names: Vec<String> = rows.iter().map(|r| r.stage.clone()).collect();
    let mut seen = BTreeSet::new();
    for n in &names {
        assert!(seen.insert(n), "the datasheet benches \"{n}\" twice: each stage needs its own name");
    }
    if std::env::var_os("STAVE_DATASHEET_WRITE").is_some() {
        assert!(timed(), "STAVE_DATASHEET_WRITE needs a release build: run scripts/datasheet.sh");
        std::fs::write(data_path(), to_json(&rows)).expect("write datasheet.json");
        println!("wrote {}", data_path().display());
        return;
    }
    let json = std::fs::read_to_string(data_path()).expect("read crates/stave-dsp/datasheet.json");
    let committed = committed_stages(&json);
    let missing: Vec<&String> = names.iter().filter(|n| !committed.contains(n)).collect();
    let stale: Vec<&String> = committed.iter().filter(|n| !names.contains(n)).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
"\n\ncrates/stave-dsp/datasheet.json is out of step with tests/datasheet.rs.\n  \
         benched but not in the data: {missing:?}\n  \
         in the data but no longer benched: {stale:?}\n\
         Every bench(..) line needs its row (the docs' Cost page is generated from the file).\n\
         Fix: run `scripts/datasheet.sh` (it measures on the reference machine, stave-mini,\n\
         and writes the file back), then commit crates/stave-dsp/datasheet.json with the change.\n"
    );
}
