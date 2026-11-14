//! Commands that run stave-play: the voicing modules' headless modes
//! (null tests, verifying blessed takes, the program material) and the
//! editor screenshot. They are registry commands like any other — CLI
//! flags, `--json`, MCP tools — whose work happens in a stave-play
//! process: those modules live with the Voicing view until it moves into
//! `stave-tools`.

use std::path::PathBuf;
use std::process::{Command as Process, ExitCode};

use serde_json::json;
use stave_tools::{Artefact, CommandError, CommandInfo, Context, Field, Input, Kind, Outcome, Registry};

const STAVE_PLAY: &str = "stave-play";

/// stave-play beside this binary, else on PATH.
fn stave_play() -> PathBuf {
    let name = format!("{STAVE_PLAY}{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name(&name))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// `stave play ARGS`: the GUI, with the terminal.
pub fn play(args: &[String]) -> Result<ExitCode, String> {
    let exe = stave_play();
    let status = Process::new(&exe)
        .args(args)
        .status()
        .map_err(|e| format!("{}: {e} (build it: cargo build -p stave-play)", exe.display()))?;
    Ok(ExitCode::from(status.code().map_or(3, |c| c.clamp(0, 255) as u8)))
}

/// Run stave-play headless: its exit status and its stdout.
fn run(args: &[String], cx: &mut Context<'_>) -> Result<(i32, String), CommandError> {
    let exe = stave_play();
    cx.progress(0.0, None, format!("{STAVE_PLAY} {}", args.join(" ")));
    let out = Process::new(&exe)
        .args(args)
        .output()
        .map_err(|e| CommandError::Failed(format!("{}: {e} (build it: cargo build -p stave-play)", exe.display())))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code().unwrap_or(3);
    if code >= 2 {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let why = stderr.lines().find(|l| l.starts_with("stave-play:")).unwrap_or(stderr.trim());
        return Err(CommandError::Failed(why.trim_start_matches("stave-play: ").to_string()));
    }
    Ok((code, stdout))
}

fn push(args: &mut Vec<String>, flag: &str, value: Option<String>) {
    if let Some(v) = value {
        args.push(flag.into());
        args.push(v);
    }
}

fn number(input: &Input, name: &str) -> Option<String> {
    input.number(name).map(|v| v.to_string())
}

const CANDIDATE: &str = "dry, PLUGIN.vst3, au:NAME, stage:NAME, wav:FILE or blessed:RECORD, then ;name=value, ;preset=NAME, ;label=TEXT";
const MATERIAL: Field = Field::flag(
    "material",
    Kind::String,
    "Program material: a built-in name (see `stave materials`), role:NAME[#N] from the reference corpus, or a WAV path",
);

pub const NULL: CommandInfo = CommandInfo {
    id: "null",
    title: "Null a candidate against a reference",
    summary: "Null A against B on program material; the residual's NMR verdict (DSP.md R16, G5)",
    description: "Run candidate A and reference B (another model, or a capture as wav:FILE) on the material, \
                  level- and latency-aligned, and judge the residual by its noise-to-mask ratio under the \
                  reference: inaudible at -10 dB or below in total and in the worst band. Fails (exit 1) \
                  when the difference is audible. --residual writes the residual to listen to. To hear \
                  it and ABX the pair: stave play --compare A --compare 'B;ref' (the Null test mode).",
    inputs: &[
        Field::positional("candidate", Kind::String, CANDIDATE),
        Field::positional("reference", Kind::String, CANDIDATE),
        MATERIAL,
        Field::flag("trim", Kind::Number, "Material trim, dB"),
        Field::flag("residual", Kind::String, "Write the residual (as heard) to this WAV"),
        Field::flag("rate", Kind::Number, "Session sample rate, Hz"),
        Field::flag("block", Kind::Integer, "Frames per process call"),
    ],
    outputs: &[
        Field::flag("inaudible", Kind::Bool, "The verdict").required(),
        Field::flag("verdict", Kind::String, "INAUDIBLE, AUDIBLE, … as the report words it").required(),
        Field::flag("report", Kind::String, "The null's line: NMR total and worst band, residual, alignment").required(),
    ],
    long_running: true,
};

fn null(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut args = vec!["--null".into(), input.str("candidate")?.into(), input.str("reference")?.into()];
    push(&mut args, "--material", input.opt_str("material").map(String::from));
    push(&mut args, "--trim", number(input, "trim"));
    push(&mut args, "--residual", input.opt_str("residual").map(String::from));
    push(&mut args, "--sample-rate", number(input, "rate"));
    push(&mut args, "--block-size", input.integer("block").map(|n| n.to_string()));
    let (code, stdout) = run(&args, cx)?;
    let report = stdout.lines().next().unwrap_or_default().to_string();
    let verdict = report.split(':').next().unwrap_or_default().trim().to_string();
    let mut o = Outcome::ok(json!({ "inaudible": code == 0, "verdict": verdict, "report": report }), stdout)?.failed_if(code != 0);
    if let Some(p) = input.path("residual") {
        o = o.with(Artefact::new(p, "audio/wav", "residual"));
    }
    Ok(o)
}

pub const VERIFY: CommandInfo = CommandInfo {
    id: "verify",
    title: "Verify blessed takes",
    summary: "Re-run every blessed candidate with this build; fail when one changed",
    description: "Re-run each blessed candidate under the path (a bless record or a directory of them) with \
                  this build, null it against its blessed take and re-measure its fingerprint (DSP.md R18). \
                  Fails (exit 1) when a residual is above --tolerance-db re the take or a characteristic \
                  moved.",
    inputs: &[
        Field::positional("records", Kind::String, "A bless record (.json) or a directory of them"),
        Field::flag("tolerance_db", Kind::Number, "A take counts as changed above this residual, dB re the take").default("-120"),
    ],
    outputs: &[
        Field::flag("changed", Kind::Bool, "Whether any take changed").required(),
        Field::flag("lines", Kind::Strings, "One line per record: same/CHANGED/ERROR, residual, fingerprint").required(),
    ],
    long_running: true,
};

fn verify(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut args = vec!["--verify-blessed".into(), input.str("records")?.into()];
    push(&mut args, "--tolerance-db", number(input, "tolerance_db"));
    let (code, stdout) = run(&args, cx)?;
    let lines: Vec<&str> = stdout.lines().collect();
    Ok(Outcome::ok(json!({ "changed": code != 0, "lines": lines }), stdout.clone())?.failed_if(code != 0))
}

pub const MATERIALS: CommandInfo = CommandInfo {
    id: "materials",
    title: "List program material",
    summary: "The built-in program material and the reference corpus's roles",
    description: "The material a comparison or null plays: the built-in, synthesised pieces by name, and the \
                  reference corpus's roles (role:NAME[#N]) with this machine's items. --export DIR also \
                  writes the built-ins there as WAVs.",
    inputs: &[
        Field::flag("export", Kind::String, "Also write each built-in as DIR/NAME.wav"),
        Field::flag("rate", Kind::Number, "The exported WAVs' rate, Hz"),
    ],
    outputs: &[Field::flag("lines", Kind::Strings, "The listing, line by line").required()],
    long_running: false,
};

fn materials(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let mut args = vec!["--list-material".to_string()];
    push(&mut args, "--export-material", input.opt_str("export").map(String::from));
    push(&mut args, "--sample-rate", number(input, "rate"));
    let (_, stdout) = run(&args, cx)?;
    let lines: Vec<&str> = stdout.lines().collect();
    Outcome::ok(json!({ "lines": lines }), stdout.clone())
}

pub const SCREENSHOT: CommandInfo = CommandInfo {
    id: "screenshot",
    title: "Screenshot a plugin",
    summary: "Open the target in the windowed host and write a PNG of it (or of its editor)",
    description: "Open the target in stave-play, play it for a moment (a held note for an instrument with \
                  --note), and write a PNG of the window, or with --editor-only of the plugin's editor alone \
                  (a stave plugin draws its face offscreen, so no other window shows in it). Needs a display.",
    inputs: &[
        Field::positional("target", Kind::String, "PLUGIN.vst3, au:NAME or stage:NAME"),
        Field::flag("out", Kind::String, "The PNG to write").required(),
        Field::flag("note", Kind::Integer, "Hold this MIDI note while it plays"),
        Field::flag("editor_only", Kind::Bool, "Only the plugin's editor"),
        Field::flag("set", Kind::Strings, "Set a param, NAME=VALUE (repeatable)"),
    ],
    outputs: &[Field::flag("out", Kind::String, "The PNG written").required()],
    long_running: true,
};

fn screenshot(input: &Input, cx: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let out = input.path("out").ok_or_else(|| CommandError::Usage("screenshot: --out is required".into()))?;
    let mut args = vec![input.str("target")?.to_string(), "--screenshot".into(), out.display().to_string()];
    push(&mut args, "--note", input.integer("note").map(|n| n.to_string()));
    if input.flag("editor_only") {
        args.push("--editor-only".into());
    }
    for s in input.strings("set") {
        push(&mut args, "--set", Some(s));
    }
    run(&args, cx)?;
    if !out.is_file() {
        return Err(CommandError::Failed(format!("stave-play wrote no {}", out.display())));
    }
    Ok(Outcome::ok(json!({ "out": out }), format!("wrote {}", out.display()))?.with(Artefact::new(out, "image/png", "screenshot")))
}

pub fn register(r: &mut Registry) {
    r.register_fn(NULL, null)
        .register_fn(VERIFY, verify)
        .register_fn(MATERIALS, materials)
        .register_fn(SCREENSHOT, screenshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voicing_commands_parse_like_any_other() {
        let mut r = Registry::new();
        register(&mut r);
        let line: Vec<String> = "null stage:clipper/diode wav:take.wav --material role:vocal#2 --trim -3"
            .split_whitespace()
            .map(String::from)
            .collect();
        let (_, req) = stave_tools::cli::parse(&r, &line).unwrap();
        let stave_tools::cli::Request::Run { id, input } = req else { panic!() };
        assert_eq!(id, "null");
        assert_eq!(input["reference"], "wav:take.wav");
        assert_eq!(input["trim"], "-3");
        let info = r.get("screenshot").unwrap().info();
        assert!(stave_tools::cli::command_help(info).contains("--editor-only"));
    }
}
