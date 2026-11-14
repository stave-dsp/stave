//! The stave tool's shared foundation: every capability — open,
//! describe, bench, process, validate, measure, characterise, fit — is a
//! typed [`Command`] in one [`Registry`], implemented once here. The
//! layers on top only present it:
//!
//! ```text
//! GUI   stave-play's command palette     runs a command, shows its text and artefacts
//! CLI   `stave <words> …`                 input from flags, --json prints Outcome::data
//! MCP   `stave mcp`                       each command a tool, its fields as JSON Schema
//! ```
//!
//! ```text
//! command   the model: CommandInfo, Field, Input, Outcome, Context (progress, cancel)
//! registry  commands by id; input validation; JSON Schema of the fields
//! target    what a command opens (path, au:, stage:, @session) and the Workspace
//! commands/ the built-in commands, one module per family
//! cli       the CLI layer: words → command, flags → input, help, exit codes
//! mcp       the MCP layer: JSON-RPC over stdio, tools, progress, resources
//! stdout    keeping stdout for data while plugins print
//! ```
//!
//! Running one, as any layer does:
//!
//! ```
//! use stave_tools::{registry, Cancel, Context, Workspace};
//!
//! let r = registry();
//! let mut ws = Workspace::default();
//! let mut progress = |_: &stave_tools::Progress| {};
//! let mut cx = Context::new(&mut ws, &mut progress, Cancel::default());
//! let out = r.run("stages", serde_json::json!({}), &mut cx).unwrap();
//! assert!(out.data["stages"].as_array().unwrap().len() > 3);
//! ```

pub mod cli;
pub mod command;
pub mod commands;
pub mod listen;
pub mod mcp;
pub mod registry;
pub mod stdout;
pub mod target;

pub use command::{
    Artefact, Cancel, Command, CommandError, CommandInfo, Context, Field, FnCommand, Input, Kind, Outcome,
    Progress, Status,
};
pub use registry::Registry;
pub use target::Workspace;
/// Inputs and outputs are JSON; a layer needs no serde_json of its own.
pub use serde_json::{json, Value};

/// A registry holding every built-in command.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    commands::register(&mut r);
    r
}
