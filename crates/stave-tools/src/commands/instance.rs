//! Opening and inspecting: `open`, `set`, `close`, `describe`, `params`,
//! `presets`, `stages`.

use std::fmt::Write;

use serde_json::json;
use stave_host::bench::{describe::describe_opened, text};
use stave_host::instance::presets as preset_names;
use stave_host::param::Param;
use stave_core::params::Params;
use stave_host::stages::{CATALOG, REFERENCES};

use crate::command::{CommandError, CommandInfo, Context, Field, Input, Kind, Outcome};
use crate::target::{hold, host_config, open_fresh, Target, BLOCK, PRESET, RATE, SET, TARGET};

const INSTANCE: &[Field] = &[TARGET, RATE, BLOCK, SET, PRESET];

pub const OPEN: CommandInfo = CommandInfo {
    id: "open",
    title: "Open a plugin or stage",
    summary: "Open a target and keep it as a session (@N) for later commands",
    description: "Open a VST3 bundle, an Audio Unit (au:NAME) or a catalog stage (stage:NAME) and keep the \
                  instance as a session. Later commands take @N as their target and work on the same \
                  instance: its settings stay, and its rate and block are the ones it was opened with. \
                  Sessions live as long as the process: an MCP server's or the GUI's for the whole run, \
                  the CLI's for one command.",
    inputs: INSTANCE,
    outputs: &[
        Field::flag("session", Kind::String, "The session's target, @N").required(),
        Field::flag("name", Kind::String, "The instance's name").required(),
        Field::flag("layout", Kind::Json, "Inputs, outputs, sample rate, max block").required(),
        Field::flag("latency", Kind::Integer, "Reported latency, frames").required(),
        Field::flag("params", Kind::Integer, "How many params it has").required(),
    ],
    long_running: false,
};

pub fn open(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let target = Target::from_input(input)?;
    if target.session().is_some() {
        return Err(CommandError::Usage("open: the target is already a session".into()));
    }
    let config = host_config(input)?;
    let mut opened = open_fresh(&target, config)?;
    let name = opened.name();
    let inst = opened.instance();
    let (layout, latency, params) = (inst.layout(), inst.latency(), inst.params().len());
    let id = cx.workspace.insert(target, config, opened);
    let text = format!(
        "@{id}: {name} — {} in / {} out @ {} Hz, latency {latency}, {params} params",
        layout.inputs, layout.outputs, layout.sample_rate
    );
    Outcome::ok(
        json!({ "session": format!("@{id}"), "name": name, "layout": layout, "latency": latency, "params": params }),
        text,
    )
}

pub const SET_PARAMS: CommandInfo = CommandInfo {
    id: "set",
    title: "Set params",
    summary: "Set a session's params or preset; prints the params after",
    description: "Apply --preset, then each --set NAME=VALUE (or the target's ;name=value pieces) to an open \
                  session. The session remembers them: a command that reopens it (validate, response) \
                  applies them again.",
    inputs: &[TARGET, SET, PRESET],
    outputs: &[Field::flag("params", Kind::Json, "Every param after the change").required()],
    long_running: false,
};

pub fn set(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    if Target::parse(input.str("target")?)?.session().is_none() {
        return Err(CommandError::Usage("set: the target must be a session (@N, from `open`)".into()));
    }
    let mut held = hold(input, cx.workspace)?;
    let params = held.instance().params();
    Outcome::ok(json!({ "params": params }), params_text(&params))
}

pub const CLOSE: CommandInfo = CommandInfo {
    id: "close",
    title: "Close a session",
    summary: "Close an open session",
    description: "Drop a session opened with `open`; its instance is released.",
    inputs: &[Field::positional("session", Kind::String, "The session, @N")],
    outputs: &[Field::flag("closed", Kind::String, "The session closed").required()],
    long_running: false,
};

pub fn close(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let s = cx.workspace.remove(input.str("session")?)?;
    Outcome::ok(json!({ "closed": format!("@{}", s.id) }), format!("closed @{}", s.id))
}

pub const DESCRIBE: CommandInfo = CommandInfo {
    id: "describe",
    title: "Describe a plugin",
    summary: "Identity, load trace, layout, latency, tail, params, presets and taps",
    description: "Open the target, run a few silent blocks (a stave plugin registers its surface then), and \
                  report what it is: the class and vendor, the load trace and host stats (VST3), the bus \
                  layout, latency, tail, every param with its range and value, the presets, and the \
                  plugin's taps.",
    inputs: INSTANCE,
    outputs: &[
        Field::flag("plugin", Kind::Json, "VST3 identity and load trace (absent for a stage)"),
        Field::flag("host", Kind::Json, "Host-side counters"),
        Field::flag("layout", Kind::Json, "Inputs, outputs, sample rate, max block").required(),
        Field::flag("latency", Kind::Integer, "Reported latency, frames").required(),
        Field::flag("tail", Kind::Json, "Reported tail").required(),
        Field::flag("params", Kind::Json, "Every param").required(),
        Field::flag("presets", Kind::Strings, "Preset names").required(),
        Field::flag("taps", Kind::Json, "The plugin's measurements").required(),
    ],
    long_running: false,
};

pub fn describe(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let held = hold(input, cx.workspace)?;
    let d = describe_opened(held.opened());
    Outcome::ok(&d, text::description(&d))
}

pub const PARAMS: CommandInfo = CommandInfo {
    id: "params",
    title: "List params",
    summary: "Every param: name, value, unit, range, steps",
    description: "The target's params as it would play them: values read live, after any preset and settings.",
    inputs: INSTANCE,
    outputs: &[Field::flag("params", Kind::Json, "Every param").required()],
    long_running: false,
};

pub fn params(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut held = hold(input, cx.workspace)?;
    let params = held.instance().params();
    Outcome::ok(json!({ "params": params }), params_text(&params))
}

pub const PRESETS: CommandInfo = CommandInfo {
    id: "presets",
    title: "List presets",
    summary: "The target's factory presets, in program order",
    description: "The positions of the target's program param (VST3 kIsProgramChange): what --preset and \
                  ;preset=NAME select.",
    inputs: &[TARGET, RATE, BLOCK],
    outputs: &[Field::flag("presets", Kind::Strings, "Preset names").required()],
    long_running: false,
};

pub fn presets(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut held = hold(input, cx.workspace)?;
    let names = preset_names(held.instance());
    let text = if names.is_empty() { "no presets".into() } else { names.join("\n") };
    Outcome::ok(json!({ "presets": names }), text)
}

pub const STAGES: CommandInfo = CommandInfo {
    id: "stages",
    title: "List catalog stages",
    summary: "Every stage:NAME target, and its params",
    description: "The stave-dsp stages a target can name as stage:NAME, hosted like plugins (their params \
                  are a plugin's). Reference stages are the classic circuits the catalog keeps for \
                  comparison.",
    inputs: &[],
    outputs: &[Field::flag("stages", Kind::Json, "name, reference, params (name, default, range)").required()],
    long_running: false,
};

pub fn stages(_: &Input, _: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut list = Vec::new();
    let mut text = String::new();
    for (spec, reference) in CATALOG.iter().map(|s| (s, false)).chain(REFERENCES.iter().map(|s| (s, true))) {
        let table = (spec.params)();
        let params: Vec<String> = (0..table.count()).filter_map(|i| table.by_index(i)).map(|p| p.meta().name.to_string()).collect();
        let _ = writeln!(text, "stage:{:<26} {}{}", spec.name, params.join(", "), if reference { "  (reference)" } else { "" });
        list.push(json!({ "name": format!("stage:{}", spec.name), "reference": reference, "params": params }));
    }
    Outcome::ok(json!({ "stages": list }), text)
}

/// Params as aligned lines (the describe text's).
pub fn params_text(params: &[Param]) -> String {
    let mut s = String::new();
    for c in params {
        let value = c
            .step_names
            .get(c.value as usize)
            .filter(|_| c.step_count > 0)
            .map_or_else(|| format!("{:.4}", c.value), |name| name.clone());
        let _ = writeln!(
            s,
            "{:<28} {:>10} {:<6} [{} .. {}]{}",
            c.name,
            value,
            c.unit,
            c.min,
            c.max,
            if c.read_only { "  (read-only)" } else { "" }
        );
    }
    if params.is_empty() {
        s.push_str("no params\n");
    }
    s
}
