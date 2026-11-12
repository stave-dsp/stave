# The stave host — agent surface

Two faces over one library. `stave-play` is the windowed host (humans:
play, hear, watch). `stave-bench` is the headless CLI (agents: maximal
machine-readable data from ANY VST3 — stave-built or third-party).
They live in `tools/stave-play` and `tools/stave-bench`, thin apps
over the `stave-host` library (`crates/stave-host`), which you can use
from Rust directly.

## The library (`stave_host`)

```text
plugin/    VST3 hosting → LoadedPlugin (an Instance)
instance   the Instance trait — the seam everything measures through
control    one parameter model (Control), lookup by name
note       note events + schedules
render     Driver: run an Instance from a Signal + notes, block by block
measure/   pure analysis over buffers: signals, levels, stats, tone/THD,
           latency, transfer function (H1), compare/null test
response   frequency response by log sweep
bench/     the bench: phases, typed reports, CLI parsing, text output
validate/  the validation suite: one file per check
```

Everything that measures is written against `Instance`, so it runs the
same on a loaded VST3 and on a pure-Rust test double (`testing.rs`).
That is how the bench phases, validation checks and the realtime audio
engine are unit-tested without a plugin or a sound card; the VST3 path
itself is tested end to end by hosting Tilt in-process
(`tests/vst3_host.rs`, via `LoadedPlugin::from_entry_points`).

**Hosting behaviors worth knowing** (all in `plugin/`):

- The class instantiated is the factory's **"Audio Module Class"**
  (factories often list the controller first).
- The executable comes from the bundle's `Info.plist` (CFBundle), and
  `bundleEntry` receives a real `CFBundleRef`.
- **Split-architecture** plugins (separate controller — the commercial
  norm) get their controller created, initialized, connected via
  `IConnectionPoint`, and synced with the component state. Edits made
  through any `ControllerHandle` (the GUI's, from another thread) reach
  the processor through a queue drained by `process`.
- `process` is allocation-free and takes any block length (chunked to
  `max_block`). Anything the host has to refuse (events or parameter
  changes past capacity) is counted in `HostStats`, never dropped
  silently.
- Plugins see a `ProcessContext`: playing, 120 bpm, 4/4.
- Every load step is traced (`PluginInfo.load`, in `--describe`).
  `STAVE_HOST_TRACE=1` streams the steps to stderr as they happen —
  the only record when a plugin crashes the process mid-load.

## stave-play (windowed)

```
cargo run --release -p stave-play -- <bundle.vst3>
```

- **Plugin tab (default)**: the plugin's OWN editor, bound to the
  playing instance (created from its controller — third-party editors
  included). Scales to fit the window.
- **Controls panel**: stave plugins get their grouped debug-bus panel;
  any other plugin gets one slider per writable parameter.
- **Keyboard panel**: mouse or computer keys (z/x octave). Notes go to
  the plugin as note events AND drive a built-in sine bank into its
  inputs, so effects are playable. Sample pads fire one-shots.
- **Topbar**: sample rate + block size are live (change = clean
  reload; the device's real rate is written back).
- **Profiles**: measured frequency-response curves of the plugin as
  currently set — a fresh instance with the live control values, a log
  sweep, H1. Deterministic; the live instance is untouched.
- **Validate**: the validation suite below against fresh instances, one check
  per frame (the window stays live; results land as they finish).
- **Perf**: load track with the budget line and dropout ticks,
  deadline-miss counter, plugin-inner (`cpu` tap) vs host-outer
  time, duration histogram, one-click 5 s pprof flamegraph.
- Other tabs: scope, spectrum, spectrogram, meters, phase, response
  (live H from the taps), debugger (raw bus), debug log.
- **Headless screenshot**: `stave-play <bundle> --screenshot out.png
  [--note 60] [--set name=value ...]` settles ~90 frames, captures the
  window, exits.

## stave-bench (headless, the agent API)

```
stave-bench <bundle.vst3 | module:NAME> [--rate 48000] [--block 256] [--seconds 3]
            [--signal sine[:FREQ[:DBFS]] | impulse | silence | noise[:DBFS]
                      | sweep[:F0:F1[:DBFS]] | wav:PATH]
            [--set name=value ...] [--preset NAME] [--notes P@ON:OFF[:V],...]
            [--out out.wav] [--describe | --list | --validate | --response]
            [--response-levels -36,-24,-12,-6,0] [--json]
```

`module:NAME` runs a stave-dsp module from `stave_host::modules::CATALOG`
(e.g. `module:filter/peak`); its settings are controls, so `--set`,
`--list` and `--validate` work as they do for a plugin.

Malformed arguments are errors that name the flag (exit 2). stdout
carries only the report: anything a plugin prints (JUCE announces its
version) is redirected to stderr.

- `--describe` → JSON: identity (class, vendor, version, SDK,
  controller kind), the load trace, layout, latency, tail, every
  control (name/value/min/max/default/unit/group/steps + names/flags),
  presets, probes, host counters. **Agents start here** — control
  names are what `--set` takes. `--list` is the same as text.
- default run (`--json` for JSON) → phases `latency`, `timed`,
  `silence`, each with frames and wall time:
  * CPU: µs/block, ns/sample, % of realtime (mean/median/p95/p99/max);
  * latency: reported PDC vs measured impulse onset AND energy peak,
    **per output channel**;
  * output: peak/RMS per channel, non-finite sample count, silence
    residual per channel, and for a sine a coherent Goertzel report
    per channel (gain, THD, THD+N, every harmonic in dBc);
  * probes, read right after the timed run (so they describe it).
- `--set` / `--preset` → any control state (plain units). `--notes`
  sends sample-accurate note events; instruments are driven by notes
  (the impulse-latency phase is skipped — no input to put it in).
- `--out out.wav` → the timed run's output, 32-bit float WAV.
- `--validate` → the validation suite (JSON results with per-check timing;
  exit 1 if anything failed). Each check gets fresh instances:
  * `reproducible` — two instances, identical input, bit-compared;
  * `no_nan_inf` — full output scan;
  * `param_sweep` — every sweepable control to min and max while
    rendering (NaN/∞, peaks past +24 dBFS). Nothing to sweep is
    `info`, never a vacuous pass;
  * `silence_tail` — after a burst (an instrument's notes end), quiet
    below −60 dBFS within reported tail + latency + 2 s (`info` if not);
  * `deadline_headroom` — p99 block cost vs budget (pass < 50%, info
    < 100%, fail over);
  * `state_roundtrip` — controls nudged off default, save → load into
    a fresh instance → save: byte-identical, or the session lies;
  * `bypass_honest` — bypass engaged: an effect's output IS its
    latency-delayed dry input (bit for bit; a null residual is
    reported on mismatch), an instrument's is silence.
  Hard-fail only the unambiguous; judgment calls are `info` with
  numbers.

## Recipes

```sh
# What can I set on this thing?
stave-bench build/StaveEddy.vst3 --describe | jq '.controls[].name'

# Why won't it load?
STAVE_HOST_TRACE=1 stave-bench Some.vst3 --list

# Render a config and listen/diff
stave-bench build/StaveEddy.vst3 --seconds 4 \
  --set heat=0.9 --set feedback=0.8 --out /tmp/take.wav

# Distortion at a level
stave-bench Some.vst3 --signal sine:1000:-6 --json | jq '.output.tone'

# Is the build sane?
stave-bench build/StaveEddy.vst3 --validate || echo BROKEN

# Where does the CPU go? stave-play → Perf → capture flamegraph, or:
cargo test --release -p <crate> --test perf -- --nocapture
```
