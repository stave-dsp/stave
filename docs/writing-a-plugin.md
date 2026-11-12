# Writing a Stave plugin

*The walkthrough for starting a new project. Read
**docs/glossary.md** first — the domain concepts in plain words,
with the web-dev bridges. **Start with `bash scripts/new-plugin.sh
<name>`** in [stave-plugins](https://github.com/stave-dsp/stave-plugins) — it stamps a complete gate-passing project (the
plugins move to this API in the overhaul's phase P). **Tilt
(`examples/tilt`) is the worked example**: params, processing and face
in one file from `use stave::prelude::*`, and a test suite whose
`tests/properties.rs` is the cautionary tale below. Copy its **shape**,
never its DSP. Everything here is enforced by a gate, a property test,
or a validator — nothing is advisory.*

## A plugin is one trait

A plugin is a type that implements `stave::Plugin`, beside a params
struct, in a crate that ends with `stave::export!(MyPlugin);`. That
crate depends on `stave` alone and starts with `use stave::prelude::*;`
— if a plugin needs anything beyond the prelude, that is a framework
bug to fix, not an import to add.

```rust
impl Plugin for Tilt {
    const INFO: Info = Info { id: "com.example.tilt", name: "Tilt", vendor: "Example", ..Info::EFFECT };
    type Params = TiltParams;                       // #[derive(Params)] + Default
    fn new(params: &Arc<TiltParams>, telemetry: &Telemetry) -> Self { /* smoothers, taps */ }
    fn prepare(&mut self, env: &Environment) { /* everything rate-derived; may allocate */ }
    fn process(&mut self, block: &mut Block<'_>) { /* realtime */ }
    fn face(p: &TiltParams) -> Option<Face> { /* a pure function of the params */ }
}

stave::export!(Tilt);   // VST3 + CLAP; class ids derived from INFO.id
```

Everything else has a default: `LAYOUTS` (`[Layout::STEREO]`),
`STATE_VERSION`, `reset`, `latency`, `tail`, `presets`, `migrations`,
`save`/`load` (state beyond params, rare). The framework owns the rest:
bypass, automation and state, presets as the host's program list, the
face's window, denormals, panics at the host boundary.

**Declarations are data.** `INFO`, `LAYOUTS`, the params' `Default`,
`face` and `presets` are pure: no behaviour, no state. **The plugin
value is the realtime half**: what `new` builds, `prepare` sizes and
`process` runs — every piece a named kernel form, a declared plane, or
a leaf (docs/architecture.md; anything that can't say which it is gets
deleted).

## The shape

```
<name>/
  Cargo.toml            # [dependencies] stave; [dev-dependencies] stave-test
  src/
    lib.rs              # THE ROOT — params, the Plugin impl, export!;
                        #   small plugins are this one file (Tilt)
    params.rs           # when the table grows: ids are forever
    face.rs             # when the face grows: a pure fn of params
    presets.rs          # factory presets, pure fn of params
    dsp/                # big plugins: a directory of COMPONENTS,
                        #   each file named for the interface it
                        #   defines:
      mod.rs            #   the component struct + process(), the
                        #   orchestration in named phases
      strip.rs          #   Strip — the audio lane
      bindings.rs       #   Bindings::apply_changes — params→structure
      modulation.rs     #   Modulation — drivers, routes, displacement
      taps.rs           #   the #[derive(Taps)] table and its publishing
  tests/                # the COLOCATED suite (see below)
```

**The tree rules.** `lib.rs` narrates the whole plugin — a reader
who opens it sees every file and the `Plugin` impl before clicking
anywhere. Big plugins split into real COMPONENTS, never distributed
`impl` blocks: each concept is a type that owns its state
(`Bindings`, `Modulation`, its taps), passes are functions whose
**signature is the access list** (what a pass may touch is its
arguments, not hidden `self` reach-through), and `process()` is the
one orchestration point where the data flow is visible. **File
names follow the interfaces they define** — a file name guides
navigation; it is never a substitute for a concrete type or
function that makes the composition obvious. **Component state is
private to its file; ports are methods; dependencies are
arguments** — siblings never reach into each other's fields, and
the parent module exports composition, never a grab-bag of shared
constants (a constant lives with the structure that knows it:
region factors on the strip, selector positions beside the
selector). Data RECORDS the passes operate on (`Strip`) may stay
open — the rule is for behavior components. Names are the glossary's
words (docs/overhaul.md, enforced by `scripts/check-vocabulary.sh`),
never positions or private verbs: `primary`/`voicing`/`post`, not
`stage_a`/`filt1`; the audio block is `process`, never "render" —
render is a pixel word; taps are a `#[derive(Taps)]` table built in
`Plugin::new`; driver lanes and detector feeds are named constants,
never bare indices. Milestone tags stay out of code comments —
provenance lives in commits and docs.

**The growth rule.** The single-file plugin is a STARTING POINT, not a
destination: when the second concept arrives (a modulator joins a
delay, a splitter joins a compressor), the processing becomes `dsp/`
components THAT DAY — drift happens in the gap between "small enough"
and "should have split already".

**The enforcement ladder.** Rules live at the strongest rung that
can hold them, and get pushed UP when a stronger one opens:
1. **Types** — the failure is unwritable: `Loop` demands a
   `NonZeroU32` delay; `PerChannel` lanes can't cross; `value()`
   can't drift from its declaration; private component fields make
   guts-reaching a compile error.
2. **Property tests** — the promise is derived and fails the build.
3. **Gates** (`scripts/check-plugin-rules.sh`, in the chain and CI) — for
   exactly what Rust cannot express: comment content, file
   organization, the choice between two type-identical APIs. Fails
   on distributed `impl` blocks, milestone tags in project
   comments, and retired names (`scripts/check-vocabulary.sh`). A
   large single file is a printed SIGNAL, never a failure — line
   count is a proxy, and the split decision is human.
Every gate check exists because its violation actually happened; a
convention without at least a gate is a deferred failure.

Project-specific tools and tests live HERE, never in `tools/` or
`crates/stave-test/tests/`. File sizes are SIGNALS, not gates
(docs/architecture.md): the size script prints
what's over threshold; you decide on concept seams.

## Performance

- **The perf report** (`stave_test::perf::perf_report`, mounted by
  `plugin_checks!` in `tests/checks.rs`): every project declares its
  representative configurations. It skips in debug builds and MEASURES
  under `cargo test --release -p <crate> --test checks perf -- --nocapture` — run
  it when touching DSP. It DOCUMENTS, it does not block (a perf gate
  on desktop hardware cries wolf): the printed table (median
  ns/frame, % of budget, spread) is the living record — update the
  baselines in the test header when they legitimately move. The
  stave host's CPU inspector is the live view of the same story.
- **The noise doctrine**: single-run frame timings carry ±5–8%
  noise on a desktop — warm up, repeat, take the median, report the
  spread. Never conclude from one run (a phantom 20% regression has
  already cost an investigation).
- **The `cpu` tap**: the VST3 wrapper times `process` every block
  and publishes duration/budget as `WrapperTaps::CPU` — any editor can
  mount a `Meter::unit(WrapperTaps::CPU)`, and a glance answers "is something
  suddenly expensive."
- **Profiling**: attach Instruments (or `samply record`) to the
  release perf report — it is a clean hot loop of exactly the plugin:
  `cargo test --release -p <crate> --test checks perf` and sample that
  process. Flag-level tuning is DONE (thin LTO + codegen-units 1 +
  apple-m1 baseline, measured; fat LTO measured and rejected — see
  .cargo/config.toml); the remaining frontier is algorithmic, and
  the rule is: schedule it when a measured need shows up.

## Params

Every param is a field of the params struct (`#[derive(Params)]`),
built in its `Default` with a stable id. **Ids are forever** — never
reuse, never renumber (docs/architecture.md).

```rust
FloatParam::new(ParamId(1), "Tilt", Range::linear(-6.0, 6.0))   // also Range::log, Range::skewed
    .unit(Unit::Db).smooth(Smooth::ms(30.0)).group("Tone");      // .default(x): else the value nearest 0
BoolParam::new(ParamId(4), "Hold", false);
BoolParam::bypass(ParamId(3));                                   // the one way to declare bypass
EnumParam::<Mode>::new(ParamId(5), "Mode", Mode(0));             // param_enum! declares Mode and its labels
```

`p.tilt.value()` reads any param in its natural type (`f32`, `bool`,
the enum) — bounded by the declaration; never restate a range at a read
site. A per-frame glide is a `Smoother` built from the param
(`Smoother::new(&p.tilt)` in `new`, `prepare(env)` in `prepare`,
`ramp(block, &p.tilt)` once per block, `next()` per frame); a stepped
param's changes at their exact frames are `Steps`. Declared smoothing
for audio-rate gain paths; plugin-owned motion only when the glide IS
the instrument (document which). Selector labels live in the
`param_enum!` — the face and the host read the same declaration.
Old sessions load through `migrations()` (`Migrations::default()
.renamed(ParamId(4), ParamId(9))`, rescales), versioned by
`STATE_VERSION`.

## The face

`Plugin::face` is a pure function of params returning a `Face`. Parts
(`Knob`, `Footswitch`, `Toggle`, `Selector`, `Lamp`, displays,
`Title`) bind to params with `p.x.bind()` and read taps by `TapId`
(`Meter::db(MyTaps::LEVEL)`); `row`/`column`/`stack` lay them out;
`Plate`s group them and a `Panel` is the enclosure. The `Theme` is the
whole look — parts never mint a colour, a font or a coordinate. Tilt's
`face` is the worked example and `tools/storybook` shows every part on
every theme. Paged views are declared: a `Pager` part plus
`pages(key, …)` overlays the pages in one rect; content that follows a
selector is `on_step(node, p.algo.bind(), step)`. Every part has a
name (`PartId`: its bound param, its role, or `node.id("…")`), so tests
address parts by name, not coordinates. A bespoke display starts IN
YOUR PROJECT as your own `Part` — `reads()` declares the params and
taps it paints from, `paint` draws a `Scene` from its `Frame`, `grabs`
says where it can be grabbed; the catalogue is for second-consumer
graduates.

## Processing

Map every piece to its form before writing it:

| You need | You use |
|---|---|
| serial audio path | `chain!` — the declaration's shape IS the signal path; `.slot` is the StageId, `.slot()` the typed cell (swappable slots stay swap-aware). `Serial`/`Chain` underneath |
| another clock domain (oversampling) | `Oversampled2x` presets — never a rate literal |
| per-channel state | `PerChannel` — lanes never cross-index |
| feedback | `Loop` (explicit delay, required) |
| detectors/LFO → knob displacement | `Router`/`ModMatrix` — routes as data, displace-from-base |
| param change detection | `Watch` / `ModalWatch` (poll unconditionally, gate by mode) — and RESET EVERY WATCH IN `prepare()`: a re-prepare wipes rate-derived coefficients and param-applied structure the params can't see |
| structural switches | `SwapPoint` (moves, never allocs) |
| latency-matched dry | `Compensator` |
| M/S or fold-down | the kernel adapters (`ms_encode`/`downmix_51`) |
| meters/viz out | a `#[derive(Taps)]` table, built in `new` from its `&Telemetry` — never name strings |
| read a knob (block rate) | `p.x.value()` — typed, BOUNDED BY THE DECLARATION |
| a knob per frame | `Smoother::ramp(block, &p.x)` — honours the block's timed changes and session restores |
| the block loop's plumbing | `block.lanes::<MAX>().run(|frame| …)` — channel clamping, lane reads/writes, undriven-output zeroing, block peak metering, all framework-owned; the plugin keeps only the per-lane-count DECISION and the DSP. Whole channels: `block.io()` (an input to read, an output whose `channels_mut()` lends every channel at once) |

`process()` is an orchestration script: **bind → process → observe**.
Block-rate decisions in bind, per-frame work in process, publishing
in observe. If `process()` reads a field directly, something is
mislayered.

Rules that are not negotiable: no allocation, locks, or blocking in
`process` (the colocated no-alloc check enforces it); dense
cross-coupled cores are LEAVES, not forced combinators; f64 where
recursion or exactness demands it (`Coupler`, the adapters) and f32
elsewhere; every numeric threshold carries the measured value it protects.

## The colocated test suite

Every project ships `tests/` with at least:

- **Your project's OWN property tests — write the first one BEFORE the
  DSP.** The generic gates prove your plugin is *sane*; only a property
  test proves
  it is *right*. The cautionary tale is tilt (`examples/tilt/
  tests/properties.rs`): its first build shipped with the tilt knob
  BACKWARDS — brighter darkened — past validation, the no-alloc
  check, the face bindings, and both industry validators, because no
  generic check can know which way a knob should point. A
  30-second `stave-bench` sine measurement caught it; the sign property
  now makes it unshippable. Measure first, then code to the
  measurement (in stave-plugins): forge's voicing suites, multiband's sidechain
  duck + `Aux` contract, smear's garden and surround-lift bit-exactness,
  eddy's field topology properties. A project without its own
  property tests is not done.
- `checks.rs` — one line, every check a plugin owes:

  ```rust
  stave_test::plugin_checks!(your_crate::YourPlugin, {
      perf: vec![PerfCase { name: "HEAVY", set: &[("Drive", 1.0)] }],
  });
  ```

  stave-host's validation suite through your VST3 exports; no
  allocation in `process` while EVERY parameter sweeps (no wiggle list
  to drift); the perf report; the face's bindings, grabs and interaction
  laws on every page and step (when there is a face); and the host
  behaviours pluginval and clap-validator probe — 0-frame, odd and
  oversized blocks, every sample rate from 8 kHz to 768 kHz,
  prepare/reset cycles that must sound as fresh, seeded random
  automation (smoothed and sample-accurate), a state round trip that
  sounds the same, and determinism. The macro installs the no-alloc
  guard as the file's global allocator, so it gets a file of its own.
- `editor.rs` — your face's own promises, through
  `stave_test::FaceDriver`, which drives a face as a person does:

  ```rust
  let mut ui = FaceDriver::of::<Tilt>();
  ui.press("tilt").move_to(dy(-55.0));    // a quarter of the travel up
  assert_eq!(ui.value("tilt"), 3.0);
  assert!(ui.shows("3.0 dB"));            // the readout, while dragging
  ui.release().exit();
  ui.assert_bracketed();                  // every edit begin → end
  ```

  Aim at a part by name (its param, its role or `node.id("…")`) or an
  exact point; read values, the edits sent to the host, hover, focus,
  cursor, the text shown and the trace that replays the session.
- `golden.rs` (if the look matters) — `stave_test::assert_golden!`
  per interaction state, drawn by the driver with deterministic text;
  `STAVE_BLESS=1` writes the goldens under `tests/golden/`.

Shared machinery lives in the `stave-test` LIB (`PluginHarness` —
drive your `Plugin` in-process, no FFI, with a chosen layout, aux
inputs, transport, sample-accurate changes and state; it is also a
stave-host `Instance`, so every host measurement runs on it);
`crates/stave-test/tests/` holds the framework-level checks and the
kernel/primitive conformance suites — they test the vocabulary, not
you.

**Refactors of shipped DSP use the bit-pin method**:
write a throwaway FNV-hash pin over real configs BEFORE the change
(include automation and mid-block structural flips), prove
bit-exactness after, delete the scratch before commit. If bits are
SUPPOSED to move, say so in the commit and re-pin.

## The gates

Before every commit: `scripts/gates.sh` (tree rules, every test in
parallel under cargo-nextest, doctests), or `scripts/gates.sh affected`
for only the packages the change can reach.

Ship = both bundle formats validate clean: Steinberg validator 47/0
and clap-validator 0-fail (CI runs both on push; the local recipes
are in `.github/workflows/ci.yml`). New ABI-touching tests
also run under Miri.
