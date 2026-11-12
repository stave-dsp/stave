# The overhaul

Stave's foundations are strong: the framework owns bypass, latency and
tail; host entry points are panic-guarded; the ABIs run under Miri and the
lock-free protocols under Loom; state survives unknown params; the DSP
kernel's laws are pinned and its circuits match ngspice. A five-way review
(November 2026: architecture, core, FFI/unsafe, DSP, host/test/tools, plus
the face layer) found that the *fit* between the parts is not yet at that
level: two ways to do several things, one word for several things, a few
dependencies pointing the wrong way, and a handful of real soundness bugs.

This file is the plan and the ledger. Every finding has an id; every fix
commit cites the id it closes (`overhaul: S3`), and ticks it here.

## What we are building toward

**A pit of success.** The obvious way to write a plugin is the correct
one, for a person or an agent reading the code cold:

* **One dependency, one import, one trait, one macro.** `stave` re-exports
  everything an author needs; `use stave::prelude::*;`; implement
  `Plugin`; `stave::export!(MyPlugin);` produces VST3 and CLAP.
  (The concrete signatures are under *Target interfaces*.)
* **One word per concept, one concept per word** (the glossary below is
  normative; `scripts/check-vocabulary.sh` rejects the banned synonyms).
* **Types, not conventions.** `ParamId` and `TapId` everywhere, enums not
  flag bags, `Option`/`Result` not sentinels, contracts the compiler
  checks rather than grep rules.
* **Layers point down.** Nothing portable depends on a platform crate;
  the plugin model depends on no UI; every crate does one thing.
* **Minimal, fenced unsafe.** Raw FFI lives in `*-sys` crates and small
  safe wrappers; every other crate is `#![forbid(unsafe_code)]`; every
  `unsafe` block states its invariant.
* **The acceptance test is Tilt.** `examples/tilt` — params, engine and
  face — reads top to bottom in one sitting, uses only the prelude, and
  is the shape every plugin copies.

### Target crate graph

```
leaves      stave-dsp   stave-telemetry   stave-macros   stave-measure
            stave-vst3-sys   stave-clap-sys
model       stave-core            → dsp, telemetry, macros     (no UI)
look        stave-ui (layout + Scene IR)   stave-raster → ui
            stave-face            → core, ui, raster         (design system)
platform    stave-cocoa           → face                     (window, pointer, blit, text)
formats     stave-vst3            → core, vst3-sys, face, cocoa
            stave-clap            → core, clap-sys, face, cocoa
author      stave                 → core, dsp, face, vst3, clap   (prelude + export!)
tooling     stave-host            → core, vst3-sys, clap-sys, measure
            stave-test            → stave, host, face
            tools/*
```

### The author's whole surface (the shape Tilt converges to)

```rust
use stave::prelude::*;

#[derive(Params)]
pub struct TiltParams {
    pub tilt: FloatParam,
    pub output: FloatParam,
    pub bypass: BoolParam,
}

impl Default for TiltParams {
    fn default() -> Self {
        TiltParams {
            tilt: FloatParam::new(ParamId(1), "Tilt", Range::linear(-6.0, 6.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(30.0)),
            output: FloatParam::new(ParamId(2), "Output", Range::linear(-24.0, 24.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(5.0)),
            bypass: BoolParam::bypass(ParamId(3)),
        }
    }
}

pub struct Tilt { /* Smoothers, filters */ }

impl Plugin for Tilt {
    const INFO: Info = Info { id: "com.example.tilt", name: "Tilt", vendor: "Example", ..Info::EFFECT };
    type Params = TiltParams;
    fn new(params: &Arc<TiltParams>, telemetry: &Telemetry) -> Self { /* … */ }
    fn prepare(&mut self, env: &Environment) { /* … */ }
    fn process(&mut self, block: &mut Block<'_>) { /* … */ }
    fn face(params: &TiltParams) -> Option<Face> { /* … */ }
}

stave::export!(Tilt);
```

## Target interfaces

The shapes every phase builds toward. Signatures are normative; bodies and
private details are not.

### Where the author's trait lives

`stave-core` stays UI-free, so the one author trait lives in the umbrella
`stave` crate (which sees core, dsp and face). Core keeps the *erased*
runtime contract the format wrappers drive (`stave_core::runtime::Erased`,
`#[doc(hidden)]`); `stave::export!` adapts `Plugin` to it.

```rust
// stave (umbrella)
pub trait Plugin: Sized + Send + 'static {
    const INFO: Info;
    const LAYOUTS: &'static [Layout] = &[Layout::STEREO];
    /// Bumped when saved state changes shape; `migrations` maps old to new.
    const STATE_VERSION: u32 = 0;
    type Params: Params + Default;

    fn new(params: &Arc<Self::Params>, telemetry: &Telemetry) -> Self;
    fn prepare(&mut self, _env: &Environment) {}   // may allocate
    fn reset(&mut self) {}                         // realtime
    fn process(&mut self, block: &mut Block<'_>);  // realtime
    fn latency(&self) -> u32 { 0 }                 // samples
    fn tail(&self) -> Tail { Tail::None }

    fn face(_params: &Self::Params) -> Option<Face> { None }
    fn presets(_params: &Self::Params) -> Vec<Preset> { Vec::new() }
    fn migrations() -> Migrations { Migrations::default() }
    fn save(&self) -> Vec<u8> { Vec::new() }       // state beyond params (rare)
    fn load(&mut self, _bytes: &[u8], _version: u32) {}
}

stave::export!(Tilt);   // VST3 + CLAP; CIDs derived stably from INFO.id
```

### Parameters

```rust
FloatParam::new(ParamId(1), "Tilt", Range::linear(-6.0, 6.0))   // also Range::log, Range::skewed
    .unit(Unit::Db).smooth(Smooth::ms(30.0)).group("Tone");
BoolParam::new(ParamId(4), "Hold", false);
BoolParam::bypass(ParamId(3));                                   // the one way to declare bypass
EnumParam::<Mode>::new(ParamId(5), "Mode", Mode::Tape);

p.tilt.value()      // f32 (Float), bool (Bool), Mode (Enum) — one reader, natural type
```

`Range` fields are private; constructors validate. `ParamId` is the only
id type (core, state, gestures, faces, migrations).

### Processing

```rust
pub struct Block<'a> { /* private */ }
impl Block<'_> {
    pub fn frames(&self) -> usize;
    pub fn sample_rate(&self) -> f32;
    pub fn transport(&self) -> Option<&Transport>;
    pub fn events(&self) -> &[Event];
    pub fn input(&self) -> &InputBuffer<'_>;
    pub fn output(&mut self) -> &mut OutputBuffer<'_>;
    pub fn aux(&self, i: usize) -> Option<&InputBuffer<'_>>;
    pub fn lanes<const N: usize>(&mut self) -> Lanes<'_, N>;   // per-frame, per-channel walk
}

pub struct Smoother { /* … */ }       // replaces ContinuousControl
impl Smoother {
    pub fn new(param: &FloatParam) -> Self;
    pub fn prepare(&mut self, env: &Environment);
    pub fn ramp<'s>(&'s mut self, block: &Block<'_>, param: &FloatParam) -> Ramp<'s>;  // Ramp::next() -> f32
}
```

### Telemetry

```rust
#[derive(Taps)]
pub struct HeirloomTaps { pub spin: Tap, pub level: Tap }   // HeirloomTaps::SPIN: TapId (derived const)

let taps = HeirloomTaps::new(telemetry);   // in Plugin::new
taps.spin.set(angle);                      // realtime, lock-free
f.tap(HeirloomTaps::SPIN)                  // in a face: typed, no strings
```

`Telemetry` (formerly `DebugBus`) has private fields. String-keyed
`probe!`/`brk_if!` remain for tooling only.

### The editor contract

```rust
// stave-core: what any editor reads and writes on its plugin (VST3's term).
pub trait Controller {
    fn param(&self, id: ParamId) -> Option<&dyn AnyParam>;   // value, normalized, format, steps
    fn begin(&self, id: ParamId);
    fn set(&self, id: ParamId, normalized: f64);
    fn end(&self, id: ParamId);
    fn tap(&self, id: TapId) -> f64;
    fn capture(&self, channel: usize, out: &mut [f32]) -> usize;
    fn curve(&self) -> Vec<f32>;
    fn presets(&self) -> &[Preset];
    fn preset(&self) -> Option<usize>;
    fn apply_preset(&self, index: usize);
}
/// The wrapper side: where an edit gesture goes (the host's automation).
pub trait GestureSink: Send { fn begin(&self, id: ParamId); fn perform(&self, id: ParamId, plain: f64); fn end(&self, id: ParamId); }
/// The one implementation, over the table, the telemetry and a sink.
pub struct PluginController { /* … */ }
```

`stave-face` consumes `&dyn Controller` (its `Host` trait goes away);
`stave-cocoa` is a window, a pointer, a blit and text — no parameter logic.

### DSP

```rust
pub trait Stage {
    fn prepare(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32;
    fn process(&mut self, block: &mut [f32]) { for s in block { *s = self.tick(*s) } }
    fn latency(&self) -> u32 { 0 }
    fn tail(&self) -> Tail { Tail::None }
}
```

Every leaf implements it. `Tail` lives in `stave-dsp` and core re-uses it
(core → dsp is allowed; the route/voice conversions move out of core).

### Faces

```rust
pub struct Grab { pub area: Area, pub gesture: Gesture }   // Area::Circle | Area::Rect
pub trait Part {
    fn occluders(&self, r: Rect, theme: &Theme, out: &mut Vec<Occluder>) {}
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {}
    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {}
    fn grabs(&self, r: Rect, f: &Frame<'_>, out: &mut Vec<Grab>) {}   // may read live values
    fn reads(&self) -> Reads { Reads::NONE }                          // params + taps, declared once
}
Theme { palette, finish, hardware, typography, spacing, coverings }
```

## Glossary (normative)

| Term | Means | Never call it |
|---|---|---|
| **Plugin** | The author's type; the one trait an author implements | Processor, Engine, Spec, StavePlugin |
| **Info** | A plugin's identity: id, name, vendor, version, features | Descriptor |
| **Layout** | A supported bus configuration | PortConfig, Arrangement (in author code) |
| **Params** / **Param** / **ParamId** | The table / one parameter / its forever id | Control, store, raw `u32` |
| **value()** | A param's current value in its natural type | plain, get, index, current |
| **normalized** | 0–1 in the param's own taper (the host and face language) | — |
| **Smooth** / **Smoother** | A declared ramp / the per-plugin state that runs it | Ramp, Glide, ContinuousControl |
| **Environment** | Conditions at prepare: sample rate, max block, layout | — |
| **prepare / reset / process** | Plugin lifecycle (prepare may allocate; process is realtime) | activate, deactivate (outside the wrappers) |
| **Block** | One `process` call's audio, events and param changes | ProcessCtx |
| **frame** | One sample position across channels; counts are `usize` | sample_offset, index, `u32` counts |
| **sample_rate** | Hz, `f32`, everywhere | sr, fs, rate |
| **Stage** | A DSP processor: prepare / reset / tick / process / latency / tail | process_sample, process_one, advance |
| **tick** | Advance a Stage one sample | process (for one sample) |
| **latency** / **Tail** | Samples of delay / how long output rings after input stops | latency_samples, tail_samples |
| **Telemetry** | The plugin → editor/tool channel (formerly DebugBus) | debug bus, bus |
| **Tap** / **TapId** | One published value / its id (engine writes, face reads) | probe |
| **Capture** | Audio snapshots published for displays | Scope, ring |
| **Face** | A plugin's editor, built from Parts on a Theme | editor tree, UiNode, Widget |
| **Part** / **Theme** / **Panel** / **Plate** | A face leaf / the look as data / the enclosure / a control group | widget, card, tokens |
| **Grab** / **Gesture** | Where a part can be grabbed / what a grab does | Control, Hit, Target |
| **Controller** | What a face reads and writes on its plugin (params, taps, presets, capture) — the VST3 meaning | Host (for this), EditorHost, Bridge |
| **Host** | The DAW; `stave-host` hosts plugins | anything else |
| **render** | Rasterise a Scene into pixels | running audio (that is `process`/`run`) |

## The ledger

Status: `[ ]` open · `[x]` done (with the commit).

### S — Soundness and realtime (first)

- [x] **S1** (87034fe) Restored values can glide in: `params/control.rs:86` reads the value before the restore counter, the writer stores value then counter. Read the counter (Acquire) first; the writer's increment Release.
- [x] **S2** (01ee9bf) `stave-test/src/harness.rs:73` `inputs_mut() -> &mut [Vec<f32>]`: a caller can reallocate a channel and the cached pointers dangle (UB from a safe API). Store `Box<[f32]>`, hand out `&mut [f32]`, rebuild pointers per block.
- [x] **S3** (87034fe) `AudioBuffer::channel/_ref` build slices from null host pointers (the debug check skips nulls). Wrappers substitute zeroed scratch; `from_raw` asserts non-null.
- [x] **S4** (98cac32: the bridge and `parts()` are gone) `stave-ui-cocoa/src/face.rs` `parts()` aliases `&mut FaceEditor` with `&Slots` (UB under Stacked Borrows). Split by destructuring.
- [x] **S5** (98cac32, 2ec7f4a: the NSView's `face` ivar is the one root, a counted `Rc` each callback holds for its call; `try_borrow_mut` drops reentrant events) The editor is reached through four raw-pointer roots (`EditorBox`, `self_ptr`, `EditorTarget`, ivars) with no reentrancy guard. One `NonNull` root; a busy flag drops reentrant events.
- [x] **S6** (a614e52) VST3 `tresult` constants are the COM-compatible values; macOS/Linux use `kNoInterface = -1`, `kResultFalse = 1`, `kInvalidArgument = 2` … `kOutOfMemory = 6` (`stave-vst3/src/sys.rs:40`). Split by `cfg(windows)`.
- [x] **S7** (a614e52) `IPlugView` refcount is faked (`wrapper/view.rs:23`); a host holding the view past the controller gets a use-after-free. Real count holding an Instance reference.
- [x] **S8** (formats half in a614e52: MSRV 1.81, `ffi_entry` on every VST3/CLAP entry; the Cocoa half in 2ec7f4a: every Objective-C method and the timer inside `guard`, a panicking face goes still) Panics cross FFI: Cocoa methods and timers, VST3 `createInstance`/`createView`/`sv_*`, CLAP `create_plugin`/`init` are unguarded; MSRV 1.80 makes that UB. MSRV 1.81 and one `ffi_entry` guard for every `extern "C"`.
- [x] **S9** (2ec7f4a: the typed `send` picks `objc_msgSend_stret` for a CGRect return on x86_64, `BOOL` is `ObjcBool`; tested on arm64 and under Rosetta) `objc_msgSend` transmutes returning `CGRect` need `_stret` on x86_64; `bool` for `BOOL` is wrong there. A typed send that handles both, or a compile error off aarch64 until it does.
- [x] **S10** (2ec7f4a: `define_class` suffixes the address of a static in the image) Objective-C class names are global (`StaveFaceView`, …): two stave plugins in one DAW share one class. Per-binary unique suffix.
- [x] **S11** (87034fe) Stable `sort_by_key` on the audio thread can allocate (`processor.rs:118`, `clap process.rs:73`). Insertion sort keyed by (frame, sequence).
- [x] **S12** (bounded scratch + `BlockTruncated` fault and the realtime panic slot done in 87034fe; this commit: inside `PanicGuard::run_realtime` the panic hook formats into one of four preallocated lock-free records — no file, lock, allocation or default hook on the audio thread — and the next main-thread `PanicGuard::report` drains them into the instance's log) Allocation and locks on the audio thread: `split_for_block` grows `Vec`s past the declared block; `PanicGuard` logs through a `Mutex` + file open. Truncate and count as a fault; report from the main thread.
- [x] **S13** (01ee9bf) Probe names > 31 bytes never match their truncated slot, so every block reserves a new one until the bus is full (`probe.rs:83`, `inline_str.rs:49`). Reject at compile time (typed taps make this moot).
- [x] **S14** (01ee9bf) `Ring::delayed` indexes before `prepare` (a `Loop::tick` before prepare panics). Size in `new` or read with a 0 fallback.
- [x] **S15** (01ee9bf) One NaN locks Svf/Biquad/Loop/Chain feedback until reset. NaN → 0 in `denormal::flush`, and a guard in `Loop`.
- [x] **S16** (87034fe) Param lookup is a linear scan through `dyn` calls on the audio thread (`types.rs:522`; CLAP bypass every block, VST3 every event). An id → index table built at instance creation; cache the bypass param.
- [x] **S17** (the 30 Hz editor timer went with L7 in e2f5294; the view, tracking area and run-loop timer are released in 2ec7f4a) Leaks: FaceMount view alloc, NSTrackingArea, CFRunLoopTimer never released; the 30 Hz editor timer keeps running with a face attached.
- [x] **S18** (2ec7f4a: a nil font fails the line; the cache empties past `FONT_CACHE`) `text.rs` `CFRetain(NULL)` if NSFont returns nil; font cache unbounded.
- [x] **S19** (01ee9bf) `stave-host` `RTLD_LOCAL = 0x4` is `RTLD_NOLOAD` on Linux (`plugin/module.rs:104`). Use `libc::RTLD_*`.
- [x] **S21** (this commit) CLAP state load never asked the host to re-read parameter values (`clap_host_params.rescan(VALUES)`), so a DAW kept showing stale values after loading a session or preset; found by clap-validator 0.4.1's state-reproducibility tests, which the validation script also hid (it died silently under `pipefail` and mis-read the coloured summary).
- [x] **S22** (this commit; release/0.2 286bd17, v0.2.2) NaN at extreme sample rates (anvil, halo) — `Biquad` held f32 coefficients: a 7–10 Hz high-pass at 768 kHz sits 3e-9 inside the stability triangle (|a1| < 1 + a2), below f32's step at 2.0 (2.4e-7), so the pole rounded onto or past z = 1 and the output DC blockers diverged to NaN; low corners at 192–705.6 kHz rang for seconds and a 10 Hz low-pass at 384 kHz rounded to zero gain (`1 − cos ω`). Coefficients and state are f64, derived cancellation-free (2·sin²(ω/2)); `tests/sample_rate_range.rs` pins every `FilterSpec` at 1 kHz–768 kHz incl. fractional rates (clap-validator `process-varying-sample-rates`).
- [x] **S23** (this commit; release/0.2 286bd17, v0.2.2) value text round-trip across unit switches (saga ramp) — `Unit::Ms`/`Unit::Hz` chose ms→s and Hz→kHz (and `Unit::Db` its −inf floor) on the raw value, so 999.9999999999999 read "1000.0 ms", parsed to 1000 and re-read "1.00 s"; the switch is now decided on the value as rounded for display, and a property test sweeps every unit kind through its switch points ± ulps and half-steps (clap-validator `param-conversions`).
- [x] **S20** (a614e52) `EditorBox` `unsafe impl Send/Sync` is unneeded; stale SAFETY/comments (`capture.rs:24`, `sys.rs:5`).

### L — Layering

- [x] **L1** (the face half in 98cac32: core has no UI dependency, defines `Controller`/`GestureSink`/`PluginController`, face depends on core, `Bind` carries a `ParamId`; 5f01da0: faces are declared by `stave::Plugin::face`, and the route params and note → voice conversion moved to the umbrella crate) `stave-core` depends on `stave-face` (`Spec::face`, `editor_face`, `BindParam`) and on `stave-dsp` (`RouteSlot`, `VoiceEvent`). Core defines the portable **Controller** contract; face depends on core; route/voice conversions move downstream. `Bind` becomes `ParamId`.
- [x] **L2** (98cac32: `PluginController` in core; stave-ui-cocoa is `FaceView` only) Portable editor logic (`Bridge`/`Slots`: param ids, taps, presets, capture over the table and telemetry) lives in the macOS crate. Move to core as the Controller implementation; the platform shell keeps only window, pointer, blit, text.
- [x] **L3** (18043c2; stave-host needs only `stave-vst3-sys` — it hosts no CLAP yet) No `*-sys` crates: `stave-host` depends on all of `stave-vst3` (and so on Cocoa) for its ABI types. Split `stave-vst3-sys` and `stave-clap-sys`.
- [x] **L4** (4a3e876: CLAP `clap.gui`, embedded Cocoa; CLAP now attaches telemetry and publishes the wrapper taps and capture; one CLAP-dialect note port for instruments and note effects, checked with clap-validator on a scratch instrument; both wrappers take stave-ui-cocoa only on macOS, and the Linux CI job checks them) Faces are VST3-only: no CLAP `gui` extension (and verify `note-ports` for instruments). `stave-vst3` hard-depends on the Cocoa crate.
- [x] **L5** (98cac32: one `Controller` in core, one `GestureSink` per wrapper) Two gesture-sink traits (`EditorHost`, `face::Host`) for one concept. One `Controller` in core; wrappers adapt it.
- [x] **L6** (448efb2, f697711) `stave-debug` is the shipping telemetry transport, with 11 `pub` atomic fields. Rename `stave-telemetry`, make fields private, typed `Tap` handles for shipping, string `probe!` for tooling only.
- [x] **L7** (e2f5294) The legacy UI layer: delete `stave-core` `editor.rs`/`editor_kit.rs`/`Widget`/`UiNode`/`ui!` editor path, `stave-ui` `tokens.rs`/`scenes/`, the legacy Cocoa widgets, mount/tick/view widget state, `stave-test` `editor.rs`/`raster.rs`, `editor_ui` in the wrappers. First move what survives: `Preset` (to core), `photo` (to face), objc helpers (`sel`/`class`/`msg_*`/`CGRect`, `detach`) out of `widgets/`, `VIEW_WHEN_BIT` users.
- [x] **L8** (e2f5294) `stave-ui` `Node` still carries CSS-like styling (bg, border, radius) that competes with "parts never mint a colour". Keep layout only.
- [x] **L9** (dc122ec) `stave-host`: the CLI parser and usage text belong in `tools/stave-bench`; `loaded.rs` depends on `bench::describe`; `modules.rs` mixes a 900-line catalog with `ModuleInstance`. `stave-measure` claims pure functions but has `wav.rs` file I/O (move to host).
- [x] **L10** (5f01da0: `crates/stave` — `Plugin`, `prelude`, `export!`; VST3 class ids are an FNV-1a hash of `Info::id`, pinned by a test, with an override form for ids a plugin already shipped) An umbrella `stave` crate with the prelude and `stave::export!` (VST3 + CLAP from one line, CIDs derived stably from `Info::id`).

### A — The author's API

- [x] **A1** (5f01da0: one `stave::Plugin`; core keeps the doc-hidden `runtime::Erased` the wrappers drive; `Processor`/`Spec`/`StavePlugin`/the wide core `Plugin`/`FaceSpec`/`HasFace`/`attach_telemetry`/`set_param`/`deactivate` are gone) Two plugin traits (`Plugin` wide/public, implemented only by `StavePlugin`; `Processor` + `Spec`), renamed twins (`debug_attach`/`attach_telemetry`, `editor_ui`/`editor`, `editor_face`/`face`, `param_table`/`params`, `activate(f32, usize)`/`activate(Environment)`), dead `Plugin::set_param`. One author trait `Plugin`; the wrapper-facing erased trait is internal.
- [x] **A2** (5f01da0: `stave_core::runtime`, `#[doc(hidden)]`, not re-exported at the root or by `stave`) Wrapper internals in the author namespace (`PluginCell`, `ProcessorState`, `ScopedFtz`, `ranges_alias`, `GestureQueue`, unsafe `split_for_block`/`feed`/`mix`). Move under `#[doc(hidden)] pub mod __wrapper` (or the format crates).
- [x] **A3** (5f01da0: `value()` in the natural type on every param, `AnyParam::value()` for the erased f64; `Smoother::new(&FloatParam)` and `Steps::new(&param)` over a `Stepped` bool/enum, both restore-safe) Five ways to read a param (`plain()`, `value()`, trait `plain()`, `get()`, `index()`); `DiscreteControl` takes id + fn pointer + raw f64 and ignores restores. One `value()` in the natural type; `Smoother::new(&FloatParam)` and a stepped equivalent built from the param.
- [x] **A4** (5f01da0: `ParamId` in the param constructors, `GestureEvent`, `Migrations`; `Range` private with `linear`/`log`/`skewed`; `ParamFlags` set only by builders, bypass only by `BoolParam::bypass`) Raw `u32` where `ParamId` belongs (`FloatParam::new`, `Migrations::renamed`, `GestureEvent`, `StateStore`); public `Range` fields allow an invalid log range; `ParamFlags` bag lets a float be "bypass". `ParamId` everywhere, private `Range`, `BoolParam::bypass`.
- [x] **A5** (5f01da0, this commit: sample rate `f32` in `Environment`, `Smoother::prepare(&Environment)` and `BypassGate`; frame offsets and counts `usize` in `Event`, `ParamChange`, `Block`) Numeric drift: sample rate `f32` here, `f64` in `ContinuousControl::prepare`/`BypassGate`/circuits; frame counts `usize` here, `u32` there. One type each (glossary).
- [x] **A6** (448efb2; 5f01da0: telemetry arrives in `Plugin::new(params, &Telemetry)` and `Taps::new` takes `&Telemetry`; faces name taps by `TapId` and the string shim is gone since 98cac32) The engine ↔ face channel is a string: engines `Tap::new("spin")`, faces `.probes(&["spin"])`, a typo reads 0. `#[derive(Taps)]` table referenced by both; telemetry handed to `Plugin::new`.
- [x] **A7** (5f01da0: `InputBuffer` (read only) / `OutputBuffer` (`channel_mut`, `channels_mut`); `Lanes` takes each output pointer once; stave-test is `forbid(unsafe_code)`) `AudioBuffer`: input exposed mutably; no way to borrow two channels mutably (why `Lanes` re-derives raw pointers). `InputBuffer`/`OutputBuffer`, `channels_mut()`.
- [x] **A8** (31340a2) History in code comments ("user-specced", "Step 1.3", "chime consumer … fifth instrument lock", "forge's boot()", milestone tags) and cargo-culted `extern crate alloc` in std crates. Remove.
- [x] **A9** (e2f5294: `ui!` deleted) `ui!` fails silently (bad `cols` → 1, bad `kind` → Rect, dropped attrs). Becomes moot with L7 for editors; any surviving macro emits `compile_error!`.
- [x] **A10** (5f01da0: one file, `use stave::prelude::*`, a `Smoother` per knob, `block.lanes::<2>()`; its laws run on the harness) Tilt clones the params `Arc` every block and hand-wires lanes, ramps and coefficients. Tilt is rewritten to the target shape as the acceptance test.

### D — DSP vocabulary

- [x] **D1** (91ce414) Most leaves aren't `Stage`s (Svf, Biquad, Allpass1, LinkwitzRiley4, Triode, PushPull, SmoothSat, PassiveToneStack, control primitives): five verbs for one step. Every leaf implements `Stage`; multi-output filters keep their extra methods.
- [x] **D2** (91ce414) The tail contract stops at the kernel: no leaf overrides it, `StageKind` forwards neither latency nor tail, `Chain` reports `Some(0)` with feedback on, `Par2` omits its pads. Forward everywhere; filters estimate a −120 dB tail; feedback chains report unbounded.
- [x] **D3** (91ce414) `prepare` means three things (`DelayLine::prepare(max_samples)`, `f64` rates in PowerSupply/WDF). `prepare(sample_rate: f32)` everywhere; `DelayLine::allocate(max_samples)`.
- [x] **D4** (91ce414) Two setter styles (rate passed per call vs cached at prepare). Always cache the rate; setters take musical units only.
- [x] **D5** (91ce414) The one-pole smoothing coefficient is written five times in the crate (and ~18 in plugins). `primitives::filters::OnePole` with `tau_coeff`, plus `DcBlocker`.
- [x] **D6** (91ce414) Missing primitives plugins re-implement: `DelayAllpass` (Schroeder), `Comb`, `DcBlocker`, `soft_clip`, `Crossover3`.
- [x] **D7** (91ce414) Two 2× oversamplers (legacy IIR `Oversampler2x` still root-exported beside the FIR internals). `Oversampled2x` is the one public form.
- [x] **D8** (91ce414) Two `Route` types (`kernel::control::Route`, `modmatrix::Route`); ModMatrix is not built on `Router` as documented. Rebuild it on `Router`.
- [x] **D9** (91ce414) Silent clamping hides bugs: `Svf::process` clamps the channel index; `Chain::push` returns a sentinel id. Svf mono under `PerChannel`; `Option`/`Result` for capacity.
- [x] **D10** (380bc12) Units and names: `rate`/`sample_rate`/`sr`/`fs`; `set_hz`/`set_rate`/`set_frequency`/`set_corner`; `_ms`/`tc_secs`/seconds/samples; phase 0–1 f64 vs radians f32; `q` vs `resonance`; three overlapping shape enums; three "Filter" types at the root. One name per concept (glossary), one shape enum.
- [x] **D11** (380bc12) Docs out of step: the kernel names forge and multiband; Chain says "inside ONE oversampler"; lib.rs describes 3 layers of 5; `tan32` is `x.tan()`; `hz_of_key` in the kernel.

### F — The face layer

- [x] **F1** (07707b8) Four nouns for one idea (`Control`, `Hit`, `Gesture`, `Target`) and two ways to declare grabs (`controls()`, `hit()`). `Grab { area, gesture }` and one `grabs(&self, r, f)` that may read live values.
- [x] **F2** (07707b8: `Theme { name, finish: Finish { coat, … }, palette, hardware, typography, spacing, coverings }`; the storybook renders byte-identical) `Theme` is a flat ~25-field struct mixing palette, materials, hardware, typography, spacing and coverings; `type_`. Group into `Palette`, `Finish`, `Hardware`, `Typography`, `Spacing`, `Coverings`.
- [x] **F3** (`LampSource::Always` replaced the pilot sentinel in 98cac32; 07707b8: `Footswitch { press: Press, lit: LitWhen }`, `Lamp { style: LampStyle }`) Sentinels and flag fields: `Lamp::pilot` is `Probe("", −∞)`; `Footswitch { invert, momentary, color }`. `LampSource::Always`; `Latch`/`Momentary` and `LitWhen` enums.
- [ ] **F4** Part dimensions are magic numbers; titles estimate text width; layout never measures text. A `Measure` from the platform reaches layout; dimensions derive from the theme's spacing and type scale.
- [x] **F5** (07707b8: watched params and taps deduplicated once at construction; the hover key is bits) `Runtime::observe` re-sorts binds and probes and formats a debug string every frame. Precompute at construction; identify the hover target without formatting.
- [x] **F6** (07707b8: `Part::reads() -> Reads { params, taps }`, a Display's `.reads()`/`.taps()` fill it and its draw fn gets the params as `Frame::binds`; one `Runtime::frame(i)`) `Display` reads and `Part::reads` are two lists; `Frame` is built in three places. One declaration; one `frame(i)` helper.
- [x] **F7** (98cac32: `FaceView` and `Controller`) Six nouns for the host side of a face (`FaceState`, `FaceEditor`, `FaceTarget`, `EditorTarget`, `FaceMount`, `Bridge`). `FaceView` (platform) and `Controller` (portable).
- [ ] **F8** (the layout half in 07707b8: `stave_test::face::grabs_land` — parts inside the face, a press at each grab's centre grabs that part — mounted on Tilt, and stave-face/tests/named_parts.rs; golden images wait for C7/T7: an oracle needs deterministic text first) No layout or golden-image tests for faces.
- [x] **F9** (07707b8: `on_step(node, bind, step)`; the static layer is baked per step) Content can't follow a parameter's step (eddy's tabs no longer select its algorithm; vault shows every machine). Visibility by param step alongside pages.
- [ ] **F10** (`Preset::from_pairs` in 5f01da0; `Frame::param(i)` reaches a bound param's range and text in 07707b8; the clip op, radial gradients, the gain-reduction meter, the shared route row and bipolar Trace/Lamp remain — raster and catalogue work for the plugin migration) Graduations from the plugin conversions: a clip op or clipped Display, radial gradients, a gain-reduction meter (hangs from the top), a shared mod-route row part, `Preset::from_pairs` (five local copies), parameter ranges reachable from draw fns, bipolar `Trace`/`Lamp`.
- [x] **F11** (2ec7f4a: `objc.rs` — typed `send`, `sel!`, `Retained`, `MainThread`, `define_class` with typed `Method`s, `guard`; `CfRef`; no crate-wide allow; `forbid(unsafe_code)` in stave-face, stave-macros and the storybook; stave-test keeps its harness's one `unsafe` until A7) `stave-ui-cocoa`'s ~960 unsafe sites and crate-wide `allow(undocumented_unsafe_blocks)`. A small typed Objective-C core (typed `send` with stret, cached selectors, `Retained<T>`, a `MainThread` token, `define_class` with unique names and a panic guard); drop the allow; `forbid(unsafe_code)` in face, test and every safe crate.

### T — Testing and tooling

- [ ] **T1** (f7778e4: `PluginHarness` is a stave-host `Instance`, so `render::Driver`, the measurements and the validation checks drive a linked plugin in-process; stave-play's `Engine` is already generic over `Instance`; folding the harness's own block loop into the Driver waits on T5's one note model) Three block drivers (`PluginHarness`, `render::Driver`, stave-play's `Engine`). One driver; the others build on it.
- [x] **T2** (f7778e4: `PluginHarness::builder()` — rate, block size, layout, starting values restored before prepare; `set_transport`, `aux_mut` (up to `MAX_AUX` buses, never allocating), `process_with(changes, events)` with sample-accurate `change(id, frame, value)`, `process_frames` (0 allowed), `prepare`/`reset`, `save`/`load`) The harness is too thin (no telemetry, sidechain, transport, sample-accurate params, layout choice), so 56 plugin test files build `ProcessCtx` by hand. A builder with `process_with(params, events)`.
- [x] **T3** (f7778e4: `stave_test::plugin_checks!(crate::Plugin, { perf, off_face, seeds })` — validation, no-alloc with every param moving, perf, face binds/grabs/laws, and the T4 checks, each a test; Tilt's three check files became one line) 54 per-plugin check files (`no_alloc.rs`, `perf.rs`, `validation.rs`) with stringly wiggle lists that drift. One `stave_test::plugin_checks!` macro; wiggle every param by default.
- [ ] **T4** (f7778e4: `stave_test::checks` — `block_sizes` (0, 1, odd, oversized, re-prepare), `sample_rates` (8 kHz–768 kHz), `prepare_and_reset_cycles` (must sound as fresh), `random_automation` (smoothed and sample-accurate, seeded), `state_round_trip`, `deterministic`; each pinned by a plugin broken on purpose; multi-threaded calls and crash isolation are V3's simulated host) Validation gaps vs pluginval/clap-validator: block-size changes, zero-length and odd blocks, sample-rate changes, activate/deactivate cycles, randomized automation, multi-threaded calls, crash isolation. One `checks/*.rs` each.
- [ ] **T5** Two note models (`stave_host::note::NoteEvent{offset,pitch,velocity f32}` vs core `NoteEvent{key,velocity f64}`) and two param models (host `Control`/`Setting`/`Range` vs `AnyParam`). One of each; convert at the ABI.
- [x] **T6** (f7778e4: one harness verb set — `process`/`process_with`/`process_frames`/`run` — and one check mount, `plugin_checks!`; this commit: audio is processed or run, never rendered — `stave_host::drive::Driver` (`processed()`), `Signal::samples`, stave-play's `Engine::fill`, every test closure — and `check-vocabulary.sh` checks "render" across the whole tree outside the pixel code; every `*Policy` is a `*Config`; the host's `Probe`/`probes()` are `TapReading`/`taps()`; the dsp catalog is `stave_host::stages` (`stage:NAME`, `CatalogInstance`) so "module" is only the VST3 dylib; `loaded::Loaded`/`LoadError` are `opened::Opened`/`OpenError`; `tone_test.rs` is `tone_window.rs` (`ToneWindow`)) `render` means five things; `run_block`/`process_block`/`process` on one harness; check mounts in five shapes (`check_no_alloc`, `perf_report`, `assert_passes`, `binds_resolve`, `assert_no_pump`); `*Policy` vs `*Config`; `probe` and `module` overloaded; `Loaded`/`LoadError` beside `LoadedPlugin`/`HostError`; `tone_test.rs` reads like a test.
- [ ] **T7** (ab34456: `stave_raster::BoxText` deterministic text, `stave_test::image` — compressed PNG write/read, tolerant compare, `assert_golden!` with `STAVE_BLESS=1` — and `scripts/check-golden-sizes.sh` (3 MiB budget); raster property tests remain) stave-raster loses its only oracle with the CoreGraphics parity test. Golden PNGs of faces and scenes; property tests (coverage conserved under translation).
- [ ] **T8** Raster performance: per-fill allocations, `Vec<Vec<Pt>>` per op, four `round`s per pixel, per-pixel linear gradient search. A reusable rasterizer scratch, fixed-point source-over, sparse spans, gradient LUT.
- [ ] **T9** Raster correctness: strokes are unions of quads + joins (overlap double-counts coverage; "exact" is false); bilinear aliases past 2× minification; hard-edged image rects; no even-odd rule or square cap; dead `one_to_one` branch; doc says round joins (they're miter).
- [ ] **T10** Public fields that break invariants (`Canvas{w,h,px}`, `Mask`); `BlockOut` exposes `Vec`; `outputs()` allocates.
- [ ] **T11** `stave-play --screenshot` captures the screen region, not the editor (caught other agents' windows). Offscreen capture through the face runtime.
- [ ] **T12** Loom models a copy of the protocol, not the shipping types. A `cfg(loom)` atomic shim on the real code.

### C — Continuous integration

CI has not been green on GitHub since the move: the zero-warnings and
clippy jobs fail on lints the floating `stable` toolchain added after the
local one (stave-play, the old Cocoa widgets), and clap-validator, built
from its git HEAD, no longer compiles. The plugins repo waits on its
framework deploy key.

- [x] **C1** (rust-toolchain.toml 1.96.0 in both repos; weekly canary on newest stable opens an issue) Pin the toolchain in `rust-toolchain.toml` (both repos) so local and CI agree; a scheduled job builds on the newest stable and opens an issue on new lints instead of breaking `main`.
- [x] **C2** An MSRV job so the floor is real. Measured: dependencies (icu 2.2 via url/eframe) need 1.86, so `rust-version` is 1.86, and CI checks it.
- [x] **C3** (clap-validator 0.4.1, VST3 SDK v3.8.1_build_84 in both repos; pluginval still to add) Pin external tools (clap-validator to a tag/rev, the VST3 SDK to a tag; add Tracktion's pluginval) and cache their builds.
- [ ] **C4** Sanitizers where Miri can't reach: AddressSanitizer and ThreadSanitizer (nightly) over the ABI tests, stave-host hosting a real plugin bundle, and the face runtime; a scheduled job.
- [ ] **C5** Fuzzing (`cargo-fuzz`, short in PRs, long nightly): state chunk load (host-supplied bytes), param text parsing, the VST3/CLAP event and param-change paths, the rasterizer.
- [ ] **C6** Every plugin through the realtime checks in CI (no allocation, no locks in `process`), via `plugin_checks!` (T3), plus the extended validator checks (T4).
- [ ] **C7** Golden images (T7): render the storybook and every plugin face offscreen, compare to committed PNGs within a tolerance, upload diffs as artifacts.
- [ ] **C8** Plugins against framework `main`: a scheduled (and dispatchable) job in stave-plugins that links the framework at `main` and runs the plugin gates, so breakage is seen before a release, not at the bump.
- [ ] **C9** (`scripts/check-vocabulary.sh` in gates and CI since 31340a2; cargo deny, the unused-dependency check, the rustdoc link check and semver-checks remain) Hygiene gates: `scripts/check-vocabulary.sh` (the glossary), `cargo deny` (licences, advisories, duplicate versions), unused-dependency check, rustdoc link check; `cargo semver-checks` on the public crates once releases are published.
- [ ] **C10** Name and shape: the workflow is `ci` (one per repo), quick jobs gate PRs, the heavy ones (Miri, sanitizers, fuzz, validators) run on `main` and nightly; required checks configured on `main`.
- [x] **C11** Moot: the repos are public (2026-11-12), so the plugins CI fetches the framework without a deploy key; public runners are also free, which unblocked CI billing.

### V — Verification (proofs, models, deterministic simulation)

What TLA+, Lean and Bend teach, applied where stave's risk is.

- [ ] **V1** (5f01da0: `Normalized` at the `Controller` boundary, `ParamId` everywhere (A4), typed taps (A6); the lifecycle as a state in this commit: `ProcessorState::prepare`/`release`, and `split_for_block` — the only way to the plugin's audio-thread borrow — is `None` unless prepared, so a host processing before activate gets silence and an `Unprepared` fault; still open: `Hz`/`Db`/`Seconds` newtypes across stave-dsp's setters — they cut across the D vocabulary and every plugin, so they land with P) Types as proofs: unit newtypes (`Hz`, `Db`, `Seconds`, `Normalized`) on the author and DSP APIs; typestate for the plugin lifecycle (`process` only on a prepared plugin); `ParamId`/`TapId` everywhere (A4, A6). Lands with the A/D vocabulary work.
- [ ] **V2** Kani (bounded model checking) harnesses in CI for the unsafe and index-heavy core: `Ring`, `InlineStr` truncation, `AudioBuffer`/`Lanes` indexing, `IndexedParams` binary search, rasterizer coverage accumulation, the gesture queue. Verus where an all-inputs invariant is worth stating inline.
- [ ] **V3** A deterministic simulated host in stave-host: seeded randomness over call order, block size (incl. 0 and odd), sample rate, activate/deactivate/setState interleavings and thread timing; any failure replays from its seed. Subsumes most of T4.
- [ ] **V4** Protocol models (TLA+ with TLC/Apalache, or Stateright in Rust) for the host lifecycle across threads, param restore and smoothing, the gesture ring (begin/perform/end, liveness), telemetry slot reservation, and editor attach/detach/refcount/reentrancy. Trace validation: tests and stave-play emit event traces checked against the model.
- [ ] **V5** Telemetry as a structured, checkable event trace (lifecycle, gestures, restores, faults) so observability doubles as verification, in tests and in real DAW sessions.
- [ ] **V6** Functional core, imperative shell: keep pure, deterministic code (faces, DSP graphs, layout, rasterizer) free of effects so golden images and replay hold; effects (threads, FFI, atomics) at the edges.
- [ ] **V7** (Optional) Lean 4 proofs of a few DSP laws as documentation-grade results: filter stability over declared ranges, taper monotonicity and round-trip, smoother convergence, the kernel's latency/tail algebra.

### U — Interaction verification (how people use the plugins)

Verification reaches the hands: every click, drag, wheel and hover on a
face is scriptable, its effect checkable at exact coordinates, and the
same checks run headless in CI and against a real window.

- [x] **U1** (ab34456: `stave_test::FaceDriver::{of::<P>(), with_params, new(face, table, presets)}` — aim at a name, `PartId`, `ParamId` or exact point, end drags at a point or `dx`/`dy`/`by` offset; every edit logged as `Edit::{Begin, Perform, End}` with `assert_bracketed`; `hovered`/`active`/`focus`/`cursor`/`texts`/`shows`/`layers`/`render`; the runtime gained keyboard focus and `Key`, which the Cocoa view now routes) A face driver in stave-test: `FaceDriver::new(face, controller)` with `click(at)`, `double_click(at)`, `drag(from, to, mods)`, `wheel(at, lines)`, `hover(at)`, `key(code)`, `release()`, plus queries: `grab_at(p) -> Option<(PartId, Gesture)>`, `cursor()`, `focus()`, `value(id)`, `rect(part)`. Built on the face runtime, deterministic, no window.
- [x] **U2** (07707b8: `PartId::{Param, Name}` from `node.id("…")` or the part; `Runtime::{find, rect, parts, grabs, grab_at}`) Parts addressable by name, not coordinates: every part gets a stable `PartId` (its bound param, its role, or an explicit `.id("bypass")`), so tests say `driver.drag(knob("tilt"), up(40.0))` and the geometry comes from the solved layout.
- [x] **U3** (ab34456: `stave_test::face::laws::gestures`, on every `Runtime::states()` state; on Tilt, every storybook story, and a `controls` story that reaches pads, pages, nudges, momentaries and `on_step`) Exact-coordinate laws for every part kind, checked on every face: a click at a control's centre hits that control; a drag of the documented travel sweeps the full range; Shift-drag is 10× finer; a click just outside the visible hardware hits nothing; double-click resets; toggles step and wrap; option labels choose; pads map x/y (and the y-down maps engram needs) exactly under the pointer.
- [x] **U4** (ab34456: `laws::layout`; "declared on top" is not needed yet — no face overlaps grabs across parts; text collisions measured with `BoxText` metrics; `Laws::off_face` names host-only params) Layout invariants on every face, every page: no two grab areas overlap unless one is declared to sit on top; every grab area lies within its part's visible hardware; every bound param is reachable by some grab on some page; nothing is outside the window; legends don't collide.
- [x] **U5** (ab34456: `laws::overlays`; the runtime paints the dragged, else hovered, part last, so its readout is on top by construction, and `Runtime::layers` shows the order) Overlays and z-order: readouts that appear on hover/drag, page flips, and any popover are checked to draw on top, to not steal grabs from what's under them unless they're interactive, and to disappear on release/exit; the hover target, cursor and focus are part of the observable state.
- [ ] **U6** (ab34456: Tilt idle/hovered/dragging, every storybook story and every page/step state of the controls story, under `tests/golden`; CI upload of `target/golden-diff` is C7's) Golden images per interaction state: idle, hovered, dragging (value readout), each page, each theme; diffs uploaded by CI (C7).
- [x] **U7** (ab34456: `laws::sessions` — seeded xorshift sessions of hover/press/move/release/wheel/key/exit; a failure prints the seed, the step and the replayable trace) Model-based UI tests: random sequences of pointer events (seeded, replayable) against every face, checking invariants after each step (values in range, gestures always bracketed begin→end, no stuck drag after release, hover cleared on exit) — the V3 simulated host for the editor.
- [ ] **U8** The real window, end to end: synthesize `NSEvent`s into the face view of a real (offscreen) window and check the same laws, so the platform's coordinate mapping (flip, scale, scroll units, modifier bits) is tested, not assumed. The y-flip bug fixed during the face work is the case this catches.
- [ ] **U9** (ab34456: `stave_face::Trace` — one event per text line — and `Runtime::apply`, which the driver uses for every act, so a test's trace replays exactly; stave-play recording remains) Session traces: stave-play records a person's interaction as a replayable trace (pointer events + the gestures they produced + param values); a recorded session becomes a regression test.

### P — The plugins (stave-plugins), after the framework settles

- [ ] **P1** Migrate all seventeen to the target author API (one trait, `stave` crate, taps tables, `Smoother`, Stage leaves).
- [ ] **P2** Delete re-implemented DSP for the new primitives (D5, D6) and the shared route row (F10).
- [ ] **P3** Replace the 54 per-plugin check files with `plugin_checks!` (T3) and hand-built `ProcessCtx` tests with the harness (T2).
- [ ] **P4** Restore eddy's algorithm tabs and vault's per-machine plates with param-step visibility (F9).

## Order

1. **S** — small, contained, no API changes; fix and pin each with a test.
2. **L7** — delete the legacy UI (after moving what survives).
3. **L** — the layering, landing the `Controller` contract, `*-sys` crates, telemetry, the umbrella crate and CLAP GUI.
4. **A, D, F** — the author API and vocabulary, with Tilt and the storybook as the running acceptance tests, then `scripts/check-vocabulary.sh`.
5. **T**, **C**, **V** and **U** (V1 and U2 land with step 4) — the harness, checks and validator, golden images, raster; CI pinned, widened and green. (C1–C3 first: CI must be green before the big refactors land.)
6. **P** — migrate the plugins once, and cut a release.
