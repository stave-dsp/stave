# Telemetry and taps

[`Telemetry`](stave::Telemetry) is the channel from a running plugin to
whoever is watching it: its face, `stave-play`, `stave-bench`, a test.
The plugin publishes named values through typed taps on the audio thread,
lock-free and allocation-free; readers find them by a constant id, never
by a string. The same block carries captured audio, the plugin's recent
log messages and an event trace that a test or a tool checks against the
protocol's laws.

## One block per instance

The format wrapper allocates one [`Telemetry`](stave::Telemetry) per
plugin instance — a fixed-capacity block of atomics, some tens of kilobytes,
shared through an `Arc` by the wrapper, the plugin's taps and the face —
and hands it to [`Plugin::new`](stave::Plugin::new). Everything a plugin
publishes is registered there, once, when the plugin is built.

## Taps

A plugin declares its taps as a struct; `#[derive(Taps)]` gives each
field a [`TapId`](stave::TapId) constant (the field name in capitals) and
a `new(&telemetry)` that registers them all:

```
use stave::prelude::*;

#[derive(Taps)]
pub struct DriveTaps {
    /// Peak level after the drive stage, in dBFS.
    pub level: Tap,
    /// Gain reduction, in dB.
    pub reduction: Tap,
}

let telemetry = Telemetry::new();
assert!((10_000..100_000).contains(&std::mem::size_of::<Telemetry>())); // some tens of kilobytes
assert_eq!(stave_telemetry::MAX_TAPS, 64);
let taps = DriveTaps::new(&telemetry);   // in Plugin::new: registers both, off the audio thread
taps.level.set(-12.0);                   // in process: a few relaxed atomic operations

// A reader names the same constant the plugin does.
assert_eq!(telemetry.value(DriveTaps::LEVEL), Some(-12.0));
assert_eq!(telemetry.value(DriveTaps::REDUCTION), None);   // registered, nothing published yet
assert_eq!(DriveTaps::LEVEL, TapId::new("level"));
```

[`Tap::set`](stave::Tap::set) is a bounds check and a few relaxed atomic
operations (the latest value, a hit count, the running minimum and
maximum) on a slot resolved at registration: no lookup, no lock, no
allocation. Because the plugin and its face name the same constant, a
typo does not compile — it never becomes a meter that silently reads 0.
A tap name holds at most 31 bytes, and a longer one is a compile error.

A block holds [`MAX_TAPS`](stave_telemetry::MAX_TAPS) (64) slots. A tap
that finds no slot — or one made with
[`Tap::detached`](stave::Tap::detached) — accepts writes and drops them,
so plugin code never branches on whether anything is attached.

## Who reads taps

- **The face.** Parts that show live values read taps by id: a
  [`Meter`](stave_face::Meter), a [`Trace`](stave_face::parts::Trace), a
  [`Lamp`](stave_face::Lamp), and any custom part through
  [`Frame::tap`](stave_face::Frame::tap). A part declares the taps it
  reads in [`Part::reads`](stave_face::Part::reads), and the face repaints
  when one moves.
- **Tools and hosts.** `stave-host` reads a stave-built plugin's block
  across the dynamic-library boundary, so `stave-bench --describe` lists
  every tap with its latest, minimum and maximum, and `stave-play` plots
  them. A third-party plugin has no telemetry block, so it has no taps.
- **Tests.** [`FaceDriver::tap`](stave_test::FaceDriver::tap) publishes a
  value the way the plugin would, so a face's meters are tested without
  audio.

```
use stave::prelude::*;

#[derive(Taps)]
pub struct DriveTaps { pub level: Tap }

#[derive(Params)]
pub struct P { pub drive: FloatParam }

let params = Arc::new(P { drive: FloatParam::new(ParamId(1), "Drive", Range::linear(0.0, 1.0)) });
let face = Face::new(
    Theme::graphite(),
    row(vec![Knob::new(params.drive.bind()).node(), Meter::db(DriveTaps::LEVEL).label("OUT").node()]),
);
let mut ui = stave_test::FaceDriver::new(face, params, Vec::new());
ui.tap(DriveTaps::LEVEL, -6.0);   // the meter now reads −6 dBFS
assert_eq!(ui.controller().tap(DriveTaps::LEVEL), -6.0);
```

### The wrapper's taps

Both format wrappers publish a set of taps around every `process` call,
for every plugin, without the plugin doing anything: input and output
peak levels in dBFS with a −120 dB floor (`io_in`, `io_out` and their
`_l` and `_r` channels — the face's meter rails), and `cpu`, the plugin's
process time over the block's realtime budget (0 to 1, and past 1 when a
block runs late). Any face can show them by id:
`Meter::unit(TapId::new("cpu"))` is a load meter.

### A circuit's solver health

A plugin that runs a circuit (`Circuit<Dk>`, `Circuit<Wdf>`, or a
hand-derived topology with Newton counts) publishes how hard each
block's solves worked through [`SolverTaps`](stave::SolverTaps): mean
and peak iterations per sample, cap hits (a solve that ended at its
iteration cap unconverged — anything but zero is a defect), continuation
fallbacks, and the worst residual a solve ended on. Register them in
`Plugin::new`, publish once a block from `process` with the circuit's
`take_health()` (the account since the last call), and `stave-play` plots
them beside the plugin's own taps, `stave-bench --describe` lists their
minimum and maximum, and a face can light a [`Lamp`](stave_face::Lamp)
on `solver_capped` — so a circuit that struggles at some knob setting, on
some material, shows it in a DAW session instead of failing silently.

```
use stave::{SolverTapIds, SolverTaps, Telemetry};
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::{Circuit, Netlist};
use stave_dsp::Stage;

let netlist = Netlist::parse_spice(
    "* clipper
     V1 in 0 0
     R1 in out 2.2k
     C1 out 0 10n
     D1 out 0 DI
     D2 0 out DI
     .model DI D(Is=2.52n N=1.752)
     .control
     wrdata out v(out)
     .endc",
)
.unwrap();
let mut clipper = Circuit::<Dk>::new(&netlist).unwrap();
clipper.drive("V1").unwrap();
clipper.prepare(96_000.0);

let telemetry = Telemetry::new();
let solver = SolverTaps::new(&telemetry, SolverTapIds::DEFAULT); // in Plugin::new

let mut block = [0.5f32; 64];                                    // in process
clipper.process(&mut block);
solver.publish(&clipper.take_health());

assert_eq!(telemetry.value(SolverTapIds::DEFAULT.capped), Some(0.0));
// Two circuits in one plugin: name each one's taps.
const MUFF: SolverTapIds = stave::solver_tap_ids!("muff");
assert_eq!(MUFF.iterations.name(), "muff_iterations");
```

A hand-derived topology's counts are cumulative, so publish the
difference since the last block (`Health::from` takes the
`NewtonStats`; it reports no residual). `crates/stave-test/tests/solver_taps.rs`
drives every topology hot through exactly this path and holds every
`capped` tap to zero.

### A stage's cost

The wrapper's `cpu` tap is the whole plugin; to see which stage the time
goes to, wrap each stage's block in [`CostTaps::measure`](stave::CostTaps::measure).
It publishes the block's wall-clock time in nanoseconds a sample and its
`load`, that time over the sample period (1 is one core's whole realtime
budget), and each slot keeps its maximum, so the worst block of a session
stays on record beside the solver's health. Name each stage's taps with
[`cost_tap_ids!`](stave::cost_tap_ids):

```
use stave::{CostTapIds, CostTaps, Telemetry};
use stave_dsp::physical::tape::{Chowdhury, Machine};
use stave_dsp::Stage;

const TAPE: CostTapIds = stave::cost_tap_ids!("tape");   // tape_ns_per_sample, tape_load
let telemetry = Telemetry::new();
let mut cost = CostTaps::new(&telemetry, TAPE);          // in Plugin::new
cost.prepare(48_000.0);                                  // in prepare

let mut deck = Chowdhury::new(Machine::TC260);
let mut block = [0.1f32; 64];                            // in process
cost.measure(block.len(), || deck.process(&mut block));

assert!(telemetry.value(TAPE.load).is_some());
```

The clock is the monotonic one, read twice a block (no lock, no
allocation). What it measures is what the host's deadline sees,
preemption included; the datasheet and `scripts/icount.sh` measure the
same stages offline and deterministically.

## Capture and the curve

The block also carries a **capture frame**: the latest block of output,
per channel (up to [`CAPTURE_CHANNELS`](stave_telemetry::CAPTURE_CHANNELS)
channels of [`CAPTURE_LEN`](stave_telemetry::CAPTURE_LEN) frames), written
by the wrapper every block, for a face's oscilloscope-style displays
([`Controller::capture`](stave::Controller::capture)). A plugin that wants
to show something else — a shaper's input against its output, so a scope
plots the transfer — calls
[`claim_capture`](stave::Telemetry::claim_capture) and writes its own
pairs through a [`Scope`](stave_telemetry::Scope), which owns the capture
scratch (sized in `prepare`, never on the audio path) and a smoothed load
figure. A plugin can also publish a transfer-curve table
([`publish_curve`](stave::Telemetry::publish_curve)) at block rate.

## Log messages

Each block keeps the instance's recent log messages in a lock-free ring
([`Messages`](stave_telemetry::Messages)): refused host calls, faults the
audio thread counted, caught panics. `stave-host` reads them while the
plugin runs, so a tool shows why a plugin misbehaved without a log file.
(The same messages also go to the plugin's log file and, under CLAP, to
the host's log.)

## The event trace

The block's event ring records what happened to the instance, in order,
as typed [`Event`](stave_telemetry::Event)s: each prepare and release,
every edit gesture (begin, perform, end), every state restore and factory
preset, and every [`Fault`](stave_telemetry::Fault). The face's
controller records every edit; both format wrappers record lifecycle
calls, state loads and faults. Writing is lock-free and allocation-free
from any thread.

Observability doubles as verification: the trace a person reads to see
what a DAW did is the one a test asserts clean.
[`events::check`](stave_telemetry::events::check) holds a trace to the
protocol's laws — a gesture begun once and ended once, edits only inside
their gesture, release only after a prepare, no gap, no fault — and
[`events::quiescent`](stave_telemetry::events::quiescent) asks whether
every gesture was closed by the end.

```
use stave_telemetry::events::{check, quiescent, Event, Recorder};
use stave_telemetry::Telemetry;

let telemetry = Telemetry::new();
telemetry.event(Event::Prepare { sample_rate: 48_000.0, max_block: 512 });
telemetry.event(Event::Begin(7));
telemetry.event(Event::Perform(7, 0.25));
telemetry.event(Event::End(7));
telemetry.event(Event::Perform(7, 0.5));   // an edit outside any gesture

let mut reader = Recorder::new();
reader.drain(telemetry.events());
let trace = reader.trace();
let broken = check(&trace);
assert_eq!(broken.len(), 1);
assert_eq!(broken[0].index, Some(4));      // the stray perform
assert!(quiescent(&trace).is_ok());        // nothing left open

// A trace is text, one record per line, and reads back.
let text = trace.to_string();
assert_eq!(text.parse::<stave_telemetry::events::Trace>().unwrap(), trace);
```

In a real DAW session, set `STAVE_EVENTS=1` and each instance appends its
trace to `<log dir>/<plugin id>-<pid>-<n>.events`. The log directory is
`$STAVE_LOG_DIR` when set, else `~/Library/Logs/stave` on macOS,
`%LOCALAPPDATA%\stave\logs` on Windows and `$XDG_STATE_HOME/stave`
(or `~/.local/state/stave`) elsewhere. Then check the files:

```sh
cargo run -p stave-telemetry --bin stave-events -- ~/Library/Logs/stave/*.events
```

`stave-events` prints each file's violations and exits 1 if any file
breaks a law — so a session in a DAW is a test run.

## Probes and breakpoints: tooling only

The string-keyed [`probe!`](stave::probe) and [`brk_if!`](stave::brk_if)
macros publish a value or arm a breakpoint by name, for harness, bench
and debugging code; `stave-play`'s debugger lists whatever they publish.
An armed breakpoint set to mute on hit pauses the plugin when it fires:
the VST3 wrapper writes silence while the plugin keeps running, so its
state stays observable. Shipping
plugin code publishes through taps, and `scripts/check-view-bindings.sh`
rejects `probe!` in `examples/`.

## Across the library boundary

[`Telemetry`](stave::Telemetry) is `#[repr(C)]` over atomics, with its
layout version, [`ABI_VERSION`](stave_telemetry::ABI_VERSION), as its
first field. `stave::export!` exports a `stave_telemetry_for` symbol
beside the VST3 entry points; `stave-host` looks it up after loading a
bundle and reads an instance's block through it — and ignores a block
whose version differs from its own, noting the mismatch in the load
trace, rather than read a layout it does not know.
