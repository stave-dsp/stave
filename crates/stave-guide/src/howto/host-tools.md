# Use the host tools

Load any VST3, stave-built or third-party, and drive, measure and
validate it: from Rust with the `stave-host` library, from a shell or an
agent with `stave-bench`, by ear with `stave-play`, and after a DAW
session with `stave-events`. This page is recipes; the CLI reference
lists every flag from each binary's own `--help`.

## One library, three tools

[`stave_host`] does the work; the tools are thin.

| Piece | What it is |
|---|---|
| [`plugin`](stave_host::plugin) | VST3 hosting: [`LoadedPlugin`](stave_host::plugin::LoadedPlugin) |
| [`instance`](stave_host::instance) | the [`Instance`](stave_host::instance::Instance) trait, the seam everything measures through |
| [`adapt`](stave_host::adapt) | any stave-dsp [`Stage`](stave_dsp::Stage) as an `Instance` |
| [`stages`](stave_host::stages) | stave-dsp stages by name ([`CATALOG`](stave_host::stages::CATALOG)), loadable like plugins |
| [`opened`](stave_host::opened) | what a tool opens: a bundle or `stage:<name>` ([`Opened`](stave_host::opened::Opened)) |
| [`drive`](stave_host::drive) | [`Driver`](stave_host::drive::Driver): run an `Instance` from a signal and notes, block by block |
| [`response`](stave_host::response) | frequency response by log sweep (H1) |
| [`bench`](stave_host::bench) | the bench: phases, typed reports, the describe dump |
| [`validate`](stave_host::validate) | the validation suite, one file per check |
| [`wav`](stave_host::wav) | WAV in and out (planar `f32`) |

Everything that measures is written against
[`Instance`](stave_host::instance::Instance), so it runs the same on a
loaded VST3, on a catalogued stage, and on a plugin linked into a test
([`PluginHarness`](stave_test::PluginHarness) is an `Instance` too).
The analysis itself is [`stave_measure`]: pure functions over buffers.

A catalogued stage behaves like a plugin whose params are the stage's
settings (`filter/peak` has three), which makes it the quickest thing to
measure:

```rust
use std::path::Path;
use stave_host::config::HostConfig;
use stave_host::opened::Opened;
use stave_host::response::{measure, ResponseConfig};

let mut peak = Opened::open(Path::new("stage:filter/peak"), HostConfig::default()).unwrap();
let inst = peak.instance();
let names: Vec<String> = inst.params().into_iter().map(|p| p.name).collect();
assert_eq!(names, ["hz", "db", "q"]);

inst.set_param("db", 6.0).unwrap();
let cfg = ResponseConfig { seconds: 0.5, ..ResponseConfig::default() };
let left = &measure(inst, &cfg)[0];
assert!((left.gain_at(1_000.0).unwrap() - 6.0).abs() < 0.1);
assert!(left.gain_at(100.0).unwrap().abs() < 0.2);

// The names `stage:NAME` takes include these.
let catalogued = |n: &str| stave_host::stages::CATALOG.iter().any(|s| s.name == n);
assert!(["filter/peak", "coupler", "circuit/transformer"].into_iter().all(catalogued));
```

## stave-bench: the agent API

`stave-bench` is the headless host. Its stdout carries only the report:
anything a plugin prints is redirected to stderr. Malformed arguments
are errors that name the flag (exit status 2).

**Start with `--describe`.** It prints, as JSON, the identity and load
trace of a VST3, its layout, latency and tail, every param (name, unit,
range, default, value, steps and their names, flags), the presets and
the taps. The param names are what `--set` takes; `--list` is the same
as text.

```sh
# What can I set on this thing?
stave-bench build/StaveTilt.vst3 --describe | jq '.params[].name'

# Why won't it load? Stream the load trace as it happens.
STAVE_HOST_TRACE=1 stave-bench Some.vst3 --list

# A configuration, kept as a 32-bit float WAV to listen to or diff.
stave-bench build/StaveTilt.vst3 --seconds 4 --set Tilt=3 --out take.wav

# Distortion at a level: gain, THD, THD+N and harmonics, per channel.
stave-bench Some.vst3 --signal sine:1000:-6 --json | jq '.output.tone'

# Is the build sane? Exit status 1 when any check fails.
stave-bench build/StaveTilt.vst3 --validate || echo BROKEN

# Frequency response at several input levels (a fresh instance each).
stave-bench stage:filter/peak --response
```

`stage:NAME` opens a catalogued stave-dsp stage instead of a bundle
(`stage:filter/peak`, `stage:coupler`, `stage:circuit/transformer`, …);
an unknown name lists the catalogue. `--notes KEY@ON:OFF[:V],...`
plays sample-accurate notes, which is how an instrument is driven.

**The default run** times its phases (latency, the timed run, silence
and, for an effect, a step), each with its frames and wall time, and
reports CPU per block against the realtime budget (mean, median, p95,
p99, max), latency (reported against the measured impulse onset and
peak, per output channel), and the output (levels, non-finite samples,
the silence residual, DC, and for a sine a coherent tone analysis).
The plugin's taps are read right after the timed run.

**`--validate`** runs the validation suite, each check on fresh
instances, and prints JSON with per-check timing. Checks hard-fail only
the unambiguous; judgement calls are `info` with the numbers, and a
check with nothing to test is `not_applicable`, never a vacuous pass.
A message the plugin logs at `Fatal`, `HostMisbehaving` or
`PluginMisbehaving` severity fails the check it was logged in. The suite, in order:

```rust
let names: Vec<&str> = stave_host::validate::ALL.iter().map(|c| c.name).collect();
assert_eq!(names, [
    "reproducible",      // two instances, identical input, bit-compared
    "no_nan_inf",        // a full scan of the output
    "param_sweep",       // every sweepable param to its ends while processing
    "params_glide",      // param moves glide rather than step
    "self_noise",        // silent input: no hiss, hum, DC or denormal storm
    "silence_tail",      // quiet within the reported tail and latency after input stops
    "deadline_headroom", // p99 block cost against the realtime budget
    "state_roundtrip",   // save, load into a fresh instance, save: identical bytes
    "solver_converges",  // circuit solvers end below their iteration cap
    "channels_linked",   // identical input on every channel, identical output
    "channels_split",    // inputs that then differ slightly stay close
    "bypass_honest",     // bypassed output is the latency-delayed dry input
]);
```

## stave-play: hear and watch it

```sh
cargo run --release -p stave-play -- build/StaveTilt.vst3
```

`stave-play` opens a plugin with its own editor, bound to the playing
instance (a third-party plugin's editor included, created from its
controller); off macOS it shows no editor yet. A keyboard panel takes the mouse or the computer keys and
sends notes to the plugin while a built-in sine bank feeds its inputs,
so an effect is playable; sample pads fire one-shots. Sample rate and
block size are live in the top bar (`--sample-rate`, `--block-size` set
them at launch; `--set NAME=VALUE` sets a param).

Its tabs are the plugin's editor, measured response profiles of the
plugin as currently set, the live response from its taps, a
spectrogram, scope, spectrum, meters, phase, the raw telemetry
(debugger), the validation suite run one check per frame, a perf view
(load track against the budget, deadline misses, a sampled flamegraph)
and the debug log.

On macOS it captures a picture and exits after the window settles
(about 90 frames), so an agent can see the editor:

```sh
stave-play build/StaveTilt.vst3 --screenshot window.png --note 60
stave-play build/StaveTilt.vst3 --screenshot face.png --editor-only
```

`--editor-only` has a stave plugin's face write its own picture,
offscreen. `--face-trace PATH` records every pointer and key event the
face sees: a session that replays as a test
([`Trace`](stave_face::Trace), [`FaceDriver`](stave_test::FaceDriver)).

## stave-events: check a DAW session

With `STAVE_EVENTS` set, every instance of a stave plugin appends its
event trace (lifecycle, gestures, restores, presets, faults) to
`<log dir>/<plugin id>-<pid>-<n>.events`. `stave-events FILE...` checks
each file against the protocol's laws, prints the violations, and exits
1 if any file breaks one:

```sh
STAVE_EVENTS=1 open -a "Your DAW"          # then play, automate, save, quit
cargo run -p stave-telemetry --bin stave-events -- target/logs/*.events
```

The laws are [`check`](stave_telemetry::events::check) and
[`quiescent`](stave_telemetry::events::quiescent), and the file is the
text of a [`Trace`](stave_telemetry::events::Trace), one
`index @block event` record per line:

```rust
use stave_telemetry::events::{check, quiescent, Trace};

let session: Trace = "\
0 @0 prepare 48000 256
1 @3 begin 1
2 @3 perform 1 0.75
3 @4 end 1
4 @9 release
".parse().unwrap();
assert!(check(&session).is_empty());
assert!(quiescent(&session).is_ok());

let stray: Trace = "0 @0 perform 1 0.5\n".parse().unwrap();
assert_eq!(check(&stray)[0].to_string(), "event 0: an edit outside its gesture");
```

## Hosting behaviours worth knowing

- The class instantiated is the factory's "Audio Module Class"
  (factories often list the controller first).
- Plugins with a separate controller (the commercial norm) get it
  created, initialised and connected through `IConnectionPoint`, and
  synced with the component's state.
- Plugins see a transport playing at 120 bpm from time zero.
- Every load step is traced; `STAVE_HOST_TRACE=1` streams the steps to
  stderr as they happen, the only record when a plugin crashes the
  process mid-load.
