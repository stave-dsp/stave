//! stave-play: the windowed host — play a plugin, hear it, watch it.
//! All hosting and measurement lives in the `stave_host` library; this
//! binary is the GUI (eframe) around it.

mod analysis;
mod app;
mod audio;
mod debug_log;
#[cfg_attr(not(target_os = "macos"), path = "editor_none.rs")]
mod editor_view;
mod errors;
mod project;
mod samples;
mod synth;
mod views;
mod voicing;

use std::path::PathBuf;

use stave_host::config::{HostConfig, SampleRate};

const USAGE: &str = "stave-play [PLUGIN.vst3] [--sample-rate HZ] [--block-size N] \
                     [--set NAME=VALUE ...] \
                     [--screenshot PATH [--note PITCH] [--editor-only]] \
                     [--face-trace PATH]
       stave-play --compare CANDIDATE --compare CANDIDATE ... [--material MATERIAL] \
                     [--trim DB] [--abx-trials N] [--voicing-dir DIR]
       stave-play --verify-blessed RECORD_OR_DIR [--tolerance-db DB]
       stave-play --list-material [--export-material DIR]

A CANDIDATE is dry, PLUGIN.vst3, stage:NAME, wav:FILE or blessed:RECORD,
then ;name=value settings, ;preset=NAME, ;label=TEXT or ;ref (quote it).
MATERIAL is a built-in name (see --list-material) or a WAV path.";

fn main() -> std::process::ExitCode {
    let launch = match parse(std::env::args().skip(1)) {
        Ok(Some(l)) => l,
        Ok(None) => {
            println!("{USAGE}");
            return std::process::ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("stave-play: {e}\n{USAGE}");
            return std::process::ExitCode::from(2);
        }
    };
    if launch.list_material {
        for b in voicing::BUILTINS {
            println!("{:<14} {}", b.name, b.about);
        }
    }
    if let Some(dir) = &launch.export_material {
        match voicing::export_builtins(dir, launch.app.sample_rate.hz()) {
            Ok(paths) => paths.iter().for_each(|p| println!("{}", p.display())),
            Err(e) => {
                eprintln!("stave-play: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    if launch.list_material || launch.export_material.is_some() {
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(path) = &launch.verify_blessed {
        return verify_blessed(path, launch.tolerance_db);
    }
    let voicing = match voicing::Voicing::new(&launch.app.voicing) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("stave-play: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let launch = launch.app;
    if let (Some(path), app::CaptureRegion::Editor) = (&launch.screenshot, launch.screenshot_region) {
        // A stave plugin's face writes its own picture there, offscreen,
        // once it has run a second (no screen capture, so no other
        // window can show in it); other editors fall back to the screen.
        let _ = std::fs::remove_file(path);
        std::env::set_var("STAVE_FACE_SNAPSHOT", path);
    }
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title("stave-play")
        .with_inner_size([1560.0, 920.0])
        .with_min_inner_size([720.0, 420.0]);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "stave-play",
        options,
        Box::new(move |cc| Box::new(app::HostApp::new(cc, launch, voicing))),
    );
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stave-play: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Re-run every blessed candidate under `path` with this build and null
/// it against its blessed take; fails when any residual is above
/// `tolerance_db` (relative to the take).
fn verify_blessed(path: &std::path::Path, tolerance_db: f64) -> std::process::ExitCode {
    let records = voicing::bless_records(path);
    if records.is_empty() {
        eprintln!("stave-play: no bless records at {}", path.display());
        return std::process::ExitCode::from(2);
    }
    let mut failed = false;
    for r in records {
        match voicing::verify_blessed(&r) {
            Ok(v) => {
                let moved = v.fingerprint_moved.as_ref().map_or(0, Vec::len);
                let ok = v.residual_db <= tolerance_db && moved == 0;
                failed |= !ok;
                let mut what = if v.bit_identical {
                    "bit-identical".to_string()
                } else {
                    format!("residual {:.1} dB", v.residual_db)
                };
                match &v.fingerprint_moved {
                    Some(m) if m.is_empty() => what.push_str(", fingerprint holds"),
                    Some(m) => what.push_str(&format!(", fingerprint moved in {} characteristics", m.len())),
                    None => {}
                }
                println!("{}  {}  {what}  ({})", if ok { "same   " } else { "CHANGED" }, v.label, v.record.display());
                for line in v.fingerprint_moved.iter().flatten() {
                    println!("         {line}");
                }
            }
            Err(e) => {
                failed = true;
                println!("ERROR    {}: {e}", r.display());
            }
        }
    }
    if failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// What the command line asks for: the app, or a headless voicing task.
struct Cli {
    app: app::Launch,
    verify_blessed: Option<PathBuf>,
    /// A blessed take counts as changed above this residual (dB re the
    /// take): numerical noise across machines passes, a real change fails.
    tolerance_db: f64,
    list_material: bool,
    export_material: Option<PathBuf>,
}

#[derive(Debug, PartialEq, thiserror::Error)]
enum ArgError {
    #[error("{0} needs a value")]
    MissingValue(&'static str),
    #[error("{flag}: invalid value '{value}'")]
    InvalidValue { flag: &'static str, value: String },
    #[error("unknown option '{0}'")]
    UnknownOption(String),
    #[error("unexpected argument '{0}'")]
    UnexpectedArgument(String),
}

/// `Ok(None)` for --help.
fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Cli>, ArgError> {
    let mut l = app::Launch {
        plugin: None,
        sample_rate: HostConfig::default().sample_rate,
        block_size: project::DEFAULT_BLOCK,
        sets: Vec::new(),
        screenshot: None,
        screenshot_note: None,
        screenshot_region: app::CaptureRegion::Window,
        voicing: voicing::Launch::default(),
    };
    let (mut verify_blessed, mut tolerance_db, mut list_material) = (None, -120.0, false);
    let mut export_material = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |flag| it.next().ok_or(ArgError::MissingValue(flag));
        let bad = |flag, v: &str| ArgError::InvalidValue {
            flag,
            value: v.to_string(),
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-s" | "--sample-rate" => {
                let v = value("--sample-rate")?;
                l.sample_rate = v
                    .parse::<f64>()
                    .ok()
                    .and_then(|hz| SampleRate::new(hz).ok())
                    .ok_or_else(|| bad("--sample-rate", &v))?;
            }
            "-b" | "--block-size" => {
                let v = value("--block-size")?;
                l.block_size = v.parse().map_err(|_| bad("--block-size", &v))?;
            }
            "--screenshot" => l.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--editor-only" => l.screenshot_region = app::CaptureRegion::Editor,
            "--note" => {
                let v = value("--note")?;
                l.screenshot_note = Some(v.parse().map_err(|_| bad("--note", &v))?);
            }
            "--face-trace" => {
                // A stave plugin's face (in this process) appends every
                // pointer and key event to PATH: a session that replays
                // as a test (`stave_face::Trace`, `FaceDriver::replay`).
                std::env::set_var("STAVE_FACE_TRACE", value("--face-trace")?);
            }
            "--set" => {
                let v = value("--set")?;
                let (k, x) = v.split_once('=').ok_or_else(|| bad("--set", &v))?;
                l.sets
                    .push((k.to_string(), x.parse().map_err(|_| bad("--set", &v))?));
            }
            "--compare" => {
                let v = value("--compare")?;
                voicing::Candidate::parse(&v).map_err(|_| bad("--compare", &v))?;
                l.voicing.candidates.push(v);
            }
            "--material" => l.voicing.material = Some(value("--material")?),
            "--trim" => {
                let v = value("--trim")?;
                l.voicing.trim_db = Some(v.parse().map_err(|_| bad("--trim", &v))?);
            }
            "--abx-trials" => {
                let v = value("--abx-trials")?;
                let n: u32 = v.parse().map_err(|_| bad("--abx-trials", &v))?;
                if n == 0 {
                    return Err(bad("--abx-trials", &v));
                }
                l.voicing.abx_trials = Some(n);
            }
            "--voicing-dir" => l.voicing.dir = Some(PathBuf::from(value("--voicing-dir")?)),
            "--verify-blessed" => verify_blessed = Some(PathBuf::from(value("--verify-blessed")?)),
            "--tolerance-db" => {
                let v = value("--tolerance-db")?;
                tolerance_db = v.parse().map_err(|_| bad("--tolerance-db", &v))?;
            }
            "--list-material" => list_material = true,
            "--export-material" => export_material = Some(PathBuf::from(value("--export-material")?)),
            s if s.starts_with('-') => return Err(ArgError::UnknownOption(arg)),
            _ if l.plugin.is_none() => l.plugin = Some(PathBuf::from(arg)),
            _ => return Err(ArgError::UnexpectedArgument(arg)),
        }
    }
    Ok(Some(Cli {
        app: l,
        verify_blessed,
        tolerance_db,
        list_material,
        export_material,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Option<app::Launch>, ArgError> {
        parse(s.split_whitespace().map(String::from)).map(|c| c.map(|c| c.app))
    }

    #[test]
    fn editor_only_narrows_the_screenshot_to_the_editor() {
        let l = p("X.vst3 --screenshot o.png --editor-only")
            .unwrap()
            .unwrap();
        assert_eq!(l.screenshot_region, app::CaptureRegion::Editor);
        let l = p("X.vst3 --screenshot o.png").unwrap().unwrap();
        assert_eq!(l.screenshot_region, app::CaptureRegion::Window);
    }

    #[test]
    fn parses_launch_options() {
        let l = p("X.vst3 -s 96000 --block-size 256 --set mix=0.5 --screenshot o.png --note 60")
            .unwrap()
            .unwrap();
        assert_eq!(
            (l.sample_rate.hz(), l.block_size.get(), l.screenshot_note),
            (96_000.0, 256, Some(60))
        );
        assert_eq!(l.sets, [("mix".to_string(), 0.5)]);
        assert_eq!(l.plugin, Some(PathBuf::from("X.vst3")));
    }

    #[test]
    fn rejects_garbage() {
        let invalid = |flag, value: &str| {
            Some(ArgError::InvalidValue {
                flag,
                value: value.into(),
            })
        };
        assert_eq!(
            p("--block-size zero").err(),
            invalid("--block-size", "zero")
        );
        assert_eq!(p("--block-size 0").err(), invalid("--block-size", "0"));
        assert_eq!(p("--set mix").err(), invalid("--set", "mix"));
        assert_eq!(p("--note").err(), Some(ArgError::MissingValue("--note")));
        assert_eq!(
            p("--bogus").err(),
            Some(ArgError::UnknownOption("--bogus".into()))
        );
        assert_eq!(
            p("A B").err(),
            Some(ArgError::UnexpectedArgument("B".into()))
        );
        assert!(p("--help").unwrap().is_none());
        assert_eq!(p("--abx-trials 0").err(), invalid("--abx-trials", "0"));
        assert_eq!(p("--compare x.vst3;drive").err(), invalid("--compare", "x.vst3;drive"));
    }

    #[test]
    fn parses_voicing_options() {
        let args = ["--compare", "dry;ref", "--compare", "stage:clipper/diode;drive=0.6", "--material",
            "drum loop", "--trim", "-6", "--abx-trials", "60", "--voicing-dir", "/tmp/v"];
        let c = parse(args.into_iter().map(String::from)).unwrap().unwrap();
        let v = &c.app.voicing;
        assert_eq!(v.candidates, ["dry;ref", "stage:clipper/diode;drive=0.6"]);
        assert_eq!(v.material.as_deref(), Some("drum loop"));
        assert_eq!((v.trim_db, v.abx_trials), (Some(-6.0), Some(60)));
        assert_eq!(v.dir, Some(PathBuf::from("/tmp/v")));
        assert!(v.wants_voicing());
        let c = parse(["--verify-blessed", "b", "--tolerance-db", "-90"].into_iter().map(String::from))
            .unwrap()
            .unwrap();
        assert_eq!((c.verify_blessed, c.tolerance_db), (Some(PathBuf::from("b")), -90.0));
    }
}
