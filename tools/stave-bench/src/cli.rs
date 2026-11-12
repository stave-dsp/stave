//! `stave-bench` arguments → a [`Cli`]. Pure: every malformed value is
//! an error naming the flag, never a silent default.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use stave_measure::loudness;

use stave_host::bench::{BenchConfig, Stimulus, StimulusError};
use stave_host::config::{HostConfig, SampleRate};
use stave_host::note::Note;
use stave_host::response::LevelResponseConfig;

pub const USAGE: &str = "\
usage: stave-bench <bundle.vst3 | stage:NAME> [options]

stage:NAME runs a stave-dsp stage from the catalog (e.g. stage:coupler);
its params are a plugin's, so --set, --list and --validate work unchanged.

modes (default: bench):
  --describe            identity, load trace, params, presets (JSON)
  --list                the same, as text
  --validate            run the validation suite (JSON; exit 1 on failure)
  --response            frequency response at several input levels
                        (fresh instance per level; channel 0)

options:
  --rate HZ             sample rate (48000)
  --block N             max block size (256)
  --seconds S           timed-run length (3)
  --signal SPEC         sine[:FREQ[:DBFS]] | impulse | silence | noise[:DBFS]
                        | sweep[:F0:F1[:DBFS]] | wav:PATH   (sine:1000:-12)
  --notes KEY@ON:OFF[:V],...
                        notes, seconds; velocity 0..1, default MIDI 100
                        (e.g. 60@0:1.5,64@0.5:1.5:0.4)
  --set NAME=VALUE      set a param (in its own units; repeatable)
  --preset NAME         apply a factory preset first
  --measure LIST        level measures, comma-separated
                        (peak,true-peak,rms,lufs-m,lufs-s,lufs-i;
                        default peak,true-peak,rms,lufs-i)
  --analysis-window N   tone-analysis window in frames (32768)
  --response-levels L   input levels for --response, dBFS, comma-separated
                        (-36,-24,-12,-6,0)
  --out PATH            write the timed run's output as 32-bit float WAV
  --json                bench report as JSON
  -h, --help            this text

env: STAVE_HOST_TRACE=1 streams the plugin load trace to stderr";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Bench,
    Describe,
    List,
    Validate,
    Response,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cli {
    pub load: PathBuf,
    pub mode: Mode,
    pub rate: SampleRate,
    pub block: NonZeroUsize,
    pub bench: BenchConfig,
    pub sets: Vec<(String, f64)>,
    pub response: LevelResponseConfig,
    pub preset: Option<String>,
    pub out: Option<PathBuf>,
    pub json: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{flag} needs a value")]
    MissingValue { flag: &'static str },
    #[error("{flag}: '{value}' is not {expected}")]
    InvalidValue {
        flag: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("{first} and {second} are exclusive")]
    ExclusiveModes {
        first: &'static str,
        second: &'static str,
    },
    #[error("unknown option '{0}'")]
    UnknownOption(String),
    #[error("unexpected argument '{0}' (already given what to load)")]
    UnexpectedArgument(String),
    #[error("missing <bundle.vst3 | stage:NAME>")]
    MissingLoad,
    #[error("--set wants NAME=VALUE, got '{0}'")]
    MalformedSet(String),
    #[error("--notes: '{note}' {problem}")]
    MalformedNote { note: String, problem: &'static str },
    #[error("invalid --measure")]
    Measures(#[from] loudness::UnknownMeasures),
    #[error("invalid --signal")]
    Stimulus(#[from] StimulusError),
}

impl Mode {
    fn flag(self) -> &'static str {
        match self {
            Self::Bench => "the bench",
            Self::Describe => "--describe",
            Self::List => "--list",
            Self::Validate => "--validate",
            Self::Response => "--response",
            Self::Help => "--help",
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, CliError> {
    let mut it = args.into_iter();
    let mut cli = Cli {
        load: PathBuf::new(),
        mode: Mode::Bench,
        rate: HostConfig::default().sample_rate,
        block: HostConfig::default().max_block,
        bench: BenchConfig::default(),
        sets: Vec::new(),
        response: LevelResponseConfig::default(),
        preset: None,
        out: None,
        json: false,
    };
    let mode = |m: Mode, cli: &mut Cli| {
        if cli.mode != Mode::Bench && cli.mode != m {
            return Err(CliError::ExclusiveModes {
                first: cli.mode.flag(),
                second: m.flag(),
            });
        }
        cli.mode = m;
        Ok(())
    };
    while let Some(arg) = it.next() {
        let mut value = |flag: &'static str| it.next().ok_or(CliError::MissingValue { flag });
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(Cli {
                    mode: Mode::Help,
                    ..cli
                })
            }
            "--describe" => mode(Mode::Describe, &mut cli)?,
            "--list" => mode(Mode::List, &mut cli)?,
            "--validate" => mode(Mode::Validate, &mut cli)?,
            "--response" => mode(Mode::Response, &mut cli)?,
            "--response-levels" => {
                cli.response.levels_db = parse_levels(&value("--response-levels")?)?
            }
            "--json" => cli.json = true,
            "--rate" => {
                let hz = number(&value("--rate")?, "--rate", POSITIVE, |_: f64| true)?;
                cli.rate = SampleRate::new(hz).map_err(|_| CliError::InvalidValue {
                    flag: "--rate",
                    value: hz.to_string(),
                    expected: POSITIVE,
                })?
            }
            "--block" => cli.block = number(&value("--block")?, "--block", AT_LEAST_ONE, |_| true)?,
            "--seconds" => {
                cli.bench.seconds =
                    number(&value("--seconds")?, "--seconds", POSITIVE, |v| v > 0.0)?
            }
            "--signal" => cli.bench.stimulus = Stimulus::parse(&value("--signal")?)?,
            "--measure" => cli.bench.measures = parse_measures(&value("--measure")?)?,
            "--analysis-window" => {
                cli.bench.analysis_window = number(
                    &value("--analysis-window")?,
                    "--analysis-window",
                    "a whole number of at least 64 frames",
                    |v: usize| v >= 64,
                )?
            }
            "--notes" => cli.bench.notes.extend(parse_notes(&value("--notes")?)?),
            "--set" => cli.sets.push(parse_set(&value("--set")?)?),
            "--preset" => cli.preset = Some(value("--preset")?),
            "--out" => {
                cli.out = Some(PathBuf::from(value("--out")?));
                cli.bench.capture = true;
            }
            s if s.starts_with('-') => return Err(CliError::UnknownOption(arg)),
            _ if cli.load.as_os_str().is_empty() => cli.load = PathBuf::from(arg),
            _ => return Err(CliError::UnexpectedArgument(arg)),
        }
    }
    if cli.load.as_os_str().is_empty() {
        return Err(CliError::MissingLoad);
    }
    Ok(cli)
}

const POSITIVE: &str = "a positive number";
const AT_LEAST_ONE: &str = "a whole number of at least 1";

fn number<T: std::str::FromStr + Copy>(
    s: &str,
    flag: &'static str,
    expected: &'static str,
    valid: impl Fn(T) -> bool,
) -> Result<T, CliError> {
    s.parse()
        .ok()
        .filter(|&v| valid(v))
        .ok_or_else(|| CliError::InvalidValue {
            flag,
            value: s.to_string(),
            expected,
        })
}

/// A comma-separated list of level-measure names.
fn parse_measures(list: &str) -> Result<Vec<String>, CliError> {
    let names: Vec<String> = list.split(',').map(|n| n.trim().to_string()).collect();
    loudness::by_names(&names)?;
    Ok(names)
}

fn parse_levels(list: &str) -> Result<Vec<f64>, CliError> {
    list.split(',')
        .map(|v| {
            number(
                v.trim(),
                "--response-levels",
                "a level at or below 0 dBFS",
                |l: f64| l <= 0.0,
            )
        })
        .collect()
}

/// `NAME=VALUE`, plain units.
fn parse_set(s: &str) -> Result<(String, f64), CliError> {
    let (name, value) = s
        .split_once('=')
        .ok_or_else(|| CliError::MalformedSet(s.to_string()))?;
    let value = number(value.trim(), "--set", "a number", |_: f64| true)?;
    Ok((name.trim().to_string(), value))
}

/// `KEY@ON:OFF[:VELOCITY][,…]`, seconds; velocity 0..=1, default MIDI 100.
fn parse_notes(s: &str) -> Result<Vec<Note>, CliError> {
    s.split(',').map(parse_note).collect()
}

fn parse_note(part: &str) -> Result<Note, CliError> {
    let malformed = |problem| CliError::MalformedNote {
        note: part.to_string(),
        problem,
    };
    let shape = || malformed("is not KEY@ON:OFF[:VELOCITY]");
    let (key, times) = part.split_once('@').ok_or_else(shape)?;
    let mut fields = times.split(':').map(str::trim);
    let (Some(on), Some(off)) = (fields.next(), fields.next()) else {
        return Err(shape());
    };
    let velocity = match fields.next() {
        None => Note::DEFAULT_VELOCITY,
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or_else(|| malformed("has a velocity outside 0..=1"))?,
    };
    if fields.next().is_some() {
        return Err(shape());
    }
    let note = Note {
        key: key.trim().parse().map_err(|_| shape())?,
        velocity,
        on_s: on.parse().map_err(|_| shape())?,
        off_s: off.parse().map_err(|_| shape())?,
    };
    if note.off_s < note.on_s {
        return Err(malformed("ends before it starts"));
    }
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &str) -> Result<Cli, CliError> {
        parse(args.split_whitespace().map(String::from))
    }

    #[test]
    fn defaults() {
        let c = p("X.vst3").unwrap();
        assert_eq!(
            (c.mode, c.rate.hz(), c.block.get(), c.json),
            (Mode::Bench, 48_000.0, 256, false)
        );
        assert_eq!(c.bench, BenchConfig::default());
    }

    #[test]
    fn everything_at_once() {
        let c = p(
            "--rate 96000 X.vst3 --block 64 --seconds 1.5 --signal sweep:20:2000 \
                   --set gain=-3 --set Delay·mix=0.5 --preset Warm --out o.wav --json \
                   --notes 60@0:1,64@0.5:1",
        )
        .unwrap();
        assert_eq!(
            (c.rate.hz(), c.block.get(), c.bench.seconds),
            (96_000.0, 64, 1.5)
        );
        assert_eq!(c.sets, [("gain".into(), -3.0), ("Delay·mix".into(), 0.5)]);
        assert_eq!(c.preset.as_deref(), Some("Warm"));
        assert!(c.bench.capture && c.json);
        assert_eq!(c.bench.notes.len(), 2);
        assert_eq!(
            c.bench.notes[1],
            Note {
                key: 64,
                velocity: Note::DEFAULT_VELOCITY,
                on_s: 0.5,
                off_s: 1.0
            }
        );
    }

    #[test]
    fn response_mode_takes_its_levels() {
        let c = p("stage:coupler --response --response-levels -30,-6").unwrap();
        assert_eq!(c.mode, Mode::Response);
        assert_eq!(c.response.levels_db, [-30.0, -6.0]);
        assert!(p("X.vst3 --response-levels 3").is_err());
    }

    #[test]
    fn measures_and_window_are_configurable() {
        let c = p("X --measure lufs-i,true-peak --analysis-window 4096").unwrap();
        assert_eq!(c.bench.measures, ["lufs-i", "true-peak"]);
        assert_eq!(c.bench.analysis_window, 4096);
        let Err(CliError::Measures(e)) = p("X --measure peak,vu") else {
            panic!("an unknown measure parsed")
        };
        assert_eq!(e.unknown, ["vu"]);
    }

    #[test]
    fn modes_are_exclusive_but_repeatable() {
        assert_eq!(p("X --describe --describe").unwrap().mode, Mode::Describe);
        assert!(matches!(
            p("X --describe --validate"),
            Err(CliError::ExclusiveModes {
                first: "--describe",
                second: "--validate"
            })
        ));
        assert_eq!(
            p("--help").unwrap().mode,
            Mode::Help,
            "help needs no bundle"
        );
    }

    #[test]
    fn notes_take_an_optional_velocity() {
        let c = p("X --notes 60@0:1,64@0.5:1:0.25").unwrap();
        assert_eq!(c.bench.notes[0].velocity, Note::DEFAULT_VELOCITY);
        assert_eq!(c.bench.notes[1].velocity, 0.25);
        assert!(matches!(
            p("X --notes 60@0:1:2"),
            Err(CliError::MalformedNote { .. })
        ));
    }

    #[test]
    fn malformed_values_name_their_flag() {
        for (args, flag) in [
            ("X --rate fast", "--rate"),
            ("X --rate -1", "--rate"),
            ("X --block 0", "--block"),
            ("X --set gain", "--set"),
            ("X --set gain=loud", "--set"),
            ("X --notes 60@1", "--notes"),
            ("X --notes 60@2:1", "--notes"),
            ("X --seconds", "--seconds"),
        ] {
            let e = p(args).unwrap_err().to_string();
            assert!(e.contains(flag), "{args}: {e}");
        }
    }

    #[test]
    fn stray_arguments_are_refused() {
        assert!(matches!(p(""), Err(CliError::MissingLoad)));
        assert!(matches!(p("A B"), Err(CliError::UnexpectedArgument(a)) if a == "B"));
        assert!(
            matches!(p("A --frobnicate"), Err(CliError::UnknownOption(o)) if o == "--frobnicate")
        );
        assert!(matches!(
            p("A --signal pink"),
            Err(CliError::Stimulus(StimulusError::UnknownKind { .. }))
        ));
    }
}
