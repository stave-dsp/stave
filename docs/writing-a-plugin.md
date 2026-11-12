# Writing a Stave plugin

*The walkthrough for starting a new project. Read
**docs/glossary.md** first — the domain concepts in plain words,
with the web-dev bridges. **Start with `bash scripts/new-plugin.sh
<name>`** in [stave-plugins](https://github.com/stave-dsp/stave-plugins) — it stamps a complete gate-passing project. **Tilt (`examples/tilt`) is the
minimal worked example** (a real, validated plugin in ~450 lines
INCLUDING its whole test suite; its `tests/properties.rs` is the
cautionary tale below) and stave-plugins' forge is the full-scale reference; copy their **shape**,
never their DSP. Everything here is enforced by a gate, a property test,
or a validator — nothing is advisory.*

## A plugin is two halves

**Declarations** — pure data and pure functions of params: who you
are, what the host can negotiate, what the knobs are, what the
editor looks like, what the presets say. No behavior, no state.

**The engine** — the realtime half: a `Processor` whose every piece
is a named kernel form, a declared plane, or a leaf (docs/architecture.md;
anything that can't say which it is gets deleted).

The framework shell composes them: you export
`StavePlugin<YourEngine>` and write no glue.

## The shape

```
plugins/<name>/
  Cargo.toml            # dev-deps: stave-test (+ assert_no_alloc)
  src/
    lib.rs              # THE ROOT — narrated: opens with the tree
                        #   (every file, one line each) so reading
                        #   top-to-bottom then clicking down IS the
                        #   walk; declarations only, < 100 lines
    params.rs           # THE identity table (ids are forever)
    editor.rs           # the face, pure fn of params
    presets.rs          # factory presets, pure fn of params
    engine.rs           # small engines: one file
    engine/             # big engines: a directory of COMPONENTS,
                        #   each file named for the interface it
                        #   defines (forge's, as the example):
      mod.rs            #   the component struct + process(), the
                        #   orchestration in named phases
      strip.rs          #   Strip — the audio lane
      bindings.rs       #   Bindings::apply_changes — params→structure
      modulation.rs     #   Modulation — drivers, routes, displacement
      process.rs        #   process_block — the audio block
      telemetry.rs      #   Telemetry::publish — taps, scope, feeds
      voicing.rs        #   engine vocabulary lives UNDER the engine
  tests/                # the COLOCATED suite (see below)
```

**The tree rules.** `lib.rs` narrates the whole plugin — a reader
who opens it sees every file and both halves before clicking
anywhere. Engine files are real COMPONENTS, never distributed
`impl Engine` blocks: each concept is a type that owns its state
(`Bindings`, `Modulation`, `Telemetry`), passes are functions whose
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
open within the engine — the rule is for behavior components. Names are DOMAIN
words, never positions or private verbs: `primary`/`voicing`/
`post`, not `stage_a`/`filt1`; the audio block is `process` (the
`Processor` contract), never "render" — render is a UI word;
telemetry matches the framework's `attach_telemetry` seam; driver
lanes and detector feeds are named constants, never bare indices.
Milestone tags (K2, G6…) stay out of code comments — provenance
lives in commits and docs.

**The growth rule.** The scaffold's single-file engine is a
STARTING POINT, not a destination: when the second concept arrives
(a modulator joins a delay, a splitter joins a compressor), the
engine becomes `engine/` components THAT DAY — drift happens in the
gap between "small enough" and "should have split already".

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
   on distributed `impl Engine` blocks, milestone tags in project
   comments, and `plain() as f32` reads. A large single-file
   engine is a printed SIGNAL, never a failure — line count is a
   proxy, and the split decision is human.
Every gate check exists because its violation actually happened; a
convention without at least a gate is a deferred failure.

Project-specific tools and tests live HERE, never in `tools/` or
`crates/stave-test/tests/`. File sizes are SIGNALS, not gates
(docs/architecture.md): the size script prints
what's over threshold; you decide on concept seams.

## Performance

- **The perf report** (`stave_test::perf::perf_report`,
  colocated as `tests/perf.rs` like the no-alloc check): every project
  declares its representative configurations. It skips in debug
  builds and MEASURES under
  `cargo test --release -p <crate> --test perf -- --nocapture` — run
  it when touching DSP. It DOCUMENTS, it does not block (a perf gate
  on desktop hardware cries wolf): the printed table (median
  ns/frame, % of budget, spread) is the living record — update the
  baselines in the test header when they legitimately move. The
  stave host's CPU inspector is the live view of the same story.
- **The noise doctrine**: single-run frame timings carry ±5–8%
  noise on a desktop — warm up, repeat, take the median, report the
  spread. Never conclude from one run (a phantom 20% regression has
  already cost an investigation).
- **The `cpu` probe**: the VST3 wrapper times `process` every block
  and publishes duration/budget as the `cpu` probe — any editor can
  mount a `Meter{probe:"cpu"}`, and a glance answers "is something
  suddenly expensive."
- **Profiling**: attach Instruments (or `samply record`) to the
  release perf report — it is a clean hot loop of exactly the engine:
  `cargo test --release -p <crate> --test perf` and sample that
  process. Flag-level tuning is DONE (thin LTO + codegen-units 1 +
  apple-m1 baseline, measured; fat LTO measured and rejected — see
  .cargo/config.toml); the remaining frontier is algorithmic, and
  the rule is: schedule it when a measured need shows up.

## The declarations half

- `lib.rs`: the `Spec` impl — `DESCRIPTOR`, explicit `CONFIGS` (the
  negotiation SET, preference order; state it even when it's the
  default), `params()`, `face()`, `presets()` — plus the two export
  macros. Nothing else.
- `params.rs`: every param with a stable id. **Ids are forever** —
  never reuse, never renumber (docs/architecture.md). Declared
  smoothing (`with_smooth`) for audio-rate gain paths; engine-owned
  motion only when the glide IS the instrument (document which).
  Selector labels live here — the face and the host read the same
  declaration.
- `editor.rs`: the **face** — a pure function of params returning a
  `Face` (`use stave_core::face::prelude::*`). Parts (`Knob`,
  `Footswitch`, `Toggle`, `Selector`, `Lamp`, displays, `Title`) bind
  to params with `p.x.bind()`; `row`/`column`/`stack` lay them out;
  `Plate`s group them and a `Panel` is the enclosure. The `Theme` is
  the whole look — parts never mint a colour, a font or a coordinate.
  `examples/tilt/src/editor.rs` is the worked example and
  `tools/storybook` shows every part on every theme. Paged views are
  declared: a `Pager` part plus `pages(key, …)` overlays the pages in
  one rect. A bespoke display starts IN YOUR PROJECT as your own
  `Part` that paints a `Scene` from the values its `Frame` reads; the
  catalogue is for second-consumer graduates.

## The engine half

Map every piece to its form before writing it (forge's
`engine/mod.rs` header is the worked example):

| You need | You use |
|---|---|
| serial audio path | `chain!` — the declaration's shape IS the signal path; `.slot` is the StageId, `.slot()` the typed cell (swappable slots stay swap-aware). `Serial`/`Chain` underneath |
| another clock domain (oversampling) | `Oversampled2x` presets — never a rate literal |
| per-channel state | `PerChannel` — lanes never cross-index |
| feedback | `Loop` (explicit delay, required) |
| detectors/LFO → knob displacement | `Router`/`ModMatrix` — routes as data, displace-from-base |
| param change detection | `Watch` / `ModalWatch` (poll unconditionally, gate by mode) — and RESET EVERY WATCH IN `activate()`: a re-prepare wipes rate-derived coefficients and param-applied structure the params can't see |
| structural switches | `SwapPoint` (moves, never allocs) |
| latency-matched dry | `Compensator` |
| M/S or fold-down | the kernel adapters (`ms_encode`/`downmix_51`) |
| meters/viz out | declared `Tap` handles — never name strings |
| read a knob (block rate) | `p.x.value()` / `p.sel.index()` — typed, BOUNDED BY THE DECLARATION; never restate a range at a read site (a clamp beside a read is a drift hazard) |
| the block loop's plumbing | `ctx.lanes::<MAX>()` — channel clamping, lane reads/writes, undriven-output zeroing, block peak metering, all framework-owned; the engine keeps only the per-lane-count DECISION and the DSP |

`run()` is an orchestration script: **bind → render → observe**.
Block-rate decisions in bind, per-sample work in render, publishing
in observe. If run() reads a field directly, something is mislayered.

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
  duck + AuxIn contract, smear's garden and surround-lift bit-exactness,
  eddy's field topology properties. A project without its own
  property tests is not done.
- `validation.rs` — `stave_test::validation::assert_passes(
  stave_host::entry_points!(your_crate))`: stave-host's validation
  suite through your VST3 exports. Passing it is the price of
  admission to a plugin suite.
- `editor.rs` — `stave_test::face::binds_resolve::<YourPlugin>(name)`:
  every bind resolves against the table, and stepped gestures bind
  stepped params, continuous ones continuous params.
- `no_alloc.rs` — `stave_test::no_alloc` with YOUR wiggle list:
  sweep the params that rebuild tables and switch structure, not
  just the easy trims. The test binary installs `AllocDisabler`.

Shared machinery lives in the `stave-test` LIB (harness, validation,
face bindings, no-alloc check); `crates/stave-test/tests/` holds the
framework-level checks and the kernel/primitive conformance suites — they
test the vocabulary, not you.

**Refactors of shipped DSP use the bit-pin method**:
write a throwaway FNV-hash pin over real configs BEFORE the change
(include automation and mid-render structural flips), prove
bit-exactness after, delete the scratch before commit. If bits are
SUPPOSED to move, say so in the commit and re-pin.

## The gates

Before every commit: `scripts/gates.sh` (tree rules, every test in
parallel under cargo-nextest, doctests), or `scripts/gates.sh affected`
for only the packages the change can reach.

Ship = both bundle formats validate clean: Steinberg validator 47/0
and clap-validator 0-fail (CI runs both on push; the local recipes
are in `.github/workflows/soundness.yml`). New ABI-touching tests
also run under Miri.
