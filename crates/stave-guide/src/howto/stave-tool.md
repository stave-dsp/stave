# The stave tool: UI, CLI and MCP

One tool, three ways in. Everything stave can do to a plugin or a model —
open it, describe it, set it, process audio through it, bench it, validate it, measure
its response, characterise it, fit it, null it — is a **command**,
implemented once in [`stave_tools`]. A person reaches it through the GUI,
a shell or a script through the CLI, and an agent through MCP; all three
run the same code on the same inputs and get the same result.

```text
                 ┌──────────────── stave_tools ────────────────┐
  stave play ──▶ │  Registry                                    │
  (GUI palette)  │   describe  params  presets  stages          │
                 │   open  set  close  process  bench           │──▶ stave_host, stave_measure,
  stave …    ──▶ │   validate  response  characterise           │    stave_test (the work)
  (CLI)          │   fit  fit.stimulus  corpus.list  corpus.check│
                 │   + null  verify  materials  screenshot (*)  │
  stave mcp  ──▶ │  CommandInfo · Input · Outcome · Context     │
  (MCP server)   └──────────────────────────────────────────────┘
```

(*) The voicing modules — null tests, blessed takes, program material —
still lives in `stave-play` beside the Voicing view; the `stave` binary
registers its headless modes as commands that run it, so they are CLI
commands and MCP tools like the rest.

## The command core

A command declares itself with a [`CommandInfo`](stave_tools::CommandInfo):

| Part | Used by |
|---|---|
| `id` — dotted words, `bench`, `fit.stimulus` | CLI words (`stave fit stimulus`), MCP tool name (`fit_stimulus`) |
| `title`, `summary`, `description` | the palette, `--help`, the MCP tool description |
| `inputs: &[Field]` — name, [`Kind`](stave_tools::Kind), help, required, positional, default | CLI flags and help, MCP `inputSchema`, validation |
| `outputs: &[Field]` | `--json`'s documented fields, MCP `outputSchema` |
| `long_running` | progress and cancellation are expected |

The [`Registry`](stave_tools::Registry) checks an input against the
fields before the command runs — unknown keys, wrong kinds and missing
required fields are usage errors, numbers given as strings (as a command
line gives them) are read, and declared defaults are filled in, so a
default lives in exactly one place. The command gets an
[`Input`](stave_tools::Input) and a [`Context`](stave_tools::Context)
(the open sessions, a progress sink, a cancellation flag) and returns an
[`Outcome`](stave_tools::Outcome): the stable JSON (`data`), the same
for a person (`text`), the files it wrote (`artefacts`), and whether its
verdict passed (`status`).

Running one is the same from any layer:

```rust
use stave_tools::{json, registry, Cancel, Context, Progress, Status, Workspace};

let r = registry();
let mut sessions = Workspace::default();
let mut progress = |p: &Progress| eprintln!("{}", p.message);
let mut cx = Context::new(&mut sessions, &mut progress, Cancel::default());

let out = r.run("describe", json!({ "target": "stage:clipper/diode" }), &mut cx).unwrap();
assert_eq!(out.status, Status::Ok);
assert_eq!(out.data["layout"]["outputs"], 2);
let params: Vec<&str> = out.data["params"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
assert!(params.contains(&"threshold"));
```

Adding a capability is adding a command: a `CommandInfo` and a function
(or a type implementing [`Command`](stave_tools::Command)), registered
with [`Registry::register_fn`](stave_tools::Registry::register_fn).
It is then a CLI subcommand with generated help, an MCP tool with a
schema, and an entry in the GUI's palette, with no layer touched:

```rust
use stave_tools::{json, CommandError, CommandInfo, Context, Field, Input, Kind, Outcome, Registry, Value};

const GREET: CommandInfo = CommandInfo {
    id: "greet",
    title: "Greet",
    summary: "Say hello",
    description: "Say hello to someone, a number of times.",
    inputs: &[
        Field::positional("who", Kind::String, "Whom to greet"),
        Field::flag("times", Kind::Integer, "How often").default("1"),
    ],
    outputs: &[Field::flag("greeting", Kind::String, "The greeting").required()],
    long_running: false,
};

fn greet(input: &Input, _: &mut Context<'_>) -> Result<Outcome, CommandError> {
    let g = format!("hello {}", input.str("who")?).repeat(input.integer("times").unwrap_or(1) as usize);
    Outcome::ok(json!({ "greeting": g }), g.clone())
}

let mut r = Registry::new();
r.register_fn(GREET, greet);

// The CLI layer, on a command line:
let args: Vec<String> = ["greet", "you", "--times", "2", "--json"].map(String::from).to_vec();
let (mut out, mut err) = (Vec::new(), Vec::new());
assert_eq!(stave_tools::cli::run(&r, &args, &mut out, &mut err), 0);
let printed: Value = std::str::from_utf8(&out).unwrap().parse().unwrap();
assert_eq!(printed["greeting"], "hello youhello you");

// The same help a person reads with `stave help greet`:
assert!(stave_tools::cli::command_help(&GREET).contains("--times N"));

```

### Targets and sessions

Every instance command takes a **target**: `PLUGIN.vst3`, `au:NAME`
(macOS), `stage:NAME` (`stave stages` lists the catalog) or `@N`, then
optional `;name=value` settings, `;preset=NAME` and `;label=TEXT` — the
plugin form of the voicing candidate grammar, so a string that works in
`stave play --compare` works everywhere. `--set NAME=VALUE` and
`--preset` add to it.

`open` keeps an instance as a session, `@1`, in the
[`Workspace`](stave_tools::Workspace); later commands that name `@1` work
on that instance with its settings, and `validate` and `response`, which
need a fresh instance per run, reopen it with them. An MCP server's and
the GUI's sessions live for the whole process; the CLI's for one command.

## The CLI: `stave`

One binary, `stave` (package `stave-cli`). Its words are the command ids;
`stave help` lists them, `stave help COMMAND` prints a command's flags,
and `stave commands --json` prints every command with its input and
output schemas.

```sh
stave stages                                   # what stage:NAME takes
stave describe build/StaveTilt.vst3 --json | jq '.params[].name'
stave bench build/StaveTilt.vst3 --seconds 4 --set Tilt=3 --out take.wav
stave bench Some.vst3 --signal sine:1000:-6 --json | jq '.output.tone'
stave process stage:clipper/diode --signal wav:di.wav --out driven.wav
stave validate build/StaveTilt.vst3 || echo BROKEN
stave response stage:filter/peak --levels -24,-6,0
stave characterise stage:circuit/ts808 'au:Some Drive;Drive=1' --tier t0
stave null stage:clipper/diode wav:take.wav --material 'role:guitar_di#1'
stave fit stimulus capture.wav --reference 0.05
stave fit --circuit tube_screamer --out ts.json
stave corpus check
stave play build/StaveTilt.vst3                # the GUI
```

Every command takes `--json` (print `Outcome::data`, or
`{"error": {"kind", "message"}}`, on stdout) and `--quiet` (no progress
or file notes on stderr). stdout carries only the result: anything a
plugin prints goes to stderr.

| Exit | Meaning |
|---|---|
| 0 | ok |
| 1 | the command ran and its verdict failed: a validation check failed, a null is audible, a blessed take changed, the corpus has a problem |
| 2 | usage: an unknown command or flag, a malformed value |
| 3 | it could not run: a plugin would not open, a file would not write |
| 130 | cancelled |

## MCP: `stave mcp`

`stave mcp` is a Model Context Protocol server on stdin/stdout
(newline-delimited JSON-RPC 2.0). Register it with a client:

```sh
claude mcp add stave -- stave mcp
```

- **Tools**: every command, named by its id with `_` for `.`
  (`fit_stimulus`, `corpus_check`), its fields as `inputSchema` and
  `outputSchema`; a call returns the text, the data as
  `structuredContent`, and every file it wrote as a `resource_link`
  (a PNG also as image content). A failing verdict starts the text with
  `status: failed`; a command that could not run is `isError: true`.
- **Progress**: a call with a `progressToken` gets
  `notifications/progress` — validation per check, characterisation per
  target and rate.
- **Cancellation**: `notifications/cancelled` stops a long call at its
  next step.
- **Resources**: `resources/list` and `resources/read` serve the files
  commands wrote this session — processed WAVs, reports, models — and
  nothing else.
- **Sessions**: calls run one at a time on one worker that keeps the
  sessions, so `open` then `bench @1` work on one instance.

Protocol versions 2025-06-18 (structured content, resource links,
titles), 2025-03-26 and 2024-11-05 (plain text content). The protocol
is implemented in `stave_tools::mcp` on serde_json: six methods and two
notifications do not justify an SDK with an async runtime.

```rust
use std::io::Cursor;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

let requests = [
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"stages","arguments":{}}}"#,
];
let sink = Sink::default();
stave_tools::mcp::serve(stave_tools::registry(), Cursor::new(requests.join("\n")), Box::new(sink.clone())).unwrap();
let replies = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
let call: stave_tools::Value = replies.lines().nth(1).unwrap().parse().unwrap();
assert!(call["result"]["structuredContent"]["stages"].as_array().unwrap().len() > 3);
```

## The GUI: `stave play`

`stave play [ARGS]` runs the windowed host, `stave-play`, with the same
arguments ([Use the host tools](crate::howto::host_tools),
[Voicing](crate::howto::voicing)). Its command palette runs registry
commands, so what an agent can call, a person can find by name.

## Moving from the old binaries

`stave-bench` and `stave-fit` are deprecated shims for one release: each
prints a one-line notice and runs the new command through the same
registry, so scripts keep working.

| Old | New |
|---|---|
| `stave-bench X [opts]` | `stave bench X [opts]` |
| `stave-bench X --describe` | `stave describe X --json` |
| `stave-bench X --list` | `stave describe X` (or `stave params X`) |
| `stave-bench X --validate` | `stave validate X --json` |
| `stave-bench X --response --response-levels L` | `stave response X --levels L` |
| `stave-fit stimulus OUT [opts]` | `stave fit stimulus OUT [opts]` |
| `stave-fit fit -o M [opts]` | `stave fit --out M [opts]` |
| `stave-play --characterise A --characterise B` | `stave characterise A B` |
| `stave-play --null A B` | `stave null A B` |
| `stave-play --verify-blessed P` | `stave verify P` |
| `stave-play --list-material` | `stave materials` |
| `stave-play X --screenshot P` | `stave screenshot X --out P` |

The flags kept their names (`--analysis-window`, `--program-db`,
`--export-pair`); the exit status for "could not run" is now 3 (it was 1
in `stave-bench`), so a script that tests only for non-zero is unchanged.
