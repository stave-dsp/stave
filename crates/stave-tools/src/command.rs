//! The command model: what one capability declares ([`CommandInfo`]: id,
//! title, inputs, outputs), what it is given ([`Input`], a [`Context`]
//! with progress and cancellation), and what it returns ([`Outcome`]:
//! stable JSON, a human rendering, the files it wrote).
//!
//! Prior art: VS Code's command registry (an id and a title per
//! capability, run from a palette or a keybinding alike), LSP's and
//! MCP's tool schemas (inputs declared as JSON Schema), cargo's
//! subcommands (`stave fit stimulus` is the id `fit.stimulus`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::target::Workspace;

/// What a field holds. The JSON Schema type, the CLI flag's parsing and
/// the help text all read it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    Number,
    Integer,
    Bool,
    /// Several strings: a repeated flag on the command line, an array in JSON.
    Strings,
    /// One of these words.
    Choice(&'static [&'static str]),
    /// Any JSON value (outputs only: a nested report).
    Json,
}

/// One input or output field.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// snake_case; the CLI flag is `--` and the name with `-` for `_`.
    pub name: &'static str,
    pub kind: Kind,
    pub help: &'static str,
    pub required: bool,
    /// Taken by position on the command line (in declaration order).
    pub positional: bool,
    /// The value an absent input takes, as the CLI would spell it; the
    /// registry fills it in before the command runs, so this is the
    /// only place a default lives.
    pub default: Option<&'static str>,
}

impl Field {
    /// An optional `--name` flag.
    pub const fn flag(name: &'static str, kind: Kind, help: &'static str) -> Self {
        Self {
            name,
            kind,
            help,
            required: false,
            positional: false,
            default: None,
        }
    }

    /// A required positional argument.
    pub const fn positional(name: &'static str, kind: Kind, help: &'static str) -> Self {
        Self {
            positional: true,
            required: true,
            ..Self::flag(name, kind, help)
        }
    }

    pub const fn required(self) -> Self {
        Self { required: true, ..self }
    }

    pub const fn optional(self) -> Self {
        Self { required: false, ..self }
    }

    pub const fn default(self, value: &'static str) -> Self {
        Self {
            default: Some(value),
            ..self
        }
    }

    /// The command-line spelling: `--analysis-window`.
    pub fn flag_name(&self) -> String {
        format!("--{}", self.name.replace('_', "-"))
    }
}

/// What a command is, for every layer: the palette shows `title`, the
/// CLI turns `id` into words and `inputs` into flags, MCP publishes the
/// inputs and outputs as JSON Schema.
#[derive(Copy, Clone, Debug)]
pub struct CommandInfo {
    /// Dotted words: `bench`, `fit.stimulus`, `corpus.check`.
    pub id: &'static str,
    /// Imperative, for a palette: "Bench a plugin".
    pub title: &'static str,
    /// One line.
    pub summary: &'static str,
    /// The full help: paragraphs, plain text.
    pub description: &'static str,
    pub inputs: &'static [Field],
    /// The top-level fields of [`Outcome::data`].
    pub outputs: &'static [Field],
    /// Reports progress and honours cancellation (seconds or more).
    pub long_running: bool,
}

impl CommandInfo {
    /// The id as command-line words: `fit.stimulus` → `["fit", "stimulus"]`.
    pub fn words(&self) -> impl Iterator<Item = &'static str> {
        self.id.split('.')
    }

    /// The id as an MCP tool name (letters, digits, `_`): `fit_stimulus`.
    pub fn tool_name(&self) -> String {
        self.id.replace('.', "_")
    }
}

/// A command's validated input: an object whose keys are the command's
/// fields, defaults filled in, every value of its field's kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input(pub(crate) Map<String, Value>);

impl Input {
    /// The raw object (as the registry validated it).
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    pub fn opt_str(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(Value::as_str)
    }

    /// A string the schema requires (or defaults).
    pub fn str(&self, name: &str) -> Result<&str, CommandError> {
        self.opt_str(name)
            .ok_or_else(|| CommandError::Usage(format!("missing {name}")))
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.opt_str(name).map(PathBuf::from)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        self.0.get(name).and_then(Value::as_f64)
    }

    pub fn integer(&self, name: &str) -> Option<u64> {
        self.0.get(name).and_then(Value::as_u64)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.0.get(name).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn strings(&self, name: &str) -> Vec<String> {
        match self.0.get(name) {
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => Vec::new(),
        }
    }
}

/// Whether the command's verdict passed. A command that ran and judged
/// (validation failed, a null is audible, the corpus has a problem) is
/// `Failed`, not an error: the CLI exits 1.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Failed,
}

/// A file a command wrote: a rendered WAV, a report, a model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Artefact {
    pub path: PathBuf,
    /// `audio/wav`, `text/markdown`, `application/json`, `image/png` …
    pub mime: String,
    /// What it is, in a word or two: "output", "residual", "report".
    pub role: String,
}

impl Artefact {
    pub fn new(path: impl Into<PathBuf>, mime: &str, role: &str) -> Self {
        Self {
            path: path.into(),
            mime: mime.into(),
            role: role.into(),
        }
    }
}

/// What a command returns.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Outcome {
    pub status: Status,
    /// The stable JSON form: the fields [`CommandInfo::outputs`] names.
    pub data: Value,
    /// The same, for a person to read.
    #[serde(skip)]
    pub text: String,
    pub artefacts: Vec<Artefact>,
}

impl Outcome {
    /// A passing outcome from any serialisable report.
    pub fn ok(data: impl Serialize, text: impl Into<String>) -> Result<Self, CommandError> {
        Ok(Self {
            status: Status::Ok,
            data: serde_json::to_value(data).map_err(|e| CommandError::Failed(e.to_string()))?,
            text: text.into(),
            artefacts: Vec::new(),
        })
    }

    pub fn failed_if(mut self, failed: bool) -> Self {
        if failed {
            self.status = Status::Failed;
        }
        self
    }

    pub fn with(mut self, artefact: Artefact) -> Self {
        self.artefacts.push(artefact);
        self
    }

    /// The process exit code: 0 passed, 1 a failing verdict.
    pub fn exit_code(&self) -> u8 {
        match self.status {
            Status::Ok => 0,
            Status::Failed => 1,
        }
    }
}

/// Why a command did not produce an outcome.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CommandError {
    /// The input is wrong (unknown field, wrong kind, malformed value):
    /// exit 2.
    #[error("{0}")]
    Usage(String),
    /// It could not run (a plugin would not open, a file would not
    /// write): exit 3.
    #[error("{0}")]
    Failed(String),
    /// Cancelled before it finished: exit 130.
    #[error("cancelled")]
    Cancelled,
}

impl CommandError {
    /// An error and every cause, `: `-joined.
    pub fn from_error(e: &dyn std::error::Error) -> Self {
        Self::Failed(causes(e))
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Failed(_) => 3,
            Self::Cancelled => 130,
        }
    }
}

/// `error: cause: cause`.
pub fn causes(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(c) = src {
        s.push_str(": ");
        s.push_str(&c.to_string());
        src = c.source();
    }
    s
}

/// One progress report: `done` of `total` (units the command names in
/// `message`), or `total: None` while the extent is unknown.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Progress {
    pub done: f64,
    pub total: Option<f64>,
    pub message: String,
}

/// A cancellation flag shared between whoever runs a command and
/// whoever may cancel it (a palette's Escape, MCP's
/// `notifications/cancelled`, a CLI's Ctrl-C).
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// What a running command may use: the open sessions, a progress sink
/// and the cancellation flag.
pub struct Context<'a> {
    pub workspace: &'a mut Workspace,
    on_progress: &'a mut dyn FnMut(&Progress),
    cancel: Cancel,
}

impl<'a> Context<'a> {
    pub fn new(workspace: &'a mut Workspace, on_progress: &'a mut dyn FnMut(&Progress), cancel: Cancel) -> Self {
        Self {
            workspace,
            on_progress,
            cancel,
        }
    }

    pub fn progress(&mut self, done: f64, total: Option<f64>, message: impl Into<String>) {
        (self.on_progress)(&Progress {
            done,
            total,
            message: message.into(),
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// `Err(Cancelled)` once cancelled: `cx.checkpoint()?` between steps.
    pub fn checkpoint(&self) -> Result<(), CommandError> {
        if self.is_cancelled() {
            Err(CommandError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A capability. Implement it for a type, or register a function with
/// [`FnCommand`].
pub trait Command: Send + Sync {
    fn info(&self) -> &CommandInfo;
    fn run(&self, input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError>;
}

/// The function a [`FnCommand`] runs.
pub type RunFn = fn(&Input, &mut Context<'_>) -> Result<Outcome, CommandError>;

/// A command that is a static description and a function.
pub struct FnCommand {
    pub info: CommandInfo,
    pub run: RunFn,
}

impl Command for FnCommand {
    fn info(&self) -> &CommandInfo {
        &self.info
    }

    fn run(&self, input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
        (self.run)(input, cx)
    }
}
