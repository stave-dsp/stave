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
look        stave-ui (layout + Scene IR)   stave-raster → ui   stave-font → raster
            stave-face            → core, ui, raster         (design system)
platform    stave-ui-cocoa        → face                     (macOS: window, pointer, blit, CoreText)
            stave-ui-shell        → face, font               (the session the others share)
            stave-ui-win32        → shell                    (Windows: HWND, GDI blit, DPI)
            stave-ui-x11          → shell                    (Linux: X11 window, PutImage, run loop)
formats     stave-vst3            → core, vst3-sys, face, the platform's ui-*
            stave-clap            → core, clap-sys, face, the platform's ui-*
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
            bypass: BoolParam::bypass(ParamId(3), "Bypass"),
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
BoolParam::bypass(ParamId(3), "Bypass");                         // the one way to declare bypass
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

The glossary lives in the guide, `crates/stave-guide/src/glossary.md`
(the docs site's Glossary page): its Terms table is the vocabulary, its
Retired names table the names that are gone, and
`scripts/check-vocabulary.sh` reads both, so the gate and the glossary
are one source.

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
- [x] **A11** (this commit) What migrating the seventeen plugins (P1) found the author surface missing, each fixed so no plugin keeps a local copy: the face prelude carries the Scene IR's words (`Pt`, `Brush`, `Font`, `Family`, `TextAlign`, `Cap`, `Op`), so draw code sets type flush or in a serif and fills gradients instead of estimating widths and painting bands; the prelude also has `Ramp`, `Watch`/`ModalWatch`, `AnyParam`, `Aux`, `Lfo` and `Generator`; `BoolParam::bypass(id, name)` names the bypass like every other param (the shipped plugins keep "bypass"); `Tabs` — a lit tab row bound to a stepped param (`Tabs::of(&p.algo)`), sharing the `Pager`'s row, with a part golden, a story and its laws; `#[derive(Smoothers)]` builds, prepares, snaps and ramps a group of `Smoother`s (`{Name}Ramps`, and `frame = T` for one `next()` per frame); `OnePole`/`DcBlocker::set_sample_rate` re-voice a moving clock without the reset `prepare` does; every circuit and kernel type is `Clone`, so a plugin settles in `prepare` and copies the settled state back in `reset`; `Generator::next()` for oscillators, LFOs, envelopes and walks; `RandomWalk` and `Lfo` resets restart their sequences (a reset sounds as fresh); `Block::aux` borrows the host's buffers, not the block, so a sidechain is read inside `lanes().run` (an aux a host aliases to an output is staged); `Oversampled2x::tick_with` runs a closure in the 2× domain (`()` is the wire stage for a closure-only interior); `stave::identity::<P>()` reads the CLAP id and VST3 class ids `export!` gives (the public `Exported` trait replaces `__export::Export`).

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
- [x] **F4** (e19b848: `stave_ui::solve(node, avail, measure)` asks a `Measure` for each `Auto` leaf's natural size; `stave_ui::Fonts` is the platform's text metrics — `stave_ui_cocoa::text::SystemFonts` (CoreText) and `stave_raster::BoxText` (tests, goldens; tracking now in em) — handed to `Runtime::new(face, fonts)`, which measures each part once through `Part::size(theme, fonts)` (`measured(part)` leaves; `leaf(part, w, h)` stays for fixed sizes); every catalogue part sizes from the theme's spacing and type scale and its measured print — titles by their measured name and tagline (no estimate, no strut), knobs by legend or widest value, a plate never narrower than its title; open: `Panel` and `Plate` margins are still structural constants, chosen when the node is built, before a theme is known) Part dimensions are magic numbers; titles estimate text width; layout never measures text. A `Measure` from the platform reaches layout; dimensions derive from the theme's spacing and type scale.
- [x] **F5** (07707b8: watched params and taps deduplicated once at construction; the hover key is bits) `Runtime::observe` re-sorts binds and probes and formats a debug string every frame. Precompute at construction; identify the hover target without formatting.
- [x] **F6** (07707b8: `Part::reads() -> Reads { params, taps }`, a Display's `.reads()`/`.taps()` fill it and its draw fn gets the params as `Frame::binds`; one `Runtime::frame(i)`) `Display` reads and `Part::reads` are two lists; `Frame` is built in three places. One declaration; one `frame(i)` helper.
- [x] **F7** (98cac32: `FaceView` and `Controller`) Six nouns for the host side of a face (`FaceState`, `FaceEditor`, `FaceTarget`, `EditorTarget`, `FaceMount`, `Bridge`). `FaceView` (platform) and `Controller` (portable).
- [x] **F8** (the layout half in 07707b8: `stave_test::face::grabs_land` — parts inside the face, a press at each grab's centre grabs that part — mounted on Tilt, and stave-face/tests/named_parts.rs; goldens: every storybook story in each state (ab34456, re-blessed in e19b848) and ff69ac8: tools/storybook/tests/parts.rs, a golden per catalogue part — 18 pictures, at rest and lit by readings, `FaceDriver::tap` to publish them) No layout or golden-image tests for faces.
- [x] **F9** (07707b8: `on_step(node, bind, step)`; the static layer is baked per step) Content can't follow a parameter's step (eddy's tabs no longer select its algorithm; vault shows every machine). Visibility by param step alongside pages.
- [x] **F10** (`Preset::from_pairs` in 5f01da0; `Frame::param(i)` reaches a bound param's range and text in 07707b8; 7cc6507: `Op::Clip`/`Op::Unclip` (`Scene::clipped`) — clips nest, text and images clip too — and `Brush::Radial` in the Scene IR and stave-raster; 133b8de: a `Display` clipped to its screen, `Meter::reduction(tap)` hanging from the top (`Hang`), `RouteParams::row(tap, label)` (the shared mod-route row), `Trace::bipolar()` and `Lamp::bipolar(tap)`) Graduations from the plugin conversions: a clip op or clipped Display, radial gradients, a gain-reduction meter (hangs from the top), a shared mod-route row part, `Preset::from_pairs` (five local copies), parameter ranges reachable from draw fns, bipolar `Trace`/`Lamp`.
- [x] **F11** (2ec7f4a: `objc.rs` — typed `send`, `sel!`, `Retained`, `MainThread`, `define_class` with typed `Method`s, `guard`; `CfRef`; no crate-wide allow; `forbid(unsafe_code)` in stave-face, stave-macros and the storybook; stave-test keeps its harness's one `unsafe` until A7) `stave-ui-cocoa`'s ~960 unsafe sites and crate-wide `allow(undocumented_unsafe_blocks)`. A small typed Objective-C core (typed `send` with stret, cached selectors, `Retained<T>`, a `MainThread` token, `define_class` with unique names and a panic guard); drop the allow; `forbid(unsafe_code)` in face, test and every safe crate.

### T — Testing and tooling

- [x] **T1** (f7778e4: `PluginHarness` is a stave-host `Instance`, so `render::Driver`, the measurements and the validation checks drive a linked plugin in-process; stave-play's `Engine` is already generic over `Instance`; 43ccd54: `PluginHarness::run` is a `drive::Driver` capture — the held inputs a looped `Recording`, the events a `NoteSchedule::of_events`, so an event lands at its frame from the run's start in whichever block that is — so there is one block loop) Three block drivers (`PluginHarness`, `render::Driver`, stave-play's `Engine`). One driver; the others build on it.
- [x] **T2** (f7778e4: `PluginHarness::builder()` — rate, block size, layout, starting values restored before prepare; `set_transport`, `aux_mut` (up to `MAX_AUX` buses, never allocating), `process_with(changes, events)` with sample-accurate `change(id, frame, value)`, `process_frames` (0 allowed), `prepare`/`reset`, `save`/`load`) The harness is too thin (no telemetry, sidechain, transport, sample-accurate params, layout choice), so 56 plugin test files build `ProcessCtx` by hand. A builder with `process_with(params, events)`.
- [x] **T3** (f7778e4: `stave_test::plugin_checks!(crate::Plugin, { perf, off_face, seeds })` — validation, no-alloc with every param moving, perf, face binds/grabs/laws, and the T4 checks, each a test; Tilt's three check files became one line) 54 per-plugin check files (`no_alloc.rs`, `perf.rs`, `validation.rs`) with stringly wiggle lists that drift. One `stave_test::plugin_checks!` macro; wiggle every param by default.
- [ ] **T4** (f7778e4: `stave_test::checks` — `block_sizes` (0, 1, odd, oversized, re-prepare), `sample_rates` (8 kHz–768 kHz), `prepare_and_reset_cycles` (must sound as fresh), `random_automation` (smoothed and sample-accurate, seeded), `state_round_trip`, `deterministic`; each pinned by a plugin broken on purpose; multi-threaded calls and crash isolation are V3's simulated host) Validation gaps vs pluginval/clap-validator: block-size changes, zero-length and odd blocks, sample-rate changes, activate/deactivate cycles, randomized automation, multi-threaded calls, crash isolation. One `checks/*.rs` each.
- [x] **T5** (43ccd54: one note model — stave-core's `Event`/`NoteEvent` (`NoteEvent::on`/`off`, `Event::note`) everywhere; `Instance::process` takes `&[Event]`, `NoteSchedule` slices core events (`Note{key, velocity f64}`), the VST3 host converts at its `IEventList`, the harness passes them straight through; one param model — `stave_host::param::Param` (was `control::Control`) built from core words: `ParamId`, core `ParamFlags`, `step_count` as `AnyParam` counts it, `Param::of(&dyn AnyParam)`; `Instance::params`/`set_param`, `ParamError`; the catalog's `Setting`/`Range`/`SettingValues` are gone — each stage declares core `FloatParam`/`EnumParam`s (`StageSpec::params`, `StageParams: Params`), the handle writes them and the stage reads them; `Unit::Custom { label, decimals }` names the units the closed list lacks (kΩ, nF); the check is `params_glide`; the retired names are in `check-vocabulary.sh`) Two note models (`stave_host::note::NoteEvent{offset,pitch,velocity f32}` vs core `NoteEvent{key,velocity f64}`) and two param models (host `Control`/`Setting`/`Range` vs `AnyParam`). One of each; convert at the ABI.
- [x] **T6** (f7778e4: one harness verb set — `process`/`process_with`/`process_frames`/`run` — and one check mount, `plugin_checks!`; 4d5e9a3: audio is processed or run, never rendered — `stave_host::drive::Driver` (`processed()`), `Signal::samples`, stave-play's `Engine::fill`, every test closure — and `check-vocabulary.sh` checks "render" across the whole tree outside the pixel code; every `*Policy` is a `*Config`; the host's `Probe`/`probes()` are `TapReading`/`taps()`; the dsp catalog is `stave_host::stages` (`stage:NAME`, `CatalogInstance`) so "module" is only the VST3 dylib; `loaded::Loaded`/`LoadError` are `opened::Opened`/`OpenError`; `tone_test.rs` is `tone_window.rs` (`ToneWindow`)) `render` means five things; `run_block`/`process_block`/`process` on one harness; check mounts in five shapes (`check_no_alloc`, `perf_report`, `assert_passes`, `binds_resolve`, `assert_no_pump`); `*Policy` vs `*Config`; `probe` and `module` overloaded; `Loaded`/`LoadError` beside `LoadedPlugin`/`HostError`; `tone_test.rs` reads like a test.
- [x] **T7** (ab34456; e061327: `crates/stave-raster/tests/properties.rs` — ink equals area under any translation and scales with scale²) (ab34456: `stave_raster::BoxText` deterministic text, `stave_test::image` — compressed PNG write/read, tolerant compare, `assert_golden!` with `STAVE_BLESS=1` — and `scripts/check-golden-sizes.sh` (3 MiB budget); raster property tests remain) stave-raster loses its only oracle with the CoreGraphics parity test. Golden PNGs of faces and scenes; property tests (coverage conserved under translation).
- [ ] **T8** (63d126f: source-over in 16.16 fixed point with one rounding, and a 256-entry gradient table per fill instead of a stop search per pixel; the reusable rasterizer scratch and sparse spans wait on `coverage.rs`, where the Kani harness now lives) Raster performance: per-fill allocations, `Vec<Vec<Pt>>` per op, four `round`s per pixel, per-pixel linear gradient search. A reusable rasterizer scratch, fixed-point source-over, sparse spans, gradient LUT.
- [ ] **T9** (e061327: the docs say what the stroker does — mitered joins, butt/round caps, and that overlapping pieces over-cover anti-aliased edge pixels — and the dead `one_to_one` branch is gone; this commit: strokes are outlines — an open subpath one polygon, a closed one two opposite rings, mitered joins with the inside of a turn cut where the offset edges cross, so a join never counts twice in an anti-aliased edge pixel (a segment too short for the cut goes through the vertex, as before); `Cap::Square`; an image minified past 2× box-filters the texels under each pixel; `tests/properties.rs`: a mitered corner inks width × arm length, a butt- or square-capped line inks its body plus caps; every committed golden still matches. Remaining, because each moves committed goldens: round caps as exact half discs (today whole discs over-ink thin knob pointers' tips), anti-aliased edges for images at fractional positions, box filtering between 1× and 2×; and even-odd fills, which need the winding sum before `coverage.rs` clamps it) Raster correctness: strokes are unions of quads + joins (overlap double-counts coverage; "exact" is false); bilinear aliases past 2× minification; hard-edged image rects; no even-odd rule or square cap; dead `one_to_one` branch; doc says round joins (they're miter).
- [x] **T10** (63d126f: `Canvas` fields private — `w()`, `h()`, `pixels()`, `pixels_mut()`, `copy_from` — so its pixels are always `w × h × 4`; `Mask` is crate-private; `BlockOut` fields private, `outputs()` an iterator that allocates nothing) Public fields that break invariants (`Canvas{w,h,px}`, `Mask`); `BlockOut` exposes `Vec`; `outputs()` allocates.
- [x] **T11** (this commit: a `FaceView` writes its own picture, drawn offscreen through the face runtime with CoreText at the backing scale, to the PNG `STAVE_FACE_SNAPSHOT` names once it has run a second; `stave-play --screenshot PATH --editor-only` sets it, so a stave editor is never a screen capture; `FaceView::snapshot(scale)` is pinned by the real-window test; another vendor's editor still falls back to its screen rectangle) `stave-play --screenshot` captures the screen region, not the editor (caught other agents' windows). Offscreen capture through the face runtime.
- [x] **T12** (1ee5eae: the loom models run the shipping types — under `cfg(loom)` stave-core's `runtime::ClaimCell` (the claim token every `PluginCell` now is) and stave-telemetry's `InlineStr`, `TapSlot`, `reserve_index` (taps and breakpoints both reserve through it) and the `Message` slot are built on loom's atomics, fence and cell (`stave-telemetry/src/sync.rs`; `zeroed!`/`zeroed_array!` keep the `const` constructors), exposed as `stave_telemetry::model`; a whole `Telemetry` stays out of the model — thousands of atomics — but every piece two threads race on is in it, as it ships) Loom models a copy of the protocol, not the shipping types. A `cfg(loom)` atomic shim on the real code.

### C — Continuous integration

CI has not been green on GitHub since the move: the zero-warnings and
clippy jobs fail on lints the floating `stable` toolchain added after the
local one (stave-play, the old Cocoa widgets), and clap-validator, built
from its git HEAD, no longer compiles. The plugins repo waits on its
framework deploy key.

- [x] **C1** (rust-toolchain.toml 1.96.0 in both repos; weekly canary on newest stable opens an issue) Pin the toolchain in `rust-toolchain.toml` (both repos) so local and CI agree; a scheduled job builds on the newest stable and opens an issue on new lints instead of breaking `main`.
- [x] **C2** An MSRV job so the floor is real. Measured: dependencies (icu 2.2 via url/eframe) need 1.86, so `rust-version` is 1.86, and CI checks it.
- [x] **C3** (clap-validator 0.4.1, VST3 SDK v3.8.1_build_84 in both repos; pluginval still to add) Pin external tools (clap-validator to a tag/rev, the VST3 SDK to a tag; add Tracktion's pluginval) and cache their builds.
- [x] **C4** (31b4b7e, f0e26ef: `scripts/sanitize.sh address|thread` over both wrappers' ABI tests, stave-host hosting Tilt through the full VST3 path (`vst3_host`, `sim_host`), the face crate and core/telemetry; the nightly `sanitizers` jobs in deep.yml; green in deep run 43265592981: ASan 2.2 min, TSan 2.5 min, macOS — stave-host hosts bundles through CoreFoundation) Sanitizers where Miri can't reach: AddressSanitizer and ThreadSanitizer (nightly) over the ABI tests, stave-host hosting a real plugin bundle, and the face runtime; a scheduled job.
- [x] **C5** (31b4b7e, 6eaf93e, this commit: `fuzz/` — `state_load` (any bytes load or are refused untouched; a load round-trips), `param_text` (any text, every unit: parse never panics; parse∘format is a fixed point), `vst3_host` (the simulated host steered by the fuzzer's tape through Tilt's VST3 event and param-change paths), `raster` (any polygon: coverage stays in the mask and in [0, 1]); `scripts/fuzz.sh`, 4 min per target nightly in deep.yml; nightly it found two bugs — a value too long for the value text lost its unit (now scientific, decided on the read-back value), an extent at infinity drew nothing; this commit: `clap_events` — Tilt through its CLAP entry with fuzzer-written input events (notes, param values and modulation for known and unknown ids, gestures, transports; any time offset, any event space) must answer every block without an error and with finite output. Fuzzing is nightly only, 4 min per target: a PR smoke would break the five-minute main signal the user asked for) Fuzzing (`cargo-fuzz`, short in PRs, long nightly): state chunk load (host-supplied bytes), param text parsing, the VST3/CLAP event and param-change paths, the rasterizer.
- [x] **C6** (framework half: Tilt's `stave_test::plugin_checks!` (T3: validation, no allocation while every parameter moves, perf, the extended T4 host-behaviour checks) runs in every CI `test (macOS)` job; the seventeen plugins get theirs with P3, in stave-plugins' own CI) Every plugin through the realtime checks in CI (no allocation, no locks in `process`), via `plugin_checks!` (T3), plus the extended validator checks (T4).
- [x] **C7** (the storybook's and Tilt's goldens (T7) run in CI's `test (macOS)` job — they were drawn on macOS arm64, so only there; a mismatch uploads `target/golden-diff` (actual + diff PNGs) as the `golden-diff` artifact; the plugins' faces join with P3) Golden images (T7): render the storybook and every plugin face offscreen, compare to committed PNGs within a tolerance, upload diffs as artifacts.
- [x] **C8** Plugins against every framework change: per PR and push, not nightly (the user's call). ci.yml's `plugins` job checks out stave-plugins, links it to the change (`scripts/link-framework.sh`) and runs clippy over every target, so breakage is seen before a release, not at the bump. Informational, not required: main may break the plugin API, since the plugins pin a tag.
- [x] **C9** (`scripts/check-vocabulary.sh` since 31340a2; 31b4b7e: `deny.toml` + `cargo deny` (licences, bans incl. duplicate versions outside the dev tools' trees, sources) and `cargo machete` in `scripts/hygiene.sh` and CI's `rules` job — machete skips stave-test until its unused `stave-telemetry` dependency goes; intra-doc links of private items too (`--document-private-items`); RustSec advisories nightly (they move without our commits; eight tool-only ignores, each with its reason); `scripts/semver-checks.sh` against the last `v*` tag, run by `scripts/release.sh` and on every push to a `release/*` branch) Hygiene gates: `scripts/check-vocabulary.sh` (the glossary), `cargo deny` (licences, advisories, duplicate versions), unused-dependency check, rustdoc link check; `cargo semver-checks` on the public crates once releases are published.
- [x] **C10** (31b4b7e, 41c2379: ci.yml is the main signal — every job fits five minutes and gates every push and PR: macOS only for test / lint (clippy + rustdoc + MSRV) / validators, Linux for the tree rules + hygiene, the portable check + loom, and Miri sharded four ways under nextest; deep.yml holds what cannot fit (Kani, sanitizers, fuzzing, the wide simulated-host sweep, advisories, semver) — nightly, never in the main run's way. The ledger's "one workflow" gave way to the five-minute budget; the nine main-signal jobs are required checks on `main` (admins may still push). First green run 43239699347: 4.4 min wall clock, every job ≤ 2.2 min) Name and shape: the workflow is `ci` (one per repo), quick jobs gate PRs, the heavy ones (Miri, sanitizers, fuzz, validators) run on `main` and nightly; required checks configured on `main`.
- [x] **C11** Moot: the repos are public (2026-11-12), so the plugins CI fetches the framework without a deploy key; public runners are also free, which unblocked CI billing.

### V — Verification (proofs, models, deterministic simulation)

What TLA+, Lean and Bend teach, applied where stave's risk is.

- [ ] **V1** (5f01da0: `Normalized` at the `Controller` boundary, `ParamId` everywhere (A4), typed taps (A6); the lifecycle as a state in this commit: `ProcessorState::prepare`/`release`, and `split_for_block` — the only way to the plugin's audio-thread borrow — is `None` unless prepared, so a host processing before activate gets silence and an `Unprepared` fault; still open: `Hz`/`Db`/`Seconds` newtypes across stave-dsp's setters — they cut across the D vocabulary and every plugin, so they land with P) Types as proofs: unit newtypes (`Hz`, `Db`, `Seconds`, `Normalized`) on the author and DSP APIs; typestate for the plugin lifecycle (`process` only on a prepared plugin); `ParamId`/`TapId` everywhere (A4, A6). Lands with the A/D vocabulary work.
- [x] **V2** (31b4b7e, f0e26ef, this commit: `#[cfg(kani)] mod proofs` + `scripts/kani.sh`, nightly per crate in deep.yml — `ranges_alias` is exactly interval overlap; `sort_param_changes` is a stable sort; `split_for_block` hands back null-free lists, inputs never aliasing outputs, outputs never aliasing each other, frames within the host's and the scratch's, every access in bounds; an unprepared state never splits; `Lanes` stays inside the block and zeroes undriven outputs; the id → index table answers as a scan; linear `Range` normalize/denormalize stay in range, pin endpoints, are monotone — Kani found that a span wider than f64 holds (−1e308..1e308) made `normalize` inf/inf = NaN, now taken at half scale (log/skew use `ln`/`powf`, outside Kani); the gesture ring is FIFO across the `u32` wrap and an admitted `Begin` always gets its `End`; `InlineStr` truncation is the longest fitting prefix and round-trips; the kernel `Ring` delays by exactly its length. Coverage accumulation is float-heavy and Kani's float encoding does not finish on it (a CI runner ran out of memory): it has an exhaustive grid test (NaN, infinities, off-canvas, every canvas to 3×3) and the `raster` fuzz target instead, which found an extent at infinity wrapping past i64::MAX so the shape drew nothing. All within small bounds (2–3 channels, ≤ 4 frames, ≤ 5 elements); every harness green in CI, one job each, 0.5–2.6 min (deep run 43265592981 and this commit's two lightened ones: the full-ring start near the `u32` wrap, denormalize's monotonicity left to the native sweep). Verus: no all-inputs invariant here needed more than Kani's bounds gave) Kani (bounded model checking) harnesses in CI for the unsafe and index-heavy core: `Ring`, `InlineStr` truncation, `AudioBuffer`/`Lanes` indexing, `IndexedParams` binary search, rasterizer coverage accumulation, the gesture queue. Verus where an all-inputs invariant is worth stating inline.
- [x] **V3** (31b4b7e: `stave_host::plugin::sim` — seeded call order over setup / activate / processing / process (0, 1, odd, max, past max frames; separate, in-place, null-channel buffers; unsorted and out-of-block param points) / controller writes / state save, load, garbage, truncation / a save on the main thread during a process on the audio thread, legal and illegal (`illegal_per_16`), each answer checked against `stave_model::lifecycle`; failures print `STAVE_SIM_SEED=<n>`, which replays them; 48 seeds per test run, 3000 nightly, a fuzzer-steered tape mode. First finding: a host's null output channel made `write_capture` index past an empty slice — a panic that disabled the instance) A deterministic simulated host in stave-host: seeded randomness over call order, block size (incl. 0 and odd), sample rate, activate/deactivate/setState interleavings and thread timing; any failure replays from its seed. Subsumes most of T4.
- [x] **V4** (31b4b7e, f0e26ef: `stave-model` — an explicit-state checker (BFS, invariants, liveness as "a quiescent state stays reachable", shortest counterexamples, `validate` for recorded traces), the lifecycle model the simulated host checks every answer against, and the gesture ring: the checker found that dropping the newest event can lose an `End` (begin, perform until full, release) so the host sees the gesture begun twice; `GestureQueue` now reserves each open gesture's `End` and refuses the `End` of a refused `Begin`, checked for 1–3 params and capacities 1–5 and proved in Kani for the shipping type; this commit: the lifecycle as a model the checker explores (it caught the model's own gap — activation before any setup) and the simulated host's every compliant run validated against it as a trace; param restore against smoothing (`restore`: the checker finds S1's value-first read snapping to the old value, the counter-first read always lands), an editor view's lifetime (`view`: a borrowed view outliving its controller is a use after free, the counted one never is and never leaks — S7), telemetry slot reservation (`slots`: each index handed out once, the count within capacity; it also shows two threads registering one name can take two slots, harmless since taps register on one thread); this commit: trace validation of real wrapper runs — after every simulated-host call the plugin's own V5 event trace is drained and held to the model (a Prepare at the last setup's rate and block size for each activation and nowhere else, a Release per deactivation, a Restore per load as accepted or refused, the fault a refused or unprepared block is, no record lost), and every compliant run's whole trace keeps V5's laws; DAW and stave-play sessions write the same trace (`STAVE_EVENTS`, checked by `stave-events`)) Protocol models (TLA+ with TLC/Apalache, or Stateright in Rust) for the host lifecycle across threads, param restore and smoothing, the gesture ring (begin/perform/end, liveness), telemetry slot reservation, and editor attach/detach/refcount/reentrancy. Trace validation: tests and stave-play emit event traces checked against the model.
- [x] **V5** (84f8e6a: `stave_telemetry::events` — a lock-free ring of typed records on the telemetry block (ABI 15): `Prepare`, `Release`, `Begin`/`Perform`/`End`, `Restore(ok)`, `Preset`, `Fault`, each with its index and block; `Recorder`, a text `Trace` that reads back, `check` (gestures begun once and ended once, edits inside their gesture and in 0–1, release after prepare, no gap, no fault) and `quiescent`; written by `PluginController` (every edit and preset), by both wrappers through `Diagnostics::event` (activate, deactivate, state loads), by every recorded `ProcessFault` and caught panic; `STAVE_EVENTS=1` writes each instance's trace to `<log dir>/<id>-<pid>-<n>.events` and `stave-events FILE` checks it; end to end on Tilt in stave-host/tests/event_trace.rs; open: validating these traces against the `stave-model` lifecycle and gesture models (V4), and a `Process` record (per block it would flood the ring)) Telemetry as a structured, checkable event trace (lifecycle, gestures, restores, faults) so observability doubles as verification, in tests and in real DAW sessions.
- [x] **V6** (31b4b7e: `scripts/check-purity.sh` in the gates and CI — stave-dsp, stave-ui, stave-raster, stave-face, stave-measure and stave-model read no clock, file, environment or randomness, start no thread, keep no global or hidden mutable state, touch no atomic or lock, except the listed and reasoned exceptions; `Trace` samples once per frame time (6e07cd1), so a repaint paints the same and a replay replays) Functional core, imperative shell: keep pure, deterministic code (faces, DSP graphs, layout, rasterizer) free of effects so golden images and replay hold; effects (threads, FFI, atomics) at the edges.
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
- [x] **U6** (the CI upload of `target/golden-diff` is C7's; ab34456: Tilt idle/hovered/dragging, every storybook story and every page/step state of the controls story, under `tests/golden`; CI upload of `target/golden-diff` is C7's) Golden images per interaction state: idle, hovered, dragging (value readout), each page, each theme; diffs uploaded by CI (C7).
- [x] **U7** (ab34456: `laws::sessions` — seeded xorshift sessions of hover/press/move/release/wheel/key/exit; a failure prints the seed, the step and the replayable trace) Model-based UI tests: random sequences of pointer events (seeded, replayable) against every face, checking invariants after each step (values in range, gestures always bracketed begin→end, no stuck drag after release, hover cleared on exit) — the V3 simulated host for the editor.
- [x] **U8** (e061327: `stave_ui_cocoa::synthetic::OffscreenWindow` — a never-shown NSWindow around a `FaceView`, fed real `NSEvent`s (window coordinates, modifier flags, click counts) and CoreGraphics line-unit wheel events; `tests/real_window.rs` is its own main-thread harness that nextest lists; removing the view's y-flip fails all of it) The real window, end to end: synthesize `NSEvent`s into the face view of a real (offscreen) window and check the same laws, so the platform's coordinate mapping (flip, scale, scroll units, modifier bits) is tested, not assumed. The y-flip bug fixed during the face work is the case this catches.
- [x] **U9** (e061327: every `FaceView` event goes through `Runtime::apply` and is appended to the file `STAVE_FACE_TRACE` names — `stave-play --face-trace PATH` sets it for a plugin in the player — and a real-window session replays headless to the same values; parameter values and edits are not in the trace, the replay derives them; ab34456: `stave_face::Trace` — one event per text line — and `Runtime::apply`, which the driver uses for every act, so a test's trace replays exactly; stave-play recording remains) Session traces: stave-play records a person's interaction as a replayable trace (pointer events + the gestures they produced + param values); a recorded session becomes a regression test.

### X — Cross-platform (Linux, Windows)

Measured 2026-11-12: `.github/workflows/platforms.yml` (build, clippy and
nextest on `ubuntu-latest` and `windows-latest`; [run
43238609491](https://github.com/stave-dsp/stave/actions/runs/43238609491))
and `cargo check --target x86_64-unknown-linux-gnu / x86_64-pc-windows-msvc`
from macOS. Nothing downstream of stave-host (stave-test, so every
plugin, face and golden test) built on either OS. The workflow now gates
every push: build + test, clippy + rustdoc, and validators, per OS —
green on `main` in [run 43256217031](https://github.com/stave-dsp/stave/actions/runs/43256217031)
(warm: build + test 2 min Linux, 3.5 min Windows; every other job under 2 min).

- [x] **X1** (d3196f3) (platforms.yml) No Linux or Windows signal: the workspace had never been built off macOS in CI. A job matrix per OS — `cargo build --workspace --all-targets`, clippy `-D warnings`, nextest + doctests (the X11 tests under `xvfb-run` with `STAVE_REQUIRE_DISPLAY=1`, so a missing server fails), validators — each job under five minutes warm.
- [x] **X2** (d3196f3) stave-host opened bundles only as CoreFoundation `CFBundle`s (`link kind framework` on both OSes) and loaded only with `dlopen` (Windows: no `dlopen`/`RTLD_*`, unix `OsStrExt`); thread CPU time was `clock_gettime` (Windows). `plugin/bundle.rs` reads the SDK layouts (`Contents/<arch>-linux/<Name>.so`, `Contents/<arch>-win/<Name>.vst3`, a bare `.so`/`.dll`/`.dylib`); `module.rs` loads with `LoadLibraryW`/`GetProcAddress` on Windows and enters through `ModuleEntry(handle)`/`ModuleExit` (Linux), `InitDll`/`ExitDll` (Windows, optional) or `bundleEntry`/`bundleExit` (macOS); on Windows a block is timed by the performance counter (`GetThreadTimes` ticks at 15.6 ms, which failed the deadline check in CI: every block read 0 or 15.6 ms). CI: `stave-bench <bundle> --validate` opens the built Tilt bundle on both OSes.
- [x] **X3** (d3196f3) stave-ui-cocoa compiled on every OS (10 errors each), and so did every crate on it. The crate is empty off macOS; the wrappers and the storybook take their platform's shell by `cfg`.
- [x] **X4** (d3196f3) No face on Windows. `stave-ui-win32`: a child `HWND` (class registered per binary, unregistered with its last window, so an unloaded DLL leaves none behind), the canvas blitted with `SetDIBitsToDevice` (and `WM_PRINTCLIENT`), pointer with capture, wheel (screen coordinates, 120ths of a notch), keys (`WM_GETDLGCODE` keeps arrows), cursors, the parent's DPI (`GetDpiForWindow`, `WM_DPICHANGED_AFTERPARENT`) or the host's scale, its own 60 Hz timer; one root (`GWLP_USERDATA`, a counted reference, released at `WM_NCDESTROY` if the host destroys the parent first); every `unsafe` block states its invariant. `tests/window.rs` drives a real child window with real window messages (`SendMessageW`): click, drag, Shift fine, Alt reset, double click, wheel, a 1.5× scale, the parent's DPI, and the window's own pixels read back through a DIB section equal the canvas.
- [x] **X5** (d3196f3) No face on Linux. `stave-ui-x11` (pure Rust, `forbid(unsafe_code)`: x11rb's own protocol, no libX11/libxcb): a child window of the host's, `PutImage` in request-sized bands, the visual's own pixel format, pointer, wheel buttons, keysyms, counted clicks (`stave_ui_shell::Clicks`), cursor-font cursors, `Xft.dpi` or the host's scale. It never blocks and starts no thread: the host's loop calls `pump` on the display connection's descriptor and a timer. `tests/xvfb.rs` drives it on a real X server through XTEST (the server's own input path): click, drag, Shift fine, Alt reset, double click, wheel, 2× scale, and every pixel of the window read back with `GetImage` equals the canvas. `run_app` (the storybook's window) loops the same pump.
- [x] **X6** (d3196f3) Text was CoreText only. `stave-font`: IBM Plex Sans SemiBold and Plex Mono Medium (SIL OFL 1.1, subset to Latin-1, 46 KB) embedded; `skrifa` parses outlines, stave-raster fills them (its own coverage, nonzero), so text anti-aliases like every other shape. `FontText` is both the `TextPainter` and the layout's `Fonts` (F4), so faces measure what they paint; pure (in `check-purity.sh`). `stave_ui_shell::Session` paints with it; `STAVE_TEXT=embedded stave-storybook --screenshot` previews it on macOS.
- [x] **X7** (d3196f3) CLAP offered `clap.gui` only on macOS. `win32` and `x11` window APIs; sizes in device pixels and `set_scale` honoured off macOS; on X11 `set_parent` joins the host's loop through `clap.posix-fd-support` (the connection's descriptor) and `clap.timer-support` (16 ms) and the plugin serves both extensions; without a host timer `set_parent` refuses (`CallError::WindowRefused`). `tests/gui_window.rs`: on Xvfb a fake host's run loop gets exactly one timer and one descriptor, the child window has the size the host was told, and `destroy` unregisters both and removes it; on Windows a real child HWND of the size the host was told, gone after `destroy`.
- [x] **X8** (d3196f3) VST3 `createView` returned null off macOS. `HWND` and `X11EmbedWindowID` platform types; sizes in device pixels; Linux joins the frame's `IRunLoop` (`setFrame`) with an `IEventHandler` and an `ITimerHandler` living in the view's box, their references the view's own; `removed` unregisters and releases. `tests/view_window.rs`: a fake `IPlugFrame` + `IRunLoop` on Xvfb (registration, the handlers' `queryInterface`, pumping, geometry, teardown), a real child HWND on Windows; the headless view tests (gesture conversation, a view outliving its controller, a panicking face) run on all three OSes.
- [x] **X9** (d3196f3) VST3 exported only `bundleEntry`/`bundleExit`. `export_vst3!` adds `ModuleEntry`/`ModuleExit` (Linux) and `InitDll`/`ExitDll` (Windows); `scripts/check-exports.sh` loads each plugin binary as a host does (ctypes: dlopen/LoadLibrary) and resolves `GetPluginFactory`, `clap_entry` and the platform's entry pair, in CI on both OSes.
- [x] **X10** (d3196f3) Validators ran on macOS only. clap-validator 0.4.1 and Steinberg's validator (SDK v3.8.1_build_84, cached per OS) over Tilt on Linux and Windows: CLAP 33 passed / 0 failed (10 skipped), VST3 47 passed / 0 failed, on both.
- [x] **X11** (d3196f3) The bundle scripts made macOS bundles only (`.dylib`, `Info.plist`, `codesign`, `~/Library/...`). `scripts/lib/bundle-common.sh` has one path per OS (uname): macOS bundles; Linux `Contents/<arch>-linux/<Name>.so` and a `.clap` `.so`, installed to `~/.vst3`, `~/.clap`; Windows (Git Bash) `Contents/<arch>-win/<Name>.vst3` and a `.clap` DLL, installed to `%LOCALAPPDATA%\Programs\Common\{VST3,CLAP}`; `validate-formats.sh` finds `validator.exe`. `release.sh` was already portable bash.
- [x] **X12** (d3196f3) stave-play: winit had no window system on Linux (eframe without `x11`), pprof does not build on Windows, cpal needs ALSA headers (CI installs `libasound2-dev`), and the editor embed is Objective-C. eframe gets `x11` on Linux; the flamegraph capture is unix-only; off macOS the player shows no plugin editor (`editor_none.rs`).
- [x] **X13** (d3196f3) stave-bench reserved stdout with unix `dup`/`dup2` and `from_raw_fd`. On Windows it duplicates the standard output handle and points both the Win32 handle and the C runtime's fd 1 at stderr.
- [x] **X14** (d3196f3) stave-dsp `langevin` lost accuracy to cancellation (ε/x) just past its series switch at 1e-4, so glibc's `tanh` failed the continuity test that Apple's passed. The series (three terms) holds to 1e-2, where the closed forms are accurate to 2e-14 on every libm.
- [x] **X15** (d3196f3) stave-core's panic-log test expected `/` in the panic location; the compiler writes `\` on Windows.
- [x] **X16** (d3196f3) Golden images: platform-independent. Every golden (Tilt, the storybook stories and states, every part) passes on Linux and Windows at the one default tolerance (any channel ≤ 3, or past it at ≤ 0.1 % of pixels) against the PNGs blessed on macOS: faces are drawn by stave-raster (the same Rust on every OS) with `BoxText`, never platform text, and what libm still touches (arc flattening, gradients) stays inside the tolerance. CI uploads `target/golden-diff` per OS on failure.
- [ ] **X17** Unverified (no headless CI for these): a real DAW hosting the editor on Linux or Windows (Bitwig, Reaper, Ardour; focus and key routing in a host's window, its run loop's timing); Wayland hosts (X11 only, through XWayland); HiDPI on a real X11 display (`Xft.dpi` parsing is tested, a 2× display is not) and per-monitor DPI changes on Windows (`WM_DPICHANGED_AFTERPARENT` is handled but never sent in CI); VST3 `IPlugViewContentScaleSupport` (not implemented: Windows VST3 hosts get the parent's DPI only); real Alt and Shift key input on Windows (the test sets the thread's keyboard state); cursor shapes (set, never looked at); the stand-alone windows (`run_app`, the live storybook) on Windows and Linux; MIT-SHM (every frame is a `PutImage` over the socket); stave-play's editor embed off macOS.

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
