//! stave-bench — the headless host: maximal machine-readable data from
//! any VST3 plugin. All the work lives in `stave_host::{bench, validate}`;
//! this binary parses arguments (`cli`), loads, and prints. See `--help` and
//! docs/host.md.

use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::process::ExitCode;

mod cli;

use cli::{Cli, Mode, USAGE};

use stave_host::bench::{self, text};
use stave_host::config::HostConfig;
use stave_host::instance::{apply_preset, Instance};
use stave_host::loaded::Loaded;
use stave_host::note::NoteSchedule;
use stave_host::render::Driver;
use stave_host::validate;
use stave_measure::signal::Silence;
use stave_host::wav;

fn main() -> ExitCode {
    // Plugins print to stdout (JUCE announces its version); stdout is
    // our JSON channel. Keep a private handle to it and point fd 1 at
    // stderr, so plugin chatter can never corrupt a report.
    let Some(mut out) = reserve_stdout() else {
        eprintln!("stave-bench: could not reserve stdout");
        return ExitCode::FAILURE;
    };
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(e) => {
            report(&e);
            eprintln!("\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&cli, &mut out) {
        Ok(code) => code,
        Err(e) => {
            report(e.as_ref());
            ExitCode::FAILURE
        }
    }
}

fn report(error: &dyn Error) {
    eprintln!("stave-bench: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}

fn run(cli: &Cli, out: &mut File) -> Result<ExitCode, Box<dyn Error>> {
    let config = HostConfig {
        sample_rate: cli.rate,
        max_block: cli.block,
        ..HostConfig::default()
    };
    let open = || Loaded::open(&cli.load, config);
    match cli.mode {
        Mode::Help => emit(out, USAGE)?,
        Mode::Validate => {
            let mut fresh = || open().map(Loaded::into_instance);
            let report = validate::Suite::default().run(&mut fresh, |r| {
                eprintln!(
                    "[validate] {:<18} {:?} ({:.0} ms)",
                    r.check, r.status, r.duration_ms
                )
            });
            emit(out, &json(&report)?)?;
            return Ok(if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        Mode::Response => {
            let mut fresh = || open().map(Loaded::into_instance);
            let r = stave_host::response::measure_levels(&mut fresh, &cli.response)?;
            if cli.json {
                emit(out, &json(&r)?)?;
            } else {
                emit(out, &text::level_response(&r))?;
            }
        }
        Mode::Describe | Mode::List => {
            let mut loaded = open()?;
            warm_up(loaded.instance());
            let d = bench::describe::describe_loaded(&loaded);
            if cli.mode == Mode::Describe {
                emit(out, &json(&d)?)?;
            } else {
                emit(out, &text::description(&d))?;
            }
        }
        Mode::Bench => {
            let mut loaded = open()?;
            let plugin = loaded.instance();
            warm_up(plugin);
            if let Some(name) = &cli.preset {
                apply_preset(plugin, name)?;
                eprintln!("preset '{name}' selected");
            }
            for (name, value) in &cli.sets {
                plugin
                    .set_control(name, *value)
                    .map_err(|e| format!("--set {name}: {e} (see --list)"))?;
            }
            let run = bench::run(plugin, &cli.bench)?;
            if let (Some(path), Some(channels)) = (&cli.out, run.capture) {
                let hz = cli.rate.hz();
                if hz.fract() != 0.0 || hz > f64::from(u32::MAX) {
                    return Err(
                        format!("a WAV header holds whole-number rates; {hz} Hz is not").into(),
                    );
                }
                let audio = wav::Audio {
                    sample_rate: hz as u32,
                    channels,
                };
                wav::write(path, &audio)?;
                eprintln!("wrote {}", path.display());
            }
            if cli.json {
                emit(out, &json(&run.report)?)?;
            } else {
                emit(
                    out,
                    &text::bench(&run.report, &cli.load.display().to_string()),
                )?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A few silent blocks: stave plugins register their surface on the
/// telemetry during their first process calls.
fn warm_up(plugin: &mut dyn Instance) {
    let mut d = Driver::new(plugin);
    let n = 4 * d.block() as u64;
    d.skip(n, &Silence, &NoteSchedule::default());
}

fn json(v: &impl serde::Serialize) -> serde_json::Result<String> {
    serde_json::to_string_pretty(v)
}

fn emit(out: &mut File, text: &str) -> std::io::Result<()> {
    let nl = if text.ends_with('\n') { "" } else { "\n" };
    write!(out, "{text}{nl}")?;
    out.flush()
}

/// Duplicate stdout for our own use, then redirect fd 1 to stderr.
fn reserve_stdout() -> Option<File> {
    extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(src: i32, dst: i32) -> i32;
    }
    std::io::stdout().flush().ok()?;
    // SAFETY: plain fd syscalls; the dup'd fd is owned by the File.
    unsafe {
        let ours = dup(1);
        if ours < 0 || dup2(2, 1) < 0 {
            return None;
        }
        Some(File::from_raw_fd(ours))
    }
}
