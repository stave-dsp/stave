//! The CLI layer: `stave <words> [args]`, every command's flags and help
//! generated from its fields, `--json` printing [`Outcome::data`].
//!
//! ```text
//! stave bench stage:clipper/diode --seconds 1 --json
//! stave fit stimulus capture.wav            (the id fit.stimulus)
//! stave help bench                          a command's flags
//! stave commands --json                     every command with its schemas
//! ```
//!
//! Exit status: 0 ok; 1 a failing verdict (validation failed, a null is
//! audible, the corpus has a problem); 2 a usage error; 3 the command
//! could not run (a plugin would not open, a file would not write);
//! 130 cancelled.

use std::io::Write;

use serde_json::{json, Map, Value};

use crate::command::{Cancel, CommandError, CommandInfo, Context, Field, Kind, Outcome, Progress};
use crate::registry::{input_schema, output_schema, Registry};
use crate::target::Workspace;

/// The words the binary handles itself, for the top-level help.
pub const LAYERS: &[(&str, &str)] = &[
    ("play [ARGS]", "the windowed host (GUI): stave-play with ARGS"),
    ("mcp", "serve every command as an MCP tool on stdin/stdout"),
    ("commands", "list the commands (--json: with their input and output schemas)"),
    ("help [COMMAND]", "this text, or a command's flags"),
];

/// Exit status of a usage error.
pub const USAGE_EXIT: u8 = 2;

/// Flags every command takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Globals {
    /// Print the outcome's data (or the error) as JSON on stdout.
    pub json: bool,
    /// No progress on stderr.
    pub quiet: bool,
}

/// What a command line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Top-level help (`None`) or one command's.
    Help(Option<&'static str>),
    /// `stave commands`.
    Commands,
    Run { id: &'static str, input: Value },
}

/// The command a line's leading words name, and the arguments after.
fn resolve<'a>(registry: &Registry, args: &'a [String]) -> Option<(&'static str, &'a [String])> {
    let words: Vec<&str> = args.iter().take_while(|a| !a.starts_with('-')).map(String::as_str).collect();
    registry
        .commands()
        .map(|c| c.info())
        .filter(|i| {
            let w: Vec<&str> = i.words().collect();
            w.len() <= words.len() && w[..] == words[..w.len()]
        })
        .max_by_key(|i| i.words().count())
        .map(|i| (i.id, &args[i.words().count()..]))
}

/// Parse a command line (without the program name).
pub fn parse(registry: &Registry, args: &[String]) -> Result<(Globals, Request), CommandError> {
    let mut g = Globals::default();
    let args: Vec<String> = args
        .iter()
        .filter(|a| match a.as_str() {
            "--json" => {
                g.json = true;
                false
            }
            "--quiet" | "-q" => {
                g.quiet = true;
                false
            }
            _ => true,
        })
        .cloned()
        .collect();
    let first = args.first().map(String::as_str);
    match first {
        None | Some("-h" | "--help") => return Ok((g, Request::Help(None))),
        Some("commands") => return Ok((g, Request::Commands)),
        Some("help") => {
            let rest = &args[1..];
            if rest.is_empty() {
                return Ok((g, Request::Help(None)));
            }
            let (id, extra) = resolve(registry, rest).ok_or_else(|| unknown(rest))?;
            if !extra.is_empty() {
                return Err(unknown(rest));
            }
            return Ok((g, Request::Help(Some(id))));
        }
        _ => {}
    }
    let (id, rest) = resolve(registry, &args).ok_or_else(|| unknown(&args))?;
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        return Ok((g, Request::Help(Some(id))));
    }
    let info = registry.get(id).map(|c| *c.info()).ok_or_else(|| unknown(&args))?;
    Ok((g, Request::Run { id, input: input(&info, rest)? }))
}

fn unknown(args: &[String]) -> CommandError {
    let words: Vec<&str> = args.iter().take_while(|a| !a.starts_with('-')).map(String::as_str).collect();
    CommandError::Usage(format!("unknown command '{}' (see `stave help`)", words.join(" ")))
}

/// A command's arguments as its input object (values as given: the
/// registry reads numbers and booleans from strings).
pub fn input(info: &CommandInfo, args: &[String]) -> Result<Value, CommandError> {
    let usage = |m: String| CommandError::Usage(format!("{}: {m}", info.id.replace('.', " ")));
    let mut out = Map::new();
    let mut positionals = info.inputs.iter().filter(|f| f.positional);
    let mut current: Option<&Field> = None;
    let mut it = args.iter();
    let push = |out: &mut Map<String, Value>, f: &Field, v: String| {
        if f.kind == Kind::Strings {
            match out.entry(f.name).or_insert_with(|| json!([])) {
                Value::Array(a) => a.push(Value::String(v)),
                other => *other = json!([v]),
            }
        } else {
            out.insert(f.name.into(), Value::String(v));
        }
    };
    while let Some(arg) = it.next() {
        if let Some(flag) = arg.strip_prefix("--") {
            let (name, inline) = match flag.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (flag, None),
            };
            let f = info
                .inputs
                .iter()
                .find(|f| f.flag_name() == format!("--{name}"))
                .ok_or_else(|| usage(format!("unknown option --{name} (see `stave help {}`)", info.id.replace('.', " "))))?;
            let value = match (f.kind, inline) {
                (_, Some(v)) => v,
                (Kind::Bool, None) => "true".into(),
                (_, None) => it.next().cloned().ok_or_else(|| usage(format!("--{name} needs a value")))?,
            };
            push(&mut out, f, value);
            continue;
        }
        let f = match current {
            Some(f) if f.kind == Kind::Strings => f,
            _ => positionals
                .next()
                .ok_or_else(|| usage(format!("unexpected argument '{arg}'")))?,
        };
        current = Some(f);
        push(&mut out, f, arg.clone());
    }
    Ok(Value::Object(out))
}

/// Run a command line: parse, run, print. `out` is stdout (the JSON
/// channel), `err` stderr (progress, errors). The exit status.
pub fn run(registry: &Registry, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let (g, request) = match parse(registry, args) {
        Ok(p) => p,
        Err(e) => return fail(&e, args.iter().any(|a| a == "--json"), out, err),
    };
    match request {
        Request::Help(None) => {
            let _ = write!(out, "{}", help(registry));
            0
        }
        Request::Help(Some(id)) => {
            let _ = write!(out, "{}", command_help(registry.get(id).map(|c| c.info()).expect("resolved")));
            0
        }
        Request::Commands => {
            let _ = writeln!(out, "{}", commands(registry, g.json));
            0
        }
        Request::Run { id, input } => {
            let mut ws = Workspace::default();
            let mut progress = |p: &Progress| {
                if !g.quiet {
                    let of = p.total.map_or_else(String::new, |t| format!(" ({}/{t})", p.done));
                    let _ = writeln!(err, "[{id}] {}{of}", p.message);
                }
            };
            let result = registry.run(id, input, &mut Context::new(&mut ws, &mut progress, Cancel::default()));
            match result {
                Ok(o) => print(&o, g, out, err),
                Err(e) => fail(&e, g.json, out, err),
            }
        }
    }
}

fn print(o: &Outcome, g: Globals, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if g.json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&o.data).unwrap_or_default());
    } else {
        let nl = if o.text.ends_with('\n') { "" } else { "\n" };
        let _ = write!(out, "{}{nl}", o.text);
    }
    if !g.quiet {
        for a in &o.artefacts {
            let _ = writeln!(err, "wrote {} ({})", a.path.display(), a.role);
        }
    }
    let _ = out.flush();
    o.exit_code()
}

fn fail(e: &CommandError, as_json: bool, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "stave: {e}");
    if as_json {
        let kind = match e {
            CommandError::Usage(_) => "usage",
            CommandError::Failed(_) => "failed",
            CommandError::Cancelled => "cancelled",
        };
        let _ = writeln!(out, "{}", json!({ "error": { "kind": kind, "message": e.to_string() } }));
    }
    if matches!(e, CommandError::Usage(_)) {
        let _ = writeln!(err, "see `stave help`");
    }
    e.exit_code()
}

/// `stave commands`: ids and summaries, or with `json` every command's
/// description and schemas.
pub fn commands(registry: &Registry, as_json: bool) -> String {
    if as_json {
        let list: Vec<Value> = registry
            .commands()
            .map(|c| {
                let i = c.info();
                json!({
                    "id": i.id, "words": i.words().collect::<Vec<_>>(), "title": i.title, "summary": i.summary,
                    "description": i.description, "long_running": i.long_running,
                    "input_schema": input_schema(i), "output_schema": output_schema(i),
                })
            })
            .collect();
        return serde_json::to_string_pretty(&list).unwrap_or_default();
    }
    registry
        .commands()
        .map(|c| c.info().id.replace('.', " "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The top-level help.
pub fn help(registry: &Registry) -> String {
    let mut s = String::from(
        "stave — the stave tool: host, measure, characterise and fit plugins and models.\n\
         One command core with three layers: this CLI, the GUI (stave play) and an MCP server (stave mcp).\n\n\
         usage: stave <command> [arguments] [--json] [--quiet]\n\n\
         commands:\n",
    );
    for c in registry.commands() {
        let i = c.info();
        s.push_str(&format!("  {:<16} {}\n", i.id.replace('.', " "), i.summary));
    }
    s.push('\n');
    for (w, about) in LAYERS {
        s.push_str(&format!("  {w:<16} {about}\n"));
    }
    s.push_str(
        "\nglobal flags:\n  --json           print the result (or the error) as JSON on stdout\n  \
         --quiet, -q      no progress or file notes on stderr\n\n\
         A TARGET is PLUGIN.vst3, au:NAME (macOS), stage:NAME (see `stave stages`) or @N (a session from\n\
         `stave open`), then optional ;name=value, ;preset=NAME, ;label=TEXT (quote it).\n\n\
         exit status: 0 ok, 1 a failing verdict (validation failed, a null audible, a corpus problem),\n\
         2 a usage error, 3 could not run, 130 cancelled.\n\
         env: STAVE_HOST_TRACE=1 streams a plugin's load trace to stderr.\n",
    );
    s
}

/// A command's help: usage, description, arguments, options, outputs.
pub fn command_help(info: &CommandInfo) -> String {
    let words = info.id.replace('.', " ");
    let mut usage = format!("usage: stave {words}");
    for f in info.inputs {
        let meta = metavar(f);
        let piece = match (f.positional, f.kind == Kind::Strings) {
            (true, true) => format!(" {meta}..."),
            (true, false) => format!(" {meta}"),
            (false, _) if f.required => format!(" {} {meta}", f.flag_name()),
            (false, _) => continue,
        };
        usage.push_str(&piece);
    }
    if info.inputs.iter().any(|f| !f.positional && !f.required) {
        usage.push_str(" [options]");
    }
    let mut s = format!("stave {words} — {}\n{}\n\n{}\n\n", info.title, info.summary, wrap(&usage, 76, 6));
    s.push_str(&wrap(info.description, 76, 0));
    s.push('\n');
    let pos: Vec<&Field> = info.inputs.iter().filter(|f| f.positional).collect();
    if !pos.is_empty() {
        s.push_str("\narguments:\n");
        for f in pos {
            s.push_str(&entry(&metavar(f), f));
        }
    }
    s.push_str("\noptions:\n");
    for f in info.inputs.iter().filter(|f| !f.positional) {
        let left = match f.kind {
            Kind::Bool => f.flag_name(),
            _ => format!("{} {}", f.flag_name(), metavar(f)),
        };
        s.push_str(&entry(&left, f));
    }
    s.push_str(&entry("--json", &Field::flag("json", Kind::Bool, "print the result (or the error) as JSON on stdout")));
    s.push_str(&entry("--quiet", &Field::flag("quiet", Kind::Bool, "no progress or file notes on stderr")));
    if !info.outputs.is_empty() {
        s.push_str("\noutput (--json), an object of:\n");
        for f in info.outputs {
            s.push_str(&entry(f.name, f));
        }
    }
    s
}

fn metavar(f: &Field) -> String {
    match f.kind {
        Kind::Number => "NUMBER".into(),
        Kind::Integer => "N".into(),
        Kind::Choice(w) => w.join("|"),
        _ => f.name.to_uppercase(),
    }
}

fn entry(left: &str, f: &Field) -> String {
    let mut help = f.help.to_string();
    if let Some(d) = f.default {
        help.push_str(&format!(" (default {d})"));
    }
    if f.kind == Kind::Strings && !f.positional {
        help.push_str(" (repeatable)");
    }
    if f.required && !f.positional && f.default.is_none() {
        help.push_str(" (required)");
    }
    let mut lines = wrap(&help, 51, 0).lines().map(String::from).collect::<Vec<_>>().into_iter();
    let first = lines.next().unwrap_or_default();
    let mut s = if left.len() <= 22 {
        format!("  {left:<22} {first}\n")
    } else {
        format!("  {left}\n  {:<22} {first}\n", "")
    };
    for l in lines {
        s.push_str(&format!("  {:<22} {l}\n", ""));
    }
    s
}

/// Words wrapped at `width` columns, continuation lines indented by
/// `indent`; a line break in the text is kept.
fn wrap(text: &str, width: usize, indent: usize) -> String {
    let mut out = String::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                out.push_str(&line);
                out.push('\n');
                line = " ".repeat(indent);
            }
            if !line.trim().is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests;
