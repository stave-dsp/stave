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
model       stave-core            → telemetry, macros          (no UI, no DSP)
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
    fn process(&mut self, block: &mut Block<'_, Self>) { /* … */ }
    fn face(params: &TiltParams) -> Option<Face> { /* … */ }
}

stave::export!(Tilt);
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

- [ ] **S1** Restored values can glide in: `params/control.rs:86` reads the value before the restore counter, the writer stores value then counter. Read the counter (Acquire) first; the writer's increment Release.
- [ ] **S2** `stave-test/src/harness.rs:73` `inputs_mut() -> &mut [Vec<f32>]`: a caller can reallocate a channel and the cached pointers dangle (UB from a safe API). Store `Box<[f32]>`, hand out `&mut [f32]`, rebuild pointers per block.
- [ ] **S3** `AudioBuffer::channel/_ref` build slices from null host pointers (the debug check skips nulls). Wrappers substitute zeroed scratch; `from_raw` asserts non-null.
- [ ] **S4** `stave-ui-cocoa/src/face.rs` `parts()` aliases `&mut FaceEditor` with `&Slots` (UB under Stacked Borrows). Split by destructuring.
- [ ] **S5** The editor is reached through four raw-pointer roots (`EditorBox`, `self_ptr`, `EditorTarget`, ivars) with no reentrancy guard. One `NonNull` root; a busy flag drops reentrant events.
- [ ] **S6** VST3 `tresult` constants are the COM-compatible values; macOS/Linux use `kNoInterface = -1`, `kResultFalse = 1`, `kInvalidArgument = 2` … `kOutOfMemory = 6` (`stave-vst3/src/sys.rs:40`). Split by `cfg(windows)`.
- [ ] **S7** `IPlugView` refcount is faked (`wrapper/view.rs:23`); a host holding the view past the controller gets a use-after-free. Real count holding an Instance reference.
- [ ] **S8** Panics cross FFI: Cocoa methods and timers, VST3 `createInstance`/`createView`/`sv_*`, CLAP `create_plugin`/`init` are unguarded; MSRV 1.80 makes that UB. MSRV 1.81 and one `ffi_entry` guard for every `extern "C"`.
- [ ] **S9** `objc_msgSend` transmutes returning `CGRect` need `_stret` on x86_64; `bool` for `BOOL` is wrong there. A typed send that handles both, or a compile error off aarch64 until it does.
- [ ] **S10** Objective-C class names are global (`StaveFaceView`, …): two stave plugins in one DAW share one class. Per-binary unique suffix.
- [ ] **S11** Stable `sort_by_key` on the audio thread can allocate (`processor.rs:118`, `clap process.rs:73`). Insertion sort keyed by (frame, sequence).
- [ ] **S12** Allocation and locks on the audio thread: `split_for_block` grows `Vec`s past the declared block; `PanicGuard` logs through a `Mutex` + file open. Truncate and count as a fault; report from the main thread.
- [ ] **S13** Probe names > 31 bytes never match their truncated slot, so every block reserves a new one until the bus is full (`probe.rs:83`, `inline_str.rs:49`). Reject at compile time (typed taps make this moot).
- [ ] **S14** `Ring::delayed` indexes before `prepare` (a `Loop::tick` before prepare panics). Size in `new` or read with a 0 fallback.
- [ ] **S15** One NaN locks Svf/Biquad/Loop/Chain feedback until reset. NaN → 0 in `denormal::flush`, and a guard in `Loop`.
- [ ] **S16** Param lookup is a linear scan through `dyn` calls on the audio thread (`types.rs:522`; CLAP bypass every block, VST3 every event). An id → index table built at instance creation; cache the bypass param.
- [ ] **S17** Leaks: FaceMount view alloc, NSTrackingArea, CFRunLoopTimer never released; the 30 Hz editor timer keeps running with a face attached.
- [ ] **S18** `text.rs` `CFRetain(NULL)` if NSFont returns nil; font cache unbounded.
- [ ] **S19** `stave-host` `RTLD_LOCAL = 0x4` is `RTLD_NOLOAD` on Linux (`plugin/module.rs:104`). Use `libc::RTLD_*`.
- [ ] **S20** `EditorBox` `unsafe impl Send/Sync` is unneeded; stale SAFETY/comments (`capture.rs:24`, `sys.rs:5`).

### L — Layering

- [ ] **L1** `stave-core` depends on `stave-face` (`Spec::face`, `editor_face`, `BindParam`) and on `stave-dsp` (`RouteSlot`, `VoiceEvent`). Core defines the portable **Controller** contract; face depends on core; route/voice conversions move downstream. `Bind` becomes `ParamId`.
- [ ] **L2** Portable editor logic (`Bridge`/`Slots`: param ids, taps, presets, capture over the table and telemetry) lives in the macOS crate. Move to core as the Controller implementation; the platform shell keeps only window, pointer, blit, text.
- [ ] **L3** No `*-sys` crates: `stave-host` depends on all of `stave-vst3` (and so on Cocoa) for its ABI types. Split `stave-vst3-sys` and `stave-clap-sys`.
- [ ] **L4** Faces are VST3-only: no CLAP `gui` extension (and verify `note-ports` for instruments). `stave-vst3` hard-depends on the Cocoa crate.
- [ ] **L5** Two gesture-sink traits (`EditorHost`, `face::Host`) for one concept. One `Controller` in core; wrappers adapt it.
- [ ] **L6** `stave-debug` is the shipping telemetry transport, with 11 `pub` atomic fields. Rename `stave-telemetry`, make fields private, typed `Tap` handles for shipping, string `probe!` for tooling only.
- [ ] **L7** The legacy UI layer: delete `stave-core` `editor.rs`/`editor_kit.rs`/`Widget`/`UiNode`/`ui!` editor path, `stave-ui` `tokens.rs`/`scenes/`, the legacy Cocoa widgets, mount/tick/view widget state, `stave-test` `editor.rs`/`raster.rs`, `editor_ui` in the wrappers. First move what survives: `Preset` (to core), `photo` (to face), objc helpers (`sel`/`class`/`msg_*`/`CGRect`, `detach`) out of `widgets/`, `VIEW_WHEN_BIT` users.
- [ ] **L8** `stave-ui` `Node` still carries CSS-like styling (bg, border, radius) that competes with "parts never mint a colour". Keep layout only.
- [ ] **L9** `stave-host`: the CLI parser and usage text belong in `tools/stave-bench`; `loaded.rs` depends on `bench::describe`; `modules.rs` mixes a 900-line catalog with `ModuleInstance`. `stave-measure` claims pure functions but has `wav.rs` file I/O (move to host).
- [ ] **L10** An umbrella `stave` crate with the prelude and `stave::export!` (VST3 + CLAP from one line, CIDs derived stably from `Info::id`).

### A — The author's API

- [ ] **A1** Two plugin traits (`Plugin` wide/public, implemented only by `StavePlugin`; `Processor` + `Spec`), renamed twins (`debug_attach`/`attach_telemetry`, `editor_ui`/`editor`, `editor_face`/`face`, `param_table`/`params`, `activate(f32, usize)`/`activate(Environment)`), dead `Plugin::set_param`. One author trait `Plugin`; the wrapper-facing erased trait is internal.
- [ ] **A2** Wrapper internals in the author namespace (`PluginCell`, `ProcessorState`, `ScopedFtz`, `ranges_alias`, `GestureQueue`, unsafe `split_for_block`/`feed`/`mix`). Move under `#[doc(hidden)] pub mod __wrapper` (or the format crates).
- [ ] **A3** Five ways to read a param (`plain()`, `value()`, trait `plain()`, `get()`, `index()`); `DiscreteControl` takes id + fn pointer + raw f64 and ignores restores. One `value()` in the natural type; `Smoother::new(&FloatParam)` and a stepped equivalent built from the param.
- [ ] **A4** Raw `u32` where `ParamId` belongs (`FloatParam::new`, `Migrations::renamed`, `GestureEvent`, `StateStore`); public `Range` fields allow an invalid log range; `ParamFlags` bag lets a float be "bypass". `ParamId` everywhere, private `Range`, `BoolParam::bypass`.
- [ ] **A5** Numeric drift: sample rate `f32` here, `f64` in `ContinuousControl::prepare`/`BypassGate`/circuits; frame counts `usize` here, `u32` there. One type each (glossary).
- [ ] **A6** The engine ↔ face channel is a string: engines `Tap::new("spin")`, faces `.probes(&["spin"])`, a typo reads 0. `#[derive(Taps)]` table referenced by both; telemetry handed to `Plugin::new`.
- [ ] **A7** `AudioBuffer`: input exposed mutably; no way to borrow two channels mutably (why `Lanes` re-derives raw pointers). `InputBuffer`/`OutputBuffer`, `channels_mut()`.
- [ ] **A8** History in code comments ("user-specced", "Step 1.3", "chime consumer … fifth instrument lock", "forge's boot()", milestone tags) and cargo-culted `extern crate alloc` in std crates. Remove.
- [ ] **A9** `ui!` fails silently (bad `cols` → 1, bad `kind` → Rect, dropped attrs). Becomes moot with L7 for editors; any surviving macro emits `compile_error!`.
- [ ] **A10** Tilt clones the params `Arc` every block and hand-wires lanes, ramps and coefficients. Tilt is rewritten to the target shape as the acceptance test.

### D — DSP vocabulary

- [ ] **D1** Most leaves aren't `Stage`s (Svf, Biquad, Allpass1, LinkwitzRiley4, Triode, PushPull, SmoothSat, PassiveToneStack, control primitives): five verbs for one step. Every leaf implements `Stage`; multi-output filters keep their extra methods.
- [ ] **D2** The tail contract stops at the kernel: no leaf overrides it, `StageKind` forwards neither latency nor tail, `Chain` reports `Some(0)` with feedback on, `Par2` omits its pads. Forward everywhere; filters estimate a −120 dB tail; feedback chains report unbounded.
- [ ] **D3** `prepare` means three things (`DelayLine::prepare(max_samples)`, `f64` rates in PowerSupply/WDF). `prepare(sample_rate: f32)` everywhere; `DelayLine::allocate(max_samples)`.
- [ ] **D4** Two setter styles (rate passed per call vs cached at prepare). Always cache the rate; setters take musical units only.
- [ ] **D5** The one-pole smoothing coefficient is written five times in the crate (and ~18 in plugins). `primitives::filters::OnePole` with `tau_coeff`, plus `DcBlocker`.
- [ ] **D6** Missing primitives plugins re-implement: `DelayAllpass` (Schroeder), `Comb`, `DcBlocker`, `soft_clip`, `Crossover3`.
- [ ] **D7** Two 2× oversamplers (legacy IIR `Oversampler2x` still root-exported beside the FIR internals). `Oversampled2x` is the one public form.
- [ ] **D8** Two `Route` types (`kernel::control::Route`, `modmatrix::Route`); ModMatrix is not built on `Router` as documented. Rebuild it on `Router`.
- [ ] **D9** Silent clamping hides bugs: `Svf::process` clamps the channel index; `Chain::push` returns a sentinel id. Svf mono under `PerChannel`; `Option`/`Result` for capacity.
- [ ] **D10** Units and names: `rate`/`sample_rate`/`sr`/`fs`; `set_hz`/`set_rate`/`set_frequency`/`set_corner`; `_ms`/`tc_secs`/seconds/samples; phase 0–1 f64 vs radians f32; `q` vs `resonance`; three overlapping shape enums; three "Filter" types at the root. One name per concept (glossary), one shape enum.
- [ ] **D11** Docs out of step: the kernel names forge and multiband; Chain says "inside ONE oversampler"; lib.rs describes 3 layers of 5; `tan32` is `x.tan()`; `hz_of_key` in the kernel.

### F — The face layer

- [ ] **F1** Four nouns for one idea (`Control`, `Hit`, `Gesture`, `Target`) and two ways to declare grabs (`controls()`, `hit()`). `Grab { area, gesture }` and one `grabs(&self, r, f)` that may read live values.
- [ ] **F2** `Theme` is a flat ~25-field struct mixing palette, materials, hardware, typography, spacing and coverings; `type_`. Group into `Palette`, `Finish`, `Hardware`, `Typography`, `Spacing`, `Coverings`.
- [ ] **F3** Sentinels and flag fields: `Lamp::pilot` is `Probe("", −∞)`; `Footswitch { invert, momentary, color }`. `LampSource::Always`; `Latch`/`Momentary` and `LitWhen` enums.
- [ ] **F4** Part dimensions are magic numbers; titles estimate text width; layout never measures text. A `Measure` from the platform reaches layout; dimensions derive from the theme's spacing and type scale.
- [ ] **F5** `Runtime::observe` re-sorts binds and probes and formats a debug string every frame. Precompute at construction; identify the hover target without formatting.
- [ ] **F6** `Display` reads and `Part::reads` are two lists; `Frame` is built in three places. One declaration; one `frame(i)` helper.
- [ ] **F7** Six nouns for the host side of a face (`FaceState`, `FaceEditor`, `FaceTarget`, `EditorTarget`, `FaceMount`, `Bridge`). `FaceView` (platform) and `Controller` (portable).
- [ ] **F8** No layout or golden-image tests for faces.
- [ ] **F9** Content can't follow a parameter's step (eddy's tabs no longer select its algorithm; vault shows every machine). Visibility by param step alongside pages.
- [ ] **F10** Graduations from the plugin conversions: a clip op or clipped Display, radial gradients, a gain-reduction meter (hangs from the top), a shared mod-route row part, `Preset::from_pairs` (five local copies), parameter ranges reachable from draw fns, bipolar `Trace`/`Lamp`.
- [ ] **F11** `stave-ui-cocoa`'s ~960 unsafe sites and crate-wide `allow(undocumented_unsafe_blocks)`. A small typed Objective-C core (typed `send` with stret, cached selectors, `Retained<T>`, a `MainThread` token, `define_class` with unique names and a panic guard); drop the allow; `forbid(unsafe_code)` in face, test and every safe crate.

### T — Testing and tooling

- [ ] **T1** Three block drivers (`PluginHarness`, `render::Driver`, stave-play's `Engine`). One driver; the others build on it.
- [ ] **T2** The harness is too thin (no telemetry, sidechain, transport, sample-accurate params, layout choice), so 56 plugin test files build `ProcessCtx` by hand. A builder with `process_with(params, events)`.
- [ ] **T3** 54 per-plugin check files (`no_alloc.rs`, `perf.rs`, `validation.rs`) with stringly wiggle lists that drift. One `stave_test::plugin_checks!` macro; wiggle every param by default.
- [ ] **T4** Validation gaps vs pluginval/clap-validator: block-size changes, zero-length and odd blocks, sample-rate changes, activate/deactivate cycles, randomized automation, multi-threaded calls, crash isolation. One `checks/*.rs` each.
- [ ] **T5** Two note models (`stave_host::note::NoteEvent{offset,pitch,velocity f32}` vs core `NoteEvent{key,velocity f64}`) and two param models (host `Control`/`Setting`/`Range` vs `AnyParam`). One of each; convert at the ABI.
- [ ] **T6** `render` means five things; `run_block`/`process_block`/`process` on one harness; check mounts in five shapes (`check_no_alloc`, `perf_report`, `assert_passes`, `binds_resolve`, `assert_no_pump`); `*Policy` vs `*Config`; `probe` and `module` overloaded; `Loaded`/`LoadError` beside `LoadedPlugin`/`HostError`; `tone_test.rs` reads like a test.
- [ ] **T7** stave-raster loses its only oracle with the CoreGraphics parity test. Golden PNGs of faces and scenes; property tests (coverage conserved under translation).
- [ ] **T8** Raster performance: per-fill allocations, `Vec<Vec<Pt>>` per op, four `round`s per pixel, per-pixel linear gradient search. A reusable rasterizer scratch, fixed-point source-over, sparse spans, gradient LUT.
- [ ] **T9** Raster correctness: strokes are unions of quads + joins (overlap double-counts coverage; "exact" is false); bilinear aliases past 2× minification; hard-edged image rects; no even-odd rule or square cap; dead `one_to_one` branch; doc says round joins (they're miter).
- [ ] **T10** Public fields that break invariants (`Canvas{w,h,px}`, `Mask`); `BlockOut` exposes `Vec`; `outputs()` allocates.
- [ ] **T11** `stave-play --screenshot` captures the screen region, not the editor (caught other agents' windows). Offscreen capture through the face runtime.
- [ ] **T12** Loom models a copy of the protocol, not the shipping types. A `cfg(loom)` atomic shim on the real code.

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
5. **T** — the harness, checks and validator, golden images, raster.
6. **P** — migrate the plugins once, and cut a release.
