# Glossary

One word per concept, one concept per word. The terms below are the
framework's vocabulary in code, docs and conversation alike, and this
page is the source `scripts/check-vocabulary.sh` reads: it rejects every
retired name anywhere in the tree and the reserved words on the author
surface. The second half explains the domain in plain words, with
bridges from ideas you may already know.

## Terms

| Term | Means | Never call it |
|---|---|---|
| [`Plugin`](stave::Plugin) | The author's type; the one trait an author implements | Processor, Engine, Spec, StavePlugin |
| [`Info`](stave::Info) | A plugin's identity: id, name, vendor, version, features | Descriptor |
| [`Layout`](stave::Layout) | A supported bus configuration | PortConfig, Arrangement |
| [`Params`](stave_core::params::Params) / Param / [`ParamId`](stave::ParamId) | The table / one parameter / its forever id | Control, store, raw u32 |
| [`value()`](stave_core::params::FloatParam::value) | A param's current value in its natural type | plain, get, index, current |
| normalized ([`Normalized`](stave_core::units::Normalized)) | 0–1 in the param's own taper: the host's and the face's language | |
| [`Smooth`](stave_core::params::Smooth) / [`Smoother`](stave_core::params::Smoother) | A declared glide / the per-plugin state that runs it (one block of it is a [`Ramp`](stave_core::params::Ramp)) | Glide, ContinuousControl |
| [`Environment`](stave::Environment) | Conditions at prepare: sample rate, max block, layout | |
| prepare / reset / process | The plugin lifecycle: prepare may allocate; process is realtime | activate, deactivate, activation |
| [`Block`](stave::Block) | One process call's audio, events and param changes | ProcessCtx |
| frame | One sample position across channels; counts are usize | sample_offset, index, u32 counts |
| sample_rate | Hz, f32, everywhere | sr, fs, rate |
| [`Stage`](stave_dsp::Stage) | A DSP unit: prepare / reset / tick / process / latency / tail | process_sample, process_one, advance |
| tick | Advance a Stage one sample | process (for one sample) |
| latency / [`Tail`](stave::Tail) | Samples of delay / how long output rings after input stops | latency_samples, tail_samples |
| [`Telemetry`](stave::Telemetry) | The plugin → editor and tool channel | debug bus, bus, DebugBus |
| [`Tap`](stave::Tap) / [`TapId`](stave::TapId) | One published value / its id: the plugin writes, the face reads | probe |
| [`Capture`](stave_telemetry::Capture) | Audio snapshots published for displays | Scope, ring |
| [`Face`](stave_face::Face) | A plugin's editor, built from Parts on a Theme | editor tree, UiNode, Widget |
| [`Part`](stave_face::Part) / [`Theme`](stave_face::Theme) / [`Panel`](stave_face::Panel) / [`Plate`](stave_face::Plate) | A face leaf / the look as data / the enclosure / a control group | widget, card, tokens |
| [`Grab`](stave_face::Grab) / [`Gesture`](stave_face::Gesture) | Where a part can be grabbed / what a grab does | Control, Hit, Target |
| [`Controller`](stave::Controller) | What a face reads and writes on its plugin (params, taps, presets, capture): the VST3 meaning | Host, EditorHost, Bridge |
| Host | The DAW; [`stave_host`] hosts plugins | anything else |
| render | Rasterise a Scene into pixels | running audio |
| model | A stave-dsp type named for what it models: the device, the maths or its authors, never how it is built ([where a model goes](crate::howto::model_placement)) | Model, Impl, Processor, Engine, Helper, V2, New, Simple, Fast (as a prefix or suffix) |

`Scope` is a real type with another meaning: the plugin-side scratch
([`Scope`](stave_telemetry::Scope)) that fills a capture and smooths the
load meter. The capture itself is never a scope.

## Retired names

Each name in the first column is gone from the API and from every word
about it. The gate rejects it anywhere in the tree (this page and the overhaul
ledger, which records the history, excepted);
use the second column instead.

| Retired | Use instead |
|---|---|
| `DebugBus` | [`Telemetry`](stave::Telemetry) |
| `debug_bus` | a [`Telemetry`](stave::Telemetry) named `telemetry` |
| `stave_debug` | [`stave_telemetry`] |
| `FaceSpec` | [`Plugin::face`](stave::Plugin::face) |
| `HasFace` | [`Plugin::face`](stave::Plugin::face) |
| `StavePlugin` | [`Plugin`](stave::Plugin) |
| `ProcessCtx` | [`Block`](stave::Block) |
| `PluginDescriptor` | [`Info`](stave::Info) |
| `PluginFeature` | [`Feature`](stave::Feature) |
| `PortConfig` | [`Layout`](stave::Layout) |
| `AuxIn` | [`Aux`](stave::Aux) |
| `AudioBuffer` | [`InputBuffer`](stave_core::InputBuffer) and [`OutputBuffer`](stave_core::OutputBuffer) |
| `ContinuousControl` | [`Smoother`](stave_core::params::Smoother) |
| `ContinuousBlock` | [`Ramp`](stave_core::params::Ramp) |
| `DiscreteControl` | [`Steps`](stave_core::params::Steps) |
| `ReconfigureControl` | [`Deferred`](stave_core::params::Deferred) |
| `continuous_controls` | [`#[derive(Smoothers)]`](stave_macros::Smoothers) |
| `attach_telemetry` | the `telemetry` argument of [`Plugin::new`](stave::Plugin::new) |
| `debug_attach` | the `telemetry` argument of [`Plugin::new`](stave::Plugin::new) |
| `param_table` | [`Plugin::Params`](stave::Plugin::Params) |
| `save_extra` | [`Plugin::save`](stave::Plugin::save) |
| `load_extra` | [`Plugin::load`](stave::Plugin::load) |
| `stave_vst3_export` | [`stave::export!`](stave::export) |
| `stave_clap_export` | [`stave::export!`](stave::export) |
| `EditorHost` | [`Controller`](stave::Controller) |
| `editor_face` | [`Plugin::face`](stave::Plugin::face) |
| `editor_ui` | [`Plugin::face`](stave::Plugin::face) |
| `UiNode` | [`FaceNode`](stave_face::FaceNode) |
| `latency_samples` | [`Plugin::latency`](stave::Plugin::latency) |
| `tail_samples` | [`Plugin::tail`](stave::Plugin::tail) |
| `ControlFlags` | [`ParamFlags`](stave_core::params::ParamFlags) |
| `ControlError` | [`ParamError`](stave_host::param::ParamError) |
| `set_control` | [`Instance::set_param`](stave_host::instance::Instance::set_param) |
| `SettingValues` | [`StageParams`](stave_host::stages::StageParams) |
| `control_of` | [`Param::of`](stave_host::param::Param::of) |
| `event_of` | [`Event::note`](stave::Event::note) |

## The audio model

**Sample.** One `f32`, roughly −1.0 to 1.0: where the speaker cone
should be at one instant on one channel. This number is the whole of
digital audio; everything else is bookkeeping around it.

**Frame.** One instant across all channels: a stereo frame is two
samples, left and right, that happen at the same moment. Frame counts
and offsets are `usize`.

**Lane (channel).** One signal path through time: lane 0 is left, lane
1 is right. Lanes are who the audio is: a stereo effect is the same
processing per lane ([`PerChannel`](stave_dsp::PerChannel)), never two
effects.

**Block.** What the host hands you: a grid of frames by lanes, commonly
64 to 2048 frames. `process` is called once per block. Think of a
request handler with a hard deadline: no allocation, no locks, no I/O.

**Sample rate.** Frames per second (44 100, 48 000, 96 000…). One frame
at 48 kHz is about 20.8 microseconds, so per-sample work happens tens
of thousands of times a second per lane.

**Decibel.** Loudness on a log scale, because ears are logarithmic.
Samples need linear multipliers: gain = 10^(dB/20), so 0 dB is ×1, −6 dB
about ×0.5, +6 dB about ×2 ([`db_to_amp`](stave_dsp::db_to_amp)). Knobs
speak dB; the plugin converts.

## The two halves of a plugin

**Declarations.** Pure data: who you are ([`Info`](stave::Info)), the
param table, the face, the presets, the layouts. No behaviour. (Like a
component's props and schema.)

**The plugin value.** What [`Plugin::new`](stave::Plugin::new) builds,
[`prepare`](stave::Plugin::prepare) sizes and
[`process`](stave::Plugin::process) runs, block by block, on the audio
thread. (Like a reducer: state plus an input block gives an output
block, under a deadline.)

**Face.** The editor: a pure function of the params returning a layout
tree (`row`, `column`, `stack`) of parts (knobs, switches, lamps,
displays, print), grouped on plates inside a panel, all drawn on a
theme. Parts never mint a colour or a font; they ask the theme. (Like a
component tree on a design system's tokens.)

**Param and the table.** A knob with a stable id, a declared range,
taper and unit, and declared smoothing. Plugins read `p.x.value()`,
typed and already bounded by the declaration; never restate a range at
a read site. Ids are forever, like database columns across migrations.

**Smoothing.** A knob's target can jump, and applying jumps clicks
("zipper noise"). A [`Smoother`](stave_core::params::Smoother) the
plugin owns glides toward the target: `ramp(block, &p.x)` once per
block, `next()` per frame, with the glide declared on the param
(`Smooth::ms(30.0)`) and host automation points honoured at their
frames. (Like a CSS transition: you declare the duration, the runtime
tweens.)

**Bypass.** The host-visible on/off. The framework owns the click-free
crossfade; a plugin only declares
[`BoolParam::bypass`](stave_core::params::BoolParam::bypass).

## Composition

**Stage.** The one processing surface: state plus prepare plus tick.
Leaves (a filter) and composites (a whole chain) present the same
surface. (Everything is a component.)

**Sequence and `chain!`.** Serial composition, declared so the code's
shape is the signal path ([`chain!`](stave_dsp::chain)). Every slot is a
named handle: `amp.voicing` is its id, `amp.voicing()` the typed stage.
(Like JSX: structure visible in the source.)

**Lanes.** The same stage per channel, state per lane, lanes never
crossing. Stereo is not mid/side even at the same lane count:
[`ChannelLayout`](stave_dsp::ChannelLayout) says what the lanes mean.

**Oversampled.** Running a stage in a faster clock domain
([`Oversampled2x`](stave_dsp::Oversampled2x)), so distortion's aliases
land above the audible band before coming back down. The form owns its
rate contract and latency arithmetic. (A second event loop with its own
tick rate; crossing it is explicit.)

**Loop.** Feedback around a stage, legal only through an explicit
delay ([`Loop`](stave_dsp::Loop)), because zero-delay feedback cannot be
solved sample by sample.

**Latency and the dry pad.** Some structures hear the signal late
(lookahead, linear-phase filters). Parallel paths are padded to arrive
at their join together ([`Compensator`](stave_dsp::Compensator)), and
the host is told the total so it can shift the track. (Clock skew:
every join is a synchronisation point.)

## The control plane

**Detector.** A control signal derived from the program: how loud it
is right now, with attack and release shaping how fast it believes
changes ([`EnvelopeFollower`](stave_dsp::EnvelopeFollower)).

**Source.** The one trait for control-signal producers
([`Source`](stave_dsp::Source)), with two kinds behind it:
program-driven (detectors, LFOs) and writer-driven (param ramps, which
follow the knob whatever audio plays).

**Route, Router and `ModMatrix`.** Wiring as data: which source
displaces which stage parameter, by how much
([`Router`](stave_dsp::Router), [`ModMatrix`](stave_dsp::ModMatrix)).
Fixed capacity, inspectable, no allocation.

**Displace from base.** Knobs set the base; modulation displaces from
it; they never fight. (Controlled components: one owner for the base
value, modulation a pure function on top.)

**Watch and `ModalWatch`.** Block-rate change detection over params:
did any of these move since the last block? ([`Watch`](stave_core::params::Watch),
[`ModalWatch`](stave_core::params::ModalWatch)). By value snapshot, and a
mode-gated param is still polled every block so a hidden mode never goes
stale. (A dependency array that is never gated off.)

**Sense endpoint.** A structure's interior signal, read at a declared
join ([`Chain::sense`](stave_dsp::Chain::sense)). Control wiring and
observation both read this one surface.

## Observation

**Telemetry.** The plugin → editor and tool channel
([`stave_telemetry`]): taps, the capture, the transfer curve, log
messages. The wrapper allocates one per instance and hands it to
[`Plugin::new`](stave::Plugin::new).

**Tap and `TapId`.** One published value and its id, declared as a
struct field under `#[derive(Taps)]`, which makes the id a constant
(`MyTaps::LEVEL`). The plugin writes `taps.level.set(v)`; a face or a
tool reads by that id. Never a string, and never "probe": that word
belongs to the tooling macro `probe!`. (Named metrics, sampled by the
UI.)

**Scope.** The framework's capture scratch for a face's
oscilloscope-style displays, plus the load meter.

## The realtime rules

**No allocation in `process`.** The audio thread cannot wait on the
allocator. The no-alloc check runs the whole loop under a guard that
fails the test on any allocation while every param moves.

**Bit-pin.** Refactor discipline: hash the output bits across real
configurations before a change, prove they did not move after. The pin
is the reviewer that cannot be argued with.

**Property test.** A test that states a promise the design makes, with
the expected value derived (from physics, mathematics or the
declaration), never recorded from a run: that is the difference from a
snapshot, which freezes whatever the code did. Its name reads as the
claim ("the echo arrives at the dialled time"), its tolerances carry the
measured values they protect, and when an ideal claim proves physically
impossible the property is qualified, never loosened into nothing. A
convention lives in a comment and dies in a refactor; a property test
fails the build. Every project ships the property tests only it can
write.

**Validators.** Steinberg's VST3 validator and clap-validator, the
formats' own compliance suites, run on the real binaries in CI. Zero
failures is the bar.

## Reading Rust here

**`super` is a path, not a parent class.** Rust has no inheritance.
Modules are namespaces that follow the file tree, and `super` means "my
parent module": `use super::echo::Echo` is `import { Echo } from
'../echo'`, nothing more. A folder's `mod.rs` is its `index.ts`.

**`pub(super)` is export scope, not a call.** "Public to my parent
module only." On methods it is the house pattern (the signature is the
access list); on fields it is a smell, because reaching into another
file's state recreates inheritance's action at a distance.

Every relationship in a plugin is composition: a type has its parts as
fields, parts receive what they need as arguments, and the only
interface mechanism is a trait, implemented explicitly.

## A Rosetta for web developers

| You know | Here it is |
|---|---|
| a request handler with an SLA | `process` on a block |
| `import { X } from '../y'` | `use super::y::X` (a module path) |
| not re-exported from the barrel | `pub(super)` |
| component props and schema | the declarations |
| a reducer | the plugin's `process` |
| JSX | a `chain!` topology, a face's `row`/`column` tree |
| a CSS transition | declared param smoothing |
| a dependency array | [`Watch`](stave_core::params::Watch) (value snapshots, never gated) |
| a controlled component | displace from base |
| metrics, structured logging | [`Tap`](stave::Tap)s |
| a second event loop | an [`Oversampled2x`](stave_dsp::Oversampled2x) clock domain |
| an object pool | the [`SwapPoint`](stave_dsp::SwapPoint) bench |
| schema migration discipline | ids are forever ([`Migrations`](stave_core::state::Migrations)) |
