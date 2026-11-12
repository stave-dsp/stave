# The Stave glossary — domain concepts, in plain words

*Read this once before reading any plugin. Every term links a
concept you must genuinely learn to the closest idea you may
already know. The right-hand analogies are honest bridges, not
equivalences — where they break down, the entry says so.*

## The audio model (start here)

**Sample** — one `f32`, roughly −1.0..1.0: where the speaker cone
should be at one instant on one channel. This number is the whole
of digital audio; everything else is bookkeeping around it.

**Frame** — one instant across all channels: a stereo frame is two
samples (left, right) that happen at the same moment.

**Lane (channel)** — one signal path through time: lane 0 is left,
lane 1 is right. In Stave, lanes are *who the audio is* — a stereo
effect is the same processing per lane (`PerChannel`), never two
effects.

**Block** — what the host hands you: a grid of frames × lanes
(commonly 64–2048 frames). `process()` is called once per block.
Think request handler: the block is the request, and you must
respond in hard real time — no allocation, no locks, no I/O.

**Sample rate** — frames per second (44100, 48000, 96000…). One
frame at 48 kHz is ~20.8 microseconds. "Per-sample" work happens
tens of thousands of times per second per lane.

**dB (decibel)** — loudness on a log scale, because ears are
logarithmic. Samples need *linear* multipliers: gain = 10^(dB/20),
so 0 dB = ×1 (unchanged), −6 dB ≈ ×0.5, +6 dB ≈ ×2. Knobs speak
dB; the plugin converts once per frame or block.

## The two halves of a plugin

**Declarations** — pure data: who you are, the knob table, the
face, the presets, the port set. No behavior. (≈ a
component's props/schema.)

**Face** — the plugin's editor: a pure function of params returning
a layout tree (`row`/`column`/`stack`) of **Parts** — knobs, switches,
lamps, displays, print — grouped on **Plates** inside a **Panel** (the
enclosure), all drawn on a **Theme**, the whole look as data. Parts
never mint a colour or a font; they ask the theme. (≈ a component
tree on a design system's tokens.)

**Plugin** — the author's type, implementing `stave::Plugin`.
`process(block)` runs per **Block** on the audio thread. (≈ the
reducer: state + input block → output block, under a deadline.)

**Param / the table** — a knob with a FOREVER-stable id, declared
range/taper/unit, and DECLARED smoothing. Plugins read
`p.x.value()` — typed, already bounded by the declaration. Never
restate a range at a read site. (≈ a typed, validated config store;
ids-forever ≈ database column stability across migrations.)

**Smoothing / ramp** — a knob's target can jump; applying jumps
clicks ("zipper noise"). A `Smoother` the plugin owns yields a
per-frame glide toward the target (`ramp(block, &p.x)`, then `.next()`
per frame), with the glide declared on the param (`Smooth::ms(30.0)`)
and host automation points honored at their frames. (≈ a CSS
transition on the value: you declare the duration, the runtime tweens.)

**Bypass** — the host-visible on/off. The FRAMEWORK owns the
click-free crossfade; a plugin declares `BoolParam::bypass(id)`.

## Composition vocabulary (the kernel forms)

**Node / Stage** — the one processing surface: state + prepare +
tick. Leaves (a filter) and composites (a whole chain) present the
same surface. (≈ everything is a component.)

**`chain!` / Sequence** — serial composition, declared so the
code's shape IS the signal path; every slot is a named handle:
`amp.voicing` is its id, `amp.voicing()` the typed cell. (≈ JSX:
structure visible in the source.)

**Lanes / `PerChannel`** — the same node per channel, state
per-lane, lanes never cross. Stereo ≠ MidSide even at the same
count: the type says what the lanes MEAN.

**Oversampled (`Oversampled2x`)** — run a node in a faster clock domain
(oversampling: process at 2×/4× to push distortion aliasing above
the audible band, then come back down). Owns its rate contract and
latency arithmetic. (≈ a different event loop with its own tick
rate; crossing is explicit.)

**Loop** — feedback around a node, legal only through an explicit
delay. (The rule exists because zero-delay feedback is unsolvable
sample-by-sample.)

**Latency / the dry pad (`Compensator`)** — some structures see the
signal "late" (lookahead, linear-phase filters). Parallel paths
must be padded to arrive at joins together, and the host is told
the total so it can shift your track. (≈ clock skew: every join is
a synchronization point.)

## The control plane

**Detector / envelope follower** — a control signal derived from
the program: "how loud is it right now," with attack/release
shaping how fast it believes changes.

**Source** — the one trait for control-signal producers. Exactly
two kinds behind it: program-driven (detectors, LFOs) and
writer-driven (param ramps — they follow the KNOB no matter what
audio plays).

**Route / Router / `ModMatrix`** — wiring as data: which source
displaces which stage parameter, by how much. Rebuilt per block,
inspectable, no allocation.

**Displace-from-base** — knobs set the BASE; modulation displaces
FROM it; they never fight. (≈ controlled components: one owner for
the base value, modulation as a pure function on top.)

**Watch / ModalWatch** — block-rate change detection over params:
"did any of these move since last block?" — with mode-keyed sets
polled unconditionally so a hidden mode can't go stale. (≈ a
dependency array — but by VALUE snapshot, and never gated off.)

**Sense endpoint (`chain.sense(id)`)** — a structure's interior
signal, readable at a declared join. Control wiring (feedback,
detectors) and observation both read this one surface.

## Observation

**Telemetry** — the plugin → editor/tool channel (crate
`stave-telemetry`): taps, capture, the transfer curve, log messages.
The wrapper allocates one per instance and hands it to `Plugin::new`.

**Tap / TapId** — one published value / its id. Declared as a struct
field under `#[derive(Taps)]`, which makes the id a constant
(`HeirloomTaps::SPIN`); the plugin writes `taps.spin.set(v)`, a face or
tool reads by that id. Never strings, never "probe" (that word is the
tooling macro `probe!`). (≈ a metrics endpoint / structured logging:
named channels, sampled by the UI.)

**Scope** — the framework's capture scratch for a face's
oscilloscope-style displays, plus the CPU load meter.

## The realtime rules (why the tests look like that)

**No allocation in `process`** — the audio thread cannot wait on
the allocator. The colocated no-alloc check runs your whole loop
under a guard that ABORTS on any allocation.

**Bit-pin** — refactor discipline: hash the output bits before a
change across real configs, prove they didn't move after. The pin
is the reviewer that can't be argued with.

**Property test** — a test that states a PROMISE the design makes, verified
as the promise itself: the expected value is DERIVED (from physics,
math, or the declaration), never recorded from a run — the crucial
difference from a snapshot, which freezes whatever the code did. A
property test's name reads as the claim ("the echo arrives at the dialed
time", "mix = 0 is bit-identical dry"); its tolerances carry the
measured values they protect; and when an ideal claim proves
physically impossible, the property is domain-qualified, never loosened
into meaninglessness. Kernel forms each own one — that is what
"properties attach to forms; conventions are deferred failures" means:
a convention lives in a comment and dies in a refactor; a property
test fails the build. Every discovered truth becomes one, so the CLASS of a
mistake can't recur silently. (≈ contract/invariant tests, or
property-based tests with a derived property.) Every project ships
the property tests only IT can write — a project without its own is
not done.

**Validators** — Steinberg's and CLAP's official host-side
compliance suites, run on the real bundles. Zero failures is the release threshold.

## Reading Rust here (the two `super`s)

**`super` is a PATH, not a parent class** — Rust has no class
inheritance (no subclassing, no overrides). Modules are namespaces
that follow the file tree, and `super` means "my parent module":
`use super::echo::Echo` in `engine/circuit.rs` is
`import { Echo } from '../echo'`, nothing more. The folder's
`mod.rs` is its `index.ts` — its `mod echo;` lines declare which
files exist and what the folder exports.

**`pub(super)` is EXPORT SCOPE, not a call** — "public only to my
parent module": the engine spine may call `circuit.head(…)`; nothing
outside `engine/` can see it. On METHODS this is the house pattern
(the signature is the access list); on FIELDS it is the smell the
ports pass removed — reaching into another file's state recreates
inheritance's action-at-a-distance without the keyword.

Every relationship in a Stave engine is composition: the engine HAS
its components as struct fields, components receive what they need
as ARGUMENTS, and the only interface mechanism is a trait (≈ a
TypeScript interface, implemented explicitly, never inherited).

## Quick Rosetta (web → Stave)

| You know | Here it's |
|---|---|
| request handler with an SLA | `process()` on a block |
| `import { X } from '../y'` | `use super::y::X` (module path, NOT inheritance) |
| not re-exported from the barrel | `pub(super)` (visible to the parent module only) |
| component props/schema | the declarations half |
| reducer | the plugin's `process` |
| JSX | `chain!` topology / a face's `row`/`column` tree |
| CSS transition | declared param smoothing |
| dependency array | `Watch` (value snapshots, never gated) |
| controlled component | displace-from-base |
| metrics/structured logging | `Tap` handles |
| a second event loop | a `Region` clock domain |
| object pool | the `SwapPoint` bench |
| schema migration discipline | ids are forever |
