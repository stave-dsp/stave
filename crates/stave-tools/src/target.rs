//! What a command opens. A target is one string:
//!
//! ```text
//! path/to/Plugin.vst3                     a VST3 bundle
//! au:NAME   or a .component path          an Audio Unit (macOS)
//! stage:clipper/diode                     a stave-dsp stage from the catalog
//! @1                                      a session opened earlier (`open`)
//! ```
//!
//! then, after `;`, `name=value` param settings (the param's own units),
//! `preset=NAME` and `label=TEXT` — the plugin form of the voicing
//! tools' candidate grammar, so a string that works in `stave-play
//! --compare` works here. `--set NAME=VALUE` and `--preset` add to it.
//!
//! A [`Workspace`] keeps opened sessions between commands (the MCP
//! server's and the GUI's live for the process; the CLI's for one
//! command), so an agent can open once, set, then bench and render the
//! same instance.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use stave_host::config::{HostConfig, SampleRate};
use stave_host::drive::Driver;
use stave_host::instance::{apply_preset, Instance};
use stave_host::note::NoteSchedule;
use stave_host::opened::{OpenError, Opened};
use stave_measure::signal::Silence;

use crate::command::{causes, CommandError, Field, Input, Kind};

/// The target field every instance command takes first.
pub const TARGET: Field = Field::positional(
    "target",
    Kind::String,
    "What to open: PLUGIN.vst3, au:NAME, stage:NAME or @SESSION, then optional ;name=value, ;preset=NAME, ;label=TEXT",
);
pub const RATE: Field = Field::flag("rate", Kind::Number, "Sample rate, Hz (a fresh target; a session keeps its own)").default("48000");
pub const BLOCK: Field = Field::flag("block", Kind::Integer, "Largest block per process call, frames").default("256");
pub const SET: Field = Field::flag("set", Kind::Strings, "Set a param, NAME=VALUE in its own units (repeatable)");
pub const PRESET: Field = Field::flag("preset", Kind::String, "Apply a factory preset first");

/// A parsed target string.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// The bundle path, `au:…`, `stage:…` or `@session`.
    pub head: String,
    pub preset: Option<String>,
    pub settings: Vec<(String, f64)>,
    pub label: Option<String>,
}

impl Target {
    pub fn parse(spec: &str) -> Result<Self, CommandError> {
        let usage = |m: String| CommandError::Usage(format!("target '{spec}': {m}"));
        let mut pieces = spec.split(';').map(str::trim);
        let head = pieces
            .next()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| usage("empty".into()))?;
        let mut t = Self {
            head: expand_home(head),
            preset: None,
            settings: Vec::new(),
            label: None,
        };
        for piece in pieces.filter(|p| !p.is_empty()) {
            let (k, v) = piece
                .split_once('=')
                .ok_or_else(|| usage(format!("'{piece}': expected name=value, preset=NAME or label=TEXT")))?;
            let (k, v) = (k.trim(), v.trim());
            match k {
                "preset" => t.preset = Some(v.into()),
                "label" => t.label = Some(v.into()),
                _ => {
                    let value = v
                        .parse()
                        .map_err(|_| usage(format!("'{piece}': '{v}' is not a number")))?;
                    t.set(k, value);
                }
            }
        }
        Ok(t)
    }

    /// The target an input names: its `target` string, plus its `set`
    /// and `preset` fields (which win over the string's).
    pub fn from_input(input: &Input) -> Result<Self, CommandError> {
        let mut t = Self::parse(input.str("target")?)?;
        for s in input.strings("set") {
            let (k, v) = parse_set(&s)?;
            t.set(&k, v);
        }
        if let Some(p) = input.opt_str("preset") {
            t.preset = Some(p.into());
        }
        Ok(t)
    }

    pub fn set(&mut self, name: &str, value: f64) {
        match self.settings.iter_mut().find(|(n, _)| n == name) {
            Some(s) => s.1 = value,
            None => self.settings.push((name.into(), value)),
        }
    }

    /// `Some("1")` for `@1`.
    pub fn session(&self) -> Option<&str> {
        self.head.strip_prefix('@')
    }

    /// The label, else the head.
    pub fn display(&self) -> String {
        self.label.clone().unwrap_or_else(|| self.head.clone())
    }

    /// Apply the preset, then the settings.
    pub fn apply(&self, inst: &mut dyn Instance) -> Result<(), CommandError> {
        if let Some(p) = &self.preset {
            apply_preset(inst, p).map_err(|e| CommandError::Failed(format!("preset: {e}")))?;
        }
        for (name, value) in &self.settings {
            inst.set_param(name, *value)
                .map_err(|e| CommandError::Failed(format!("set {name}: {e} (see `stave params`)")))?;
        }
        Ok(())
    }
}

/// `NAME=VALUE`.
pub fn parse_set(s: &str) -> Result<(String, f64), CommandError> {
    let bad = || CommandError::Usage(format!("set: '{s}' is not NAME=VALUE with a number"));
    let (name, value) = s.split_once('=').ok_or_else(bad)?;
    let value: f64 = value.trim().parse().map_err(|_| bad())?;
    Ok((name.trim().into(), value))
}

/// A leading `~/` is the home directory (targets are often quoted for
/// their `;`, so no shell expanded it).
fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest).display().to_string(),
        _ => p.into(),
    }
}

/// The host configuration an input asks for (`rate`, `block`).
pub fn host_config(input: &Input) -> Result<HostConfig, CommandError> {
    let mut c = HostConfig::default();
    if let Some(hz) = input.number("rate") {
        c.sample_rate = SampleRate::new(hz).map_err(|_| CommandError::Usage(format!("rate: {hz} is not a positive rate")))?;
    }
    if let Some(n) = input.integer("block") {
        c.max_block = NonZeroUsize::new(n as usize).ok_or_else(|| CommandError::Usage("block: at least 1".into()))?;
    }
    Ok(c)
}

/// One opened instance kept between commands.
pub struct Session {
    pub id: String,
    /// What was opened, with the settings applied since.
    pub target: Target,
    pub config: HostConfig,
    pub opened: Opened,
}

/// Open sessions by id (`@1`, `@2` …).
#[derive(Default)]
pub struct Workspace {
    sessions: Vec<Session>,
    next: u32,
}

impl Workspace {
    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.iter()
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Session, CommandError> {
        let id = id.trim_start_matches('@');
        let known: Vec<String> = self.sessions.iter().map(|s| format!("@{}", s.id)).collect();
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| CommandError::Usage(format!("no session @{id} (open: {})", if known.is_empty() { "none".into() } else { known.join(", ") })))
    }

    /// Keep `opened` as a new session; its id.
    pub fn insert(&mut self, target: Target, config: HostConfig, opened: Opened) -> String {
        self.next += 1;
        let id = self.next.to_string();
        self.sessions.push(Session {
            id: id.clone(),
            target,
            config,
            opened,
        });
        id
    }

    pub fn remove(&mut self, id: &str) -> Result<Session, CommandError> {
        let id = self.get_mut(id)?.id.clone();
        let i = self.sessions.iter().position(|s| s.id == id).unwrap_or_default();
        Ok(self.sessions.remove(i))
    }
}

/// Open a fresh instance of a (non-session) target, warmed up, its
/// preset and settings applied.
pub fn open_fresh(target: &Target, config: HostConfig) -> Result<Opened, CommandError> {
    let mut opened = Opened::open(Path::new(&target.head), config).map_err(|e| CommandError::Failed(causes(&e)))?;
    warm_up(opened.instance());
    target.apply(opened.instance())?;
    Ok(opened)
}

/// An instance a command works on: a fresh one, or a session's.
pub enum Held<'w> {
    Fresh(Box<Opened>, Target),
    Session(&'w mut Session),
}

impl Held<'_> {
    pub fn instance(&mut self) -> &mut dyn Instance {
        match self {
            Self::Fresh(o, _) => o.instance(),
            Self::Session(s) => s.opened.instance(),
        }
    }

    pub fn opened(&self) -> &Opened {
        match self {
            Self::Fresh(o, _) => o,
            Self::Session(s) => &s.opened,
        }
    }

    pub fn target(&self) -> &Target {
        match self {
            Self::Fresh(_, t) => t,
            Self::Session(s) => &s.target,
        }
    }
}

/// The instance an input names: a session (its `set` and `preset`
/// applied to it, and remembered), or a fresh instance at the input's
/// rate and block.
pub fn hold<'w>(input: &Input, workspace: &'w mut Workspace) -> Result<Held<'w>, CommandError> {
    let target = Target::from_input(input)?;
    match target.session() {
        Some(id) => {
            let s = workspace.get_mut(id)?;
            target.apply(s.opened.instance())?;
            if let Some(p) = target.preset {
                s.target.preset = Some(p);
            }
            for (k, v) in target.settings {
                s.target.set(&k, v);
            }
            Ok(Held::Session(s))
        }
        None => {
            let config = host_config(input)?;
            Ok(Held::Fresh(Box::new(open_fresh(&target, config)?), target))
        }
    }
}

/// A factory of fresh instances for a command that needs one per run
/// (validation, the level response): a session's target and config are
/// reopened. The preset and settings are checked once here, so the
/// factory can apply them without failing.
pub fn factory(input: &Input, workspace: &mut Workspace) -> Result<(Target, HostConfig), CommandError> {
    let target = Target::from_input(input)?;
    let (mut t, config) = match target.session() {
        Some(id) => {
            let s = workspace.get_mut(id)?;
            (s.target.clone(), s.config)
        }
        None => (target.clone(), host_config(input)?),
    };
    if target.session().is_some() {
        if let Some(p) = target.preset {
            t.preset = Some(p);
        }
        for (k, v) in target.settings {
            t.set(&k, v);
        }
    }
    open_fresh(&t, config)?;
    Ok((t, config))
}

/// A fresh instance of a checked target (see [`factory`]).
pub fn reopen(target: &Target, config: HostConfig) -> Result<Box<dyn Instance>, OpenError> {
    let mut inst = Opened::open(Path::new(&target.head), config)?.into_instance();
    let _ = target.apply(inst.as_mut());
    Ok(inst)
}

/// A few silent blocks: stave plugins register their surface on the
/// telemetry during their first process calls.
pub fn warm_up(plugin: &mut dyn Instance) {
    let mut d = Driver::new(plugin);
    let n = 4 * d.block() as u64;
    d.skip(n, &Silence, &NoteSchedule::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_candidate_plugin_form() {
        let t = Target::parse("stage:clipper/diode; drive=0.6 ;preset=Hot;label=diode hot").unwrap();
        assert_eq!(t.head, "stage:clipper/diode");
        assert_eq!(t.settings, [("drive".to_string(), 0.6)]);
        assert_eq!((t.preset.as_deref(), t.label.as_deref()), (Some("Hot"), Some("diode hot")));
        assert_eq!(Target::parse("@2").unwrap().session(), Some("2"));
        for bad in ["", ";drive=1", "x;drive", "x;drive=hot"] {
            assert!(matches!(Target::parse(bad), Err(CommandError::Usage(_))), "{bad}");
        }
    }

    #[test]
    fn sets_parse_or_name_themselves() {
        assert_eq!(parse_set("Delay · mix = 0.5").unwrap(), ("Delay · mix".into(), 0.5));
        assert!(parse_set("gain").is_err());
        assert!(parse_set("gain=loud").is_err());
    }
}
