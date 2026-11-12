# stave

A from-scratch Rust audio plugin framework + a suite of plugins built
on it + a transparent host for analysing them.

No JUCE, no `nih-plug`, no `clap-sys` — hand-written CLAP and VST3 ABI
bindings. The point is total ownership and a small comprehensible
audio stack, with testability and observability as defaults rather
than disciplines.

## What's in the box

### Plugins (`plugins/`)

Every plugin ships as native macOS `.vst3` + `.clap` bundles from one
cdylib and is held to the same gauntlet: the in-house validation
suite, the Steinberg VST3 validator and clap-validator
(`scripts/validate-formats.sh` runs both locally).

**Stable** — feature-complete, with their own property suites:

| plugin | category | what it is |
|---|---|---|
| Stave Forge | distortion | amp-topology saturation: two clipping stages with interstage memory, slew grit, iron hysteresis |
| Stave Multiband | dynamics | 3-band compressor with Linkwitz-Riley crossover and external sidechain |
| Stave Smear | resonator | Sympathetic Garden — self-tuning resonator field; smear blooms across pitches |
| Stave Eddy | delay | a feedback delay whose read head breathes: four feedback laws, an in-loop diffusion field |
| Stave Tilt | EQ | a one-knob tilt EQ — **the minimal worked example** (~450 lines including its whole test suite) |

**In development** — usable, still moving:

| plugin | category | what it is |
|---|---|---|
| Stave Saga | reverse / reverb | two time machines in series (reverse delays and reverbs) on a 32 kHz clock you can pull; see docs/saga.md |
| Stave Heirloom | reverse / lo-fi | Saga's reverse machines played back off a worn record: platter wobble and a generation-loss wear stage |
| Stave Ribbon | delay | an impossible tape echo: a tape loop modelled in tape coordinates, and an ERASE knob you can turn down |
| Stave Vault | reverb | a synthesized room (live image-source early field) into a nonlinear Hadamard FDN tank |
| Stave Engram | reverb | a reverb with associative memory: attention over remembered grains of your playing shapes the tail |
| Stave Revenant | generative | an echo-state network that learns your playing online and keeps playing when you stop |
| Stave Aeolian | resonator | a sympathetic string harp: twelve plucked-string voices that ring with what you play |
| Stave Anvil | amp | a multiband metalcore amp head: parallel triode cascades, push-pull power, per-band saturation |
| Stave Halo | amp | a high-headroom clean amp whose attack can stay cleaner than its sustain |
| Stave Tolex | cabinet | a guitar cabinet built, not sampled: speaker motor, cone, box and air as a physical model |
| Stave Dynamo | synth | a physical-modelling synth built from machinery that never existed |
| Stave Opal | synth | a 4-operator FM synth |
| Stave Chime | synth | a polyphonic sine/saw synth with a modulation matrix |

### Framework crates (`crates/`)

The contracts they keep are in **docs/architecture.md**.

- **`stave-core`** — the plugin model: `Processor` (the realtime
  half) + `Spec` (the declarations half) composed by
  `StavePlugin<E>`; typed params (`FloatParam`/`BoolParam`/
  `EnumParam`, `#[derive(Params)]`, declaration-owned smoothing and
  ranges), state chunks, events, the `ui!` editor markup.
- **`stave-vst3`** / **`stave-clap`** — the hand-written ABIs; one
  export macro each.
- **`stave-dsp`** — the DSP vocabulary: layered downward-only
  (docs/architecture.md): `kernel` forms (`Chain`, `Loop`,
  `Oversampled2x`, `Router`, `SwapPoint`, `PerChannel`), `primitives`
  (filters, delay lines, envelopes, LFOs, `RandomWalk`, resampling),
  `behavioral` (saturation curves with ADAA, slew, hysteresis),
  `circuit` (device models, solvers and schematic-derived topologies)
  and `structures`. `tests/datasheet.rs` prints the measured
  ns/sample price of every primitive.
- **`stave-debug`** — the `DebugBus`: lock-free probes, controls,
  presets, and typed `Tap` handles that feed every meter and scope.
- **`stave-ui`** — the OS-agnostic view layer (box model, flexbox,
  style tokens, the `Scene` vector vocabulary); **`stave-ui-cocoa`** —
  its macOS backend.
- **`stave-raster`** — a portable, pure-Rust software renderer that
  draws a `Scene` into an RGBA pixel buffer identically on every
  platform (text is left to the OS).
- **`stave-macros`** — `#[derive(Params)]` and `ui!` (re-exported by
  `stave-core` and `stave-ui`; proc-macros need a crate of their own).
- **`stave-measure`** — audio measurement as pure functions: levels,
  spectra, THD, latency, transfer functions, aliasing/IMD, dynamics,
  modulation, zipper, reverb-tail metrics, WAV I/O.
- **`stave-test`** — the shared checks every plugin mounts in ~10
  lines each (validation suite, editor-binding proofs, no-alloc
  guard, perf report, stability check) and, under `tests/`, the
  conformance suites for the DSP vocabulary itself.
- **`stave-host`** — host any VST3 and measure it (below).

### The host (`crates/stave-host`, `tools/`)

One library, two apps — see **docs/host.md** for the full surface:

- **`stave-host`** — host any VST3 (stave-built or third-party),
  drive it, measure it: loading, rendering, the bench and the
  validation suite, as a library.
- **`stave-play`** (`tools/stave-play`) — windowed host: the plugin's
  own editor bound to the live instance, a playable sine bank + MIDI
  keyboard, measured response profiles, and a CPU inspector (load
  track, deadline misses, flamegraph). Headless
  `--screenshot out.png [--note 60] [--editor-only]` captures the window,
  or just the plugin editor.
- **`stave-bench`** (`tools/stave-bench`) — the agent API:
  `--describe` (identity, load trace, every control, probe, preset as
  JSON), offline renders with CPU / latency / THD / silence reports,
  `--validate` (the pluginval-style suite).
- **`stave-storybook`** (`tools/storybook`) — the design system's
  native storybook: every component live and draggable, no plugin
  required; `--screenshot DIR` captures every story page.

## Building

```sh
# Build + bundle + validate + install one plugin (fresh inodes —
# never cp -R over a loaded bundle):
bash scripts/install.sh stave-eddy-vst3 release

# Launch the host on it:
cargo run --release -p stave-play -- \
  build/StaveEddy.vst3
```

## The gates

Every commit runs, in one chain:

```sh
scripts/gates.sh            # tree rules, every test in parallel, doctests (~40 s warm)
scripts/gates.sh affected   # only the packages the change can reach
```

Tests run under [cargo-nextest](https://nexte.st) (`cargo install cargo-nextest`).

CI additionally holds rustc, clippy and rustdoc at zero warnings, and
runs Miri on the ABI tests, Loom on the lock-free protocols, the
Steinberg validator and clap-validator on every push to main.

## Writing a plugin

```sh
bash scripts/new-plugin.sh myplugin
```

That stamps a complete, gate-passing project — declarations, a
passthrough engine, a house-styled editor, and the full colocated
test suite — ready for your DSP. Then read **docs/writing-a-plugin.md**
(the two-halves model, the kernel vocabulary, and why every project
writes its own properties) with **plugins/tilt** open beside it as the worked example.
Its `tests/properties.rs` tells the story of why projects write their own
properties: tilt's first build shipped with its one knob backwards, past
every generic gate — only a measurement caught it.

## License

MIT, see [LICENSE](LICENSE).
