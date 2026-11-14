# Framework roadmap

The working ledger for the framework around the DSP: plugin formats,
hosts it has been proven in, how a plugin ships, and the GUI and MIDI
features a commercial plugin needs. `DSP.md` is the DSP library's ledger
and `OVERHAUL.md` the framework's internal fit; this file is about
*reach*: what it takes for a stave plugin to go everywhere a JUCE plugin
goes.

Every item has an id (`F-…`), acceptance criteria, how it is verified,
and a status. Tick an item with the pull request that lands it, as
`PR #N (<merge commit>)`. The merge queue squashes, so a branch's own
commit hashes never reach main: cite the PR, never a branch commit. An
item is done when its verification runs in CI, or, where no CI can run
it (a commercial DAW, a signing identity), when a recorded manual pass
is linked from the item.

## Goals

- **One plugin, every format.** `stave::export!(MyPlugin)` is the only
  line an author writes; every format a host can load comes out of it,
  with the same identity, parameters, state and face.
- **Supported means proven.** A format, host or platform is claimed only
  after a recorded pass: a validator in CI, a host driven by a test, or
  a dated manual checklist. Nothing is "should work".
- **The wrappers stay thin and shared.** Each format is a translation of
  its ABI onto `stave_core::runtime` (the processor claim, the state
  machinery, the bypass gate, telemetry, the panic guard). A behaviour
  lives once, in the runtime, never per format.
- **Unsafe stays fenced.** Raw ABI types live in a `*-sys` crate with
  layout assertions; the wrapper's `unsafe` is the FFI boundary only,
  every block stating its invariant; everything above is
  `#![forbid(unsafe_code)]`.

## Where stave stands against JUCE

JUCE is the reference: twenty years old, in nearly every commercial
plugin, with every format and a full GUI toolkit. Stave is not trying
to be JUCE; it is trying to make a better plugin with less code and
more proof. An honest comparison, November 2026:

### Where stave leads

- **Verified DSP.** Circuit models are held to ngspice on the same
  netlist (83 decks), devices to digitised datasheet curves, physical
  models to high-precision solutions, each with an error budget a test
  enforces (`DSP.md`). JUCE's `dsp` module is textbook primitives with
  no claim of hardware accuracy and no reference to drift from.
- **Measurement built in.** `stave-measure` (THD, aliasing, loudness,
  perceptual metrics, system identification), `stave-host`'s validation
  suite, `stave-bench` and fingerprints run on every plugin and on any
  third-party VST3 or Audio Unit. In JUCE that is a separate tool a team
  builds itself.
- **Safety by construction.** The ABIs are written by hand and run under
  Miri; the lock-free protocols under Loom; Kani proves the core's
  invariants; every host entry point is panic-guarded; realtime
  allocation is a test failure (`assert_no_alloc` in `plugin_checks!`).
  JUCE is C++: memory and thread safety are the author's discipline.
- **One line of checks.** `stave_test::plugin_checks!` gives a plugin
  validation, no-alloc, a CPU budget, golden faces and host behaviour.
- **Docs from code.** The guide's articles are doctests, the reference
  pages and the model catalogue are generated, and nothing on the site
  is written twice, so the docs cannot drift from the API.
- **No SDK licences.** VST3, CLAP and AU are written from the public ABIs
  in Rust, MIT-licensed; JUCE is GPL or a paid licence.

### Where stave trails

- **Formats.** JUCE ships VST3, AU v2, AUv3 (macOS and iOS), AAX, LV2,
  standalone and Unity. Stave ships VST3 and CLAP, and AU v2 is in
  progress (F-AU); no standalone, LV2, AAX or AUv3 yet.
- **GUI maturity.** JUCE has a full widget toolkit with text editing,
  accessibility (VoiceOver, UIA), OpenGL and per-monitor DPI handled in
  years of hosts. Stave's faces are a small, verified design system
  drawn by its own rasteriser: no text entry, no accessibility tree,
  HiDPI tested on macOS only (F-GUI).
- **Shipping tooling.** JUCE (with Projucer, CMake and community
  scripts) produces installers, code signing and notarisation recipes
  that many teams have used. Stave bundles and ad-hoc signs; it has no
  installer, no Developer ID signing, no notarisation (F-SHIP).
- **Battle-testing.** JUCE plugins have run in every DAW on every OS for
  two decades, and its wrappers carry fixes for each host's quirks.
  Stave is validated by clap-validator, Steinberg's validator and its
  own host, and has been tried by hand in a few DAWs; no host is proven
  by a recorded pass yet (F-DAW).
- **Ecosystem.** JUCE has a large community, forum, third-party modules,
  books and courses, and developers who already know it. Stave has this
  repository, its guide, and the plugins in stave-plugins.
- **MIDI.** JUCE has MIDI 2.0 UMP and MPE helpers. Stave's events are
  note on/off/choke with a note id (F-MIDI2).

## Ledger

Ordered by priority (user decision, 2026-11-13).

### F-AU — Audio Unit v2 export (macOS)

`stave::export!` makes the crate an Audio Unit as well: a `.component`
bundle that AudioToolbox registers.

- **Acceptance**
  - An `stave-au` crate (raw types in `stave-au-sys`) implementing the
    AU v2 plug-in interface (`AudioComponentPlugInInterface`, every
    selector Lookup dispatches) over `stave_core::runtime`: the shared
    processor claim, the state machinery, the bypass gate, telemetry,
    the panic guard.
  - Component type from the plugin's kind: `aumu` for an instrument,
    `aumf` for an effect that takes notes, `aufx` otherwise; manufacturer
    and subtype codes from `Info::au` (derived from the vendor and id when
    unset, so every existing plugin builds as an AU unchanged).
  - The bundle step writes `Info.plist`'s `AudioComponents` entry from
    the codes compiled into the binary (never typed twice), in
    `scripts/bundle-au.sh`, `bundle-all.sh` and `install.sh`.
  - Audio through `AudioBufferList` (host or plugin buffers, in place),
    input pulled through the host's input callback or a connection, sidechains
    as extra input elements; parameters (ranges, units, value strings,
    text conversion, listener notifications for edits from the face);
    state through `ClassInfo`; latency, tail, bypass; MIDI for
    instruments and note effects (`MusicDeviceMIDIEvent`); host
    transport (`HostCallbacks`); supported channel counts from
    `LAYOUTS`; the face in a Cocoa view (`AudioUnitCocoaViewInfo`).
- **Verification**
  - `auval -v <type> <subtype> <manufacturer>` passes for Tilt and for
    an instrument, in CI on macOS (the component installed into
    `~/Library/Audio/Plug-Ins/Components` on the runner).
  - `stave-host`'s Audio Unit backend loads the built component and the
    framework's validation suite passes on it.
  - Manual: GarageBand loads both, automation records and plays back,
    a saved song reopens with its settings (procedure in the guide's
    formats page).
- **Status:** In progress.

### F-DAW — Real-DAW compatibility matrix

A published matrix of hosts × formats × platforms, every cell backed by
a recorded pass.

- **Acceptance**
  - REAPER (scriptable, licence-free to evaluate) driven automatically
    on CI: load each format, bounce a project, compare the bounce with
    `stave-host`'s offline output, automate a parameter, save and reopen.
  - Ableton Live semi-automated with the user: a scripted session
    (AbletonOSC) with the steps a person must do listed and timed.
  - Manual checklists for Logic Pro, Pro Tools (once F-AAX decides),
    Bitwig, Cubase, Studio One, FL Studio, GarageBand: load, process,
    automate, save/reopen, editor open/close/resize, offline bounce,
    sample-rate and buffer changes, each a dated record.
  - The matrix is published on the docs site and only claims
    "supported" for a cell after a recorded pass; a cell without one
    says "untested".
- **Verification:** the REAPER job in CI; the checklists' records linked
  from the matrix.
- **Status:** Paused (user decision, 2026-11-14).

### F-STANDALONE — Standalone application wrapper

Any stave plugin runs as its own application.

- **Acceptance**
  - `stave::export!` (or a one-line `main`) builds a standalone app: an
    audio device (input and output, rate and buffer choice), MIDI input
    for instruments, the face in a window, state saved between launches.
  - macOS `.app` bundle; Windows and Linux executables.
- **Verification:** a headless test drives the standalone's engine with
  a null device; a smoke test launches the app on each OS in CI.
- **Status:** In progress (parallel work).

### F-SHIP — Installers, signing and notarisation

A plugin can be shipped to users who are not developers.

- **Acceptance**
  - macOS: Developer ID signing of every bundle (VST3, CLAP, AU, app),
    hardened runtime, notarisation and stapling, a signed `.pkg`
    installer choosing formats; Windows: an installer (MSI or Inno) and
    Authenticode signing; Linux: a tarball with install script.
  - Identities come from local configuration or CI secrets, never the
    repository; an unsigned build still works for development.
- **Verification:** `spctl --assess` and `stapler validate` on the
  products in a release job; installers installed and uninstalled on CI
  runners.
- **Status:** In progress (parallel work).

### F-GUI — GUI essentials: text entry, accessibility, HiDPI

- **Acceptance**
  - **Text entry:** a part that edits a value as text (click a readout,
    type `-3.5 dB`), with IME-safe input, selection, copy/paste, and
    the host's key focus handled per platform.
  - **Accessibility:** an accessibility tree from the face's parts
    (NSAccessibility on macOS, UI Automation on Windows, AT-SPI on
    Linux): every control named, valued, adjustable from the keyboard.
  - **HiDPI:** per-monitor scale changes on Windows, VST3
    `IPlugViewContentScaleSupport`, a 2× X11 display, faces resizable
    where the host allows.
- **Verification:** the face's laws extended (text entry round-trips a
  value through `parse`/`format`); VoiceOver and Accessibility Inspector
  checks recorded per release; scale tests at 1×, 1.5×, 2× per OS.
- **Status:** Open. (HiDPI: macOS backing scale and Windows parent DPI
  exist; see OVERHAUL.md X17 for what is unverified.)

### F-MIDI2 — MIDI 2.0 and MPE

- **Acceptance**
  - Per-note expressions (pitch bend, pressure, timbre) as events with
    a note id, from CLAP note expressions, VST3 note expression, AU
    MIDI and MPE zones in MIDI 1.0.
  - MIDI CC, pitch bend, channel pressure and program change as events
    for plugins that want them; MIDI 2.0 UMP in hosts that send it
    (CLAP's MIDI 2 dialect, AU's `MIDIEventList`).
- **Verification:** each format's translation unit-tested against the
  spec's byte layouts; an MPE instrument driven through `stave-host` in
  every format gives the same audio.
- **Status:** Open.

### F-LV2 — LV2 export (Linux)

- **Acceptance:** `stave::export!` emits an LV2 plugin with its TTL
  generated from the same metadata (ports, params as control ports or
  patch properties, state, latency, MIDI via atom ports, the face via
  the X11 UI extension).
- **Verification:** `lv2lint` and a host (Carla or jalv) driven in CI on
  Linux.
- **Status:** Open.

### F-AAX — AAX decision

AAX needs Avid's SDK (licence agreement), PACE signing (paid, per
developer) and Pro Tools to test.

- **Acceptance:** a written decision: build an AAX wrapper (and how to
  keep the SDK out of a public MIT repository), wrap through a bridge,
  or decline and say so on the docs site.
- **Verification:** the decision recorded here with its costs.
- **Status:** Open.

### F-AUV3 — AUv3 and iOS

- **Acceptance:** an app extension (`.appex`) inside a host app for
  macOS and iOS: `AUAudioUnit` subclass over the same runtime, the face
  in a `UIView`/`NSView` controller, touch input for faces, the iOS
  audio session.
- **Verification:** `auval` on the macOS extension; an iOS simulator
  test hosting it with `AVAudioEngine`.
- **Status:** Open (after F-AU, which it shares most of its code with).
