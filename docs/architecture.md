# Architecture

The contracts the code keeps. Anything here is either enforced (a named
test, check or lint) or is the reason a piece is shaped the way it is.
History lives in git; product design lives with each plugin (`plugins/<name>/docs/` in
[stave-plugins](https://github.com/stave-dsp/stave-plugins)).

## Crates

```
stave-macros ─┐
stave-debug ──┼─ stave-core ─┬─ stave-vst3 ─┐
stave-ui ─────┘              ├─ stave-clap ─┼─ examples/tilt, stave-plugins
                             └─ stave-dsp ──┘
stave-ui-cocoa ── stave-vst3 (editor view)
stave-measure ── stave-host ── stave-test, tools/*
```

- **stave-core** — the plugin model: `Processor` (realtime half) and
  `Spec` (declarations half) composed by `StavePlugin<E>`; typed params;
  state; events; the `ui!` editor markup; `wrapper`, the machinery both
  format wrappers share.
- **stave-dsp** — the DSP vocabulary (below). Depends on nothing in the
  plugin model: it never sees hosts, params or events.
- **stave-vst3 / stave-clap** — hand-written ABIs. A plugin exports with
  one macro each; the macro implements `Vst3Ids` / `ClapEntry` on the
  engine and the blanket `Vst3Export` / `ClapExport` impls lift it onto
  `StavePlugin<E>`.
- **stave-debug** — the `DebugBus`: declared `Tap`s, probes, the scope
  and capture buffers, breakpoints, the plugin's recent log messages.
  Lock-free; zero cost unattached.
- **stave-ui / stave-ui-cocoa** — OS-agnostic layout and style tokens;
  the macOS renderer.
- **stave-measure** — measurement as pure functions over buffers and
  signals.
- **stave-host** — loads any VST3 (or any stave-dsp module) as an
  `Instance`; bench, validation suite, rendering. `tools/stave-bench` and
  `tools/stave-play` are its CLI and GUI.
- **stave-test** — shared plugin checks (`validation`, `editor`,
  `no_alloc`, `perf`, `stability`) and, under `tests/`, conformance
  suites for the framework itself.

## stave-dsp

Folders name the modeling method; dependencies point down only
(`scripts/check-view-bindings.sh` enforces the kernel's).

| layer | contents | may use |
|---|---|---|
| `kernel/` | the composition forms | nothing below |
| `primitives/` | filters, control signals, delays, oscillators, resampling, deterministic randomness | kernel |
| `behavioral/` | fitted or by-ear shapes: curves with ADAA, slew, hysteresis, nonlinear RC | kernel, primitives |
| `circuit/` | devices, solver methods and topologies derived from schematics | kernel, primitives, behavioral |
| `structures/` | `Chain` (sequence with addressable slots), `ModMatrix` | all of the above |

### The node contract

Every processor implements `Stage`: `prepare(rate)`, `reset()`,
`tick(x)`, optionally `process_block`, and reports `latency_samples()`
and `tail_samples()`. Leaves and composites present the same surface.

- **Rate contract.** `prepare` receives the rate the stage actually
  runs at, and derives every coefficient from it. Inside an oversampler
  that is the oversampled rate (`tests/invariance_conformance.rs`).
- **Preconditions are asserted, not repaired.** An input outside a
  method's domain (`Q ≤ 0`, a non-positive rate, a pot past its travel)
  is a caller bug: `debug_assert!`, never a silent clamp. Clamps that
  remain are limits of the method itself — a bilinear corner below
  Nyquist, a one-pole no faster than one sample, a read position
  inside the buffer.
- **Configuration is data.** Multi-value setup takes a config struct
  with named fields (`FilterSpec`, `AdsrConfig`, `RcClipperConfig`,
  `PowerSupplyConfig`, …); variants are enums (`AdaaOrder`, `Bias`,
  `LevelPolicy`), never clamped integers.

### Composition forms (`kernel/`)

| form | type | law (pinned in `tests/kernel_properties.rs`) |
|---|---|---|
| Node | `Stage` | one surface for leaf and composite |
| Sequence | `Serial` | prepare cascades; latencies and tails sum |
| Branch | `Par2` + `Compensator` | paths arrive at the join aligned |
| Loop | `Loop` | feedback only through an explicit `NonZeroU32` delay |
| Oversampled | `Oversampled2x` | interior prepared at k·fs; latency divides back |
| Lanes | `PerChannel` + `ChannelLayout` | lanes never mix or cross-index |
| Control | `Source`, `Route`, `Router` | routes displace from the knob's base value |
| Voices | `VoiceRack`, `Voice`, `VoiceEvent` | fixed pool under a declared steal policy (`tests/voice_properties.rs`) |

`VoiceEvent` is the kernel's own note vocabulary; engines translate
`stave_core::NoteEvent` into it.

### The circuit layer (`circuit/`)

SPICE practice: `devices/` (model cards — Koren and Dempwolf triodes,
Shockley diodes, the Jiles–Atherton magnetic core), `methods/` (load
lines, Newton with junction limiting and backtracking, wave digital
filters, Wright-ω closed forms), `topologies/` (common cathode, triode
stage, push-pull pair, power supply, coupler, tone stack, diode clipper,
op-amp diode clipper, transformer). Topologies are checked against
ngspice waveforms rendered by `scripts/ngspice-render.py`
(`stave-test/tests/matches_ngspice.rs`); what SPICE has no model for
(the magnetic core) against SciPy solutions of its equations from
`scripts/ja-reference.py` (`matches_reference.rs`). Dynamic components
must converge to the reference as the step shrinks, and every Newton
solve must converge at the host rate (`solver_conformance.rs`).
Physical behavior belongs to the framework; a product that wants
exaggeration owns it in its own engine.

## The plugin model (`stave-core`)

- **Engine = `Processor` + `Spec`.** `Processor::new(params)` builds
  everything; `activate(env)` derives every coefficient from the
  environment; `process(ctx)` is realtime and allocation-free
  (`stave_test::no_alloc`). `Spec` declares descriptor, port configs,
  params, editor, presets, state version and migrations.
- **Params are the spine.** A typed table (`#[derive(Params)]`) owns
  ranges, tapers and smoothing; engines read `p.x.value()` or a ramp,
  never a cast-and-clamp copy (`check-plugin-rules.sh`).
- **Ids are forever; shipped ranges and tapers are frozen.** VST3
  automation stores normalized positions, so re-ranging a shipped id
  remaps users' curves. Change a range with a new id plus
  `Migrations::renamed`/`rescaled`; bump `state_version` whenever
  `migrate()` must reason about a change.
- **Editors are declarations.** `Spec::editor` returns a `UiNode` tree
  (`ui!` markup); widgets bind to params by id and write only through
  host gestures. Bespoke visuals are `Widget::Scene` pictures.

## Format wrappers (`stave_core::wrapper`, stave-vst3, stave-clap)

- Host entry points get a raw instance pointer from any thread. Shared
  fields are atomics (`SharedState`); processor-side state sits in a
  `PluginCell` claimed by an atomic token, so a host protocol violation
  is a refused call, not a data race.
- In-place hosts (input and output aliasing) go through preallocated
  scratch (`ProcessorState`); `AudioBuffer` never aliases.
- Bypass, latency and tail are framework-owned: bypass crossfades to
  latency-matched dry.
- The wrapper ABIs run under Miri (`tests/wrapper_abi.rs`,
  `tests/table_abi.rs`); the core and debug protocols are Loom-modelled.

## Diagnostics (`stave_core::diagnostics`)

- Every refused host call is a `CallError`, logged as `"<call>: <error>"`
  before the wrapper returns the format's failure code. Answers are not
  errors: `kNoInterface`, a failed bus negotiation, text that does not parse.
- The audio thread only counts (`ProcessFault`: concurrent process, block
  too long, dropped events, …); the next main-thread call reports the counts
  — CLAP asks for `on_main_thread`, VST3 reports from lifecycle and state
  calls.
- Sinks: the plugin's log file (`log_dir()/<plugin id>.log`;
  `$STAVE_LOG_DIR`, which cargo points at `target/logs`), the CLAP host's
  `clap.log`, and the debug bus's message ring, which stave-host reads
  (`Instance::messages`).
- A panic never reaches the host: every entry point that runs plugin code
  goes through `PanicGuard` (`catch_unwind`). The panic is logged as
  `Fatal` with its location; the instance is disabled — process writes
  silence and reports an error, every later call takes its refusal, and
  the plugin's state is never touched again until teardown, whose
  destructor also runs under `catch_unwind`.
  Release builds unwind for this.

## Observation (`stave-debug`)

Engines declare `Tap`s and publish through the handles; wire names are
data on the declaration, never strings at publish sites
(`check-view-bindings.sh`, G6). Publishing needs a `&DebugBus`, so an
unattached engine cannot reach the path.

## Verification

| what | where |
|---|---|
| zero rustc / clippy / rustdoc warnings | CI `warnings` and `clippy` jobs; policy in `[workspace.lints]` |
| kernel and voice properties | `stave-dsp/tests/` |
| circuits match ngspice or an independent ODE solve | `stave-test/tests/matches_{ngspice,reference}.rs` |
| every solver converges at 48 kHz | `stave-test/tests/solver_conformance.rs` |
| VST3/CLAP struct layouts match the SDK headers | `const` asserts in each `sys.rs`; `scripts/abi-layouts.sh` re-measures |
| every plugin passes Steinberg's validator and clap-validator | `scripts/validate-formats.sh` (CI) |
| rate, block-size and bit-exactness invariants | `stave-test/tests/*_conformance.rs` |
| every plugin: validation suite, editor bindings, no-alloc, perf, stability | each plugin's `tests/` via `stave_test` (`examples/tilt/tests/` here) |
| unsafe core | Miri and Loom jobs |
| tree rules | `check-plugin-rules.sh`, `check-view-bindings.sh`, `check-file-sizes.sh` |

## Conventions

- One concept per file, named for what it owns; a folder is a boundary
  whose `mod.rs` states the contract. File size is a signal
  (`check-file-sizes.sh` prints over ~1000 lines), never a gate.
- f64 for accumulators, coefficients, phase and anything persistent;
  f32 at audio-rate interfaces.
- Time constants are rate-derived (`1 − e^(−1/(τ·fs))`), never bare
  per-sample coefficients.
- Bounded queues never drop silently: saturating counters, surfaced.
- Randomness is seeded (`primitives::random`): a render reproduces from its
  seed.
