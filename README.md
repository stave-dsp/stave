# stave

A from-scratch Rust audio plugin framework + a transparent host for
analysing plugins.

No JUCE, no `nih-plug`, no `clap-sys` — hand-written CLAP and VST3 ABI
bindings. The point is total ownership and a small comprehensible
audio stack, with testability and observability as defaults rather
than disciplines.

The plugins built on it live in
[stave-plugins](https://github.com/stave-dsp/stave-plugins); the website
in [stave-dsp.github.io](https://github.com/stave-dsp/stave-dsp.github.io).

## A plugin

One dependency, one import, one trait, one macro:

```rust
use stave::prelude::*;

#[derive(Params)]
pub struct GainParams {
    pub gain: FloatParam,
    pub bypass: BoolParam,
}

impl Default for GainParams {
    fn default() -> Self {
        GainParams {
            gain: FloatParam::new(ParamId(1), "Gain", Range::linear(-24.0, 24.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(10.0)),
            bypass: BoolParam::bypass(ParamId(2)),
        }
    }
}

pub struct Gain {
    params: Arc<GainParams>,
    gain: Smoother,
}

impl Plugin for Gain {
    const INFO: Info = Info { id: "com.example.gain", name: "Gain", vendor: "Example", ..Info::EFFECT };
    type Params = GainParams;

    fn new(params: &Arc<GainParams>, _telemetry: &Telemetry) -> Self {
        Gain { params: params.clone(), gain: Smoother::new(&params.gain) }
    }

    fn prepare(&mut self, env: &Environment) {
        self.gain.prepare(env);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut gain = self.gain.ramp(block, &self.params.gain);
        block.lanes::<2>().run(|frame| {
            let g = db_to_amp(gain.next());
            for lane in 0..frame.lanes() {
                frame.write(lane, frame.input(lane) * g);
            }
        });
    }
}

stave::export!(Gain);   // a VST3 and a CLAP plugin, from one cdylib
```

The framework owns everything around `process`: click-free bypass,
automation and state, presets, the face's window, denormals, and panics
at the host boundary.

## What's in the box

### The worked example (`examples/tilt`)

Stave Tilt, a one-knob tilt EQ: params, processing and face in one
file, from `use stave::prelude::*`, and the shape every plugin copies.
It ships as native macOS `.vst3` + `.clap` bundles from one cdylib and
passes the in-house validation suite, the Steinberg VST3 validator and
clap-validator (`scripts/validate-formats.sh` runs both locally). The
host's own tests load it through the full VST3 path.

### Framework crates (`crates/`)

The contracts they keep are in **docs/architecture.md**.

- **`stave`** — the one dependency an author takes: the `Plugin` trait,
  `stave::prelude`, `stave::export!` (VST3 + CLAP, class ids derived
  from `Info::id`), and re-exports of the model, the DSP library and the
  design system.
- **`stave-core`** — the plugin model, UI-free: typed params
  (`FloatParam`/`BoolParam`/`EnumParam`, `#[derive(Params)]`,
  declaration-owned smoothing and ranges), `Smoother`/`Steps`, `Block`,
  presets, state chunks, events, the `Controller` a face edits through,
  and the erased runtime the format wrappers drive.
- **`stave-vst3`** / **`stave-clap`** — the plugin side of each
  hand-written ABI (authors never name them: `stave::export!` does).
- **`stave-vst3-sys`** / **`stave-clap-sys`** — the raw ABIs alone
  (types, constants, vtables, IIDs, layout assertions; `no_std`), shared
  by the wrappers and by `stave-host`.
- **`stave-dsp`** — the DSP vocabulary: layered downward-only
  (docs/architecture.md): `kernel` forms (`Chain`, `Loop`,
  `Oversampled2x`, `Router`, `SwapPoint`, `PerChannel`), `primitives`
  (filters, delay lines, envelopes, LFOs, `RandomWalk`, resampling),
  `behavioral` (saturation curves with ADAA, slew, hysteresis),
  `circuit` (device models, solvers and schematic-derived topologies)
  and `structures`. `tests/datasheet.rs` prints the measured
  ns/sample price of every primitive.
- **`stave-telemetry`** — `Telemetry`, the plugin → face/tool
  channel: typed `Tap`s (`#[derive(Taps)]`, read by `TapId`) that feed
  every meter and scope, capture, log messages; lock-free. A plugin gets
  its telemetry in `Plugin::new`.
- **`stave-face`** — the design system: a plugin's editor is a `Face`
  (`Plugin::face`, a pure function of the params) — `Part`s (knobs,
  switches, lamps, displays, `Plate`s) laid out in rows and columns
  inside a `Panel`, on a `Theme` that holds the whole look as data
  (finish, palette, hardware, typography, spacing, coverings). Parts are
  addressed by name (`PartId`), grabbed through `Grab`s.
  `tools/storybook` shows every part on every theme.
- **`stave-ui`** — the OS-agnostic look layer: the layout tree and its
  flexbox solver, and the `Scene` vector vocabulary;
  **`stave-ui-cocoa`** — the macOS platform that shows a face
  (`FaceView`).
- **`stave-raster`** — a portable, pure-Rust software renderer that
  draws a `Scene` into an RGBA pixel buffer identically on every
  platform (text is left to the OS).
- **`stave-macros`** — `#[derive(Params)]` and `#[derive(Taps)]`
  (re-exported by `stave`; proc-macros need a crate of their own).
- **`stave-measure`** — audio measurement as pure functions: levels,
  spectra, THD, latency, transfer functions, aliasing/IMD, dynamics,
  modulation, zipper, reverb-tail metrics (no file I/O: WAV lives in
  `stave-host`).
- **`stave-test`** — the shared checks every plugin mounts in ~10
  lines each (validation suite, face-binding proofs, no-alloc
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
# Build + bundle + validate + install the example (fresh inodes —
# never cp -R over a loaded bundle):
bash scripts/install.sh stave-tilt-vst3 release

# Launch the host on it:
cargo run --release -p stave-play -- build/StaveTilt.vst3
```

## The gates

Every commit runs, in one chain:

```sh
scripts/gates.sh            # tree rules (incl. the glossary), every test, doctests (~40 s warm)
scripts/gates.sh affected   # only the packages the change can reach
```

`scripts/check-vocabulary.sh` holds the code to the glossary
(docs/overhaul.md): retired names and the words it reserves are
rejected.

Tests run under [cargo-nextest](https://nexte.st) (`cargo install cargo-nextest`).

## Releasing

```sh
scripts/release.sh 0.2.0   # version bump, gates, commit, tag v0.2.0, push
```

Plugin suites pin a release tag;
[stave-plugins](https://github.com/stave-dsp/stave-plugins) moves to it with
`scripts/bump-framework.sh v0.2.0`.

CI additionally holds rustc, clippy and rustdoc at zero warnings, and
runs Miri on the ABI tests, Loom on the lock-free protocols, the
Steinberg validator and clap-validator on every push to main.

## Writing a plugin

Read **docs/writing-a-plugin.md** (the `Plugin` trait, params,
processing, the face, and why every project writes its own properties)
with **examples/tilt** open beside it as the worked example. Its
`tests/properties.rs` tells the story of why projects write their own
properties: tilt's first build shipped with its one knob backwards, past
every generic gate — only a measurement caught it.

To start a new plugin, `bash scripts/new-plugin.sh myplugin` in
[stave-plugins](https://github.com/stave-dsp/stave-plugins) stamps a
complete, gate-passing project.

## License

MIT, see [LICENSE](LICENSE).
