//! stave-play: the windowed host — play a plugin, hear it, watch it.
//! All hosting and measurement lives in the `stave_host` library; this
//! binary is the GUI (eframe) around it.

mod analysis;
mod app;
mod audio;
mod debug_log;
mod editor_view;
mod errors;
mod project;
mod samples;
mod synth;
mod views;

use std::path::PathBuf;

use stave_host::config::{HostConfig, SampleRate};

const USAGE: &str = "stave-play [PLUGIN.vst3] [--sample-rate HZ] [--block-size N] \
                     [--set NAME=VALUE ...] \
                     [--screenshot PATH [--note PITCH] [--editor-only]] \
                     [--face-trace PATH]";

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
        Box::new(move |cc| Box::new(app::HostApp::new(cc, launch))),
    );
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stave-play: {e}");
            std::process::ExitCode::FAILURE
        }
    }
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
fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<app::Launch>, ArgError> {
    let mut l = app::Launch {
        plugin: None,
        sample_rate: HostConfig::default().sample_rate,
        block_size: project::DEFAULT_BLOCK,
        sets: Vec::new(),
        screenshot: None,
        screenshot_note: None,
        screenshot_region: app::CaptureRegion::Window,
    };
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
            s if s.starts_with('-') => return Err(ArgError::UnknownOption(arg)),
            _ if l.plugin.is_none() => l.plugin = Some(PathBuf::from(arg)),
            _ => return Err(ArgError::UnexpectedArgument(arg)),
        }
    }
    Ok(Some(l))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Option<app::Launch>, ArgError> {
        parse(s.split_whitespace().map(String::from))
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
    }
}
