# Architecture

How the framework is put together: which crate owns what, how a plugin
reaches a DAW through the format wrappers, how failures are reported,
and which platforms show a face. The crate graph page draws the
dependencies from `cargo metadata`; this page says why they point the
way they do.

## The crates

An author depends on one crate, [`stave`](stave), and imports
[`stave::prelude`](stave::prelude). Everything else is either what
`stave` re-exports, what its wrappers use, or tooling around it.

**The author's surface**

- [`stave`](stave) — the [`Plugin`](stave::Plugin) trait, the prelude,
  [`stave::export!`](stave::export) (a VST3 and a CLAP plugin from one
  `cdylib`, class ids derived from `Info::id`), the derives, and
  re-exports of the model, [`stave::dsp`](stave::dsp) and
  [`stave::face`](stave::face).
- [`stave_core`] — the plugin model with no UI dependency:
  [`Info`](stave_core::Info) and [`Layout`](stave_core::Layout), typed
  params, [`Smoother`](stave_core::params::Smoother) and
  [`Steps`](stave_core::params::Steps), [`Block`](stave_core::Block)
  with its [`InputBuffer`](stave_core::InputBuffer) and
  [`OutputBuffer`](stave_core::OutputBuffer), presets, state, events, and
  the editor contract ([`Controller`](stave_core::Controller), implemented
  by [`PluginController`](stave_core::PluginController) over the param
  table, the telemetry and a wrapper's
  [`GestureSink`](stave_core::GestureSink)). Its `runtime` module, the
  plugin as the wrappers drive it, is hidden from the docs: not author
  API.
- [`stave_dsp`] — the DSP vocabulary (see [the DSP library](crate::concepts::dsp)).
  It knows nothing of hosts, params or events.
- `stave-macros` — `#[derive(Params)]`, `#[derive(Taps)]` and
  `#[derive(Smoothers)]` (proc-macros need a crate of their own).
- [`stave_telemetry`] — [`Telemetry`](stave_telemetry::Telemetry), the
  plugin → face and tool channel (see [telemetry](crate::concepts::telemetry)).

**The face**

- [`stave_face`] — the design system: parts, themes, layout and the
  face [`Runtime`](stave_face::Runtime) (see [faces](crate::concepts::faces)).
- [`stave_ui`] — the OS-agnostic layout tree with its flexbox solver,
  and the [`Scene`](stave_ui::scene::Scene) vector IR every picture is
  made of. Pure, so tested headless.
- [`stave_raster`] — a pure-Rust software rasteriser: a Scene into a
  premultiplied RGBA8 buffer, identically on every platform.
- `stave-font` — embedded IBM Plex Sans and Mono outlines, filled by
  stave-raster, for the platforms without a text system of their own.
- `stave-ui-cocoa`, `stave-ui-win32`, `stave-ui-x11` — one platform
  view each (`FaceView`): a window, a pointer, a blit and text, never
  parameter logic. `stave-ui-shell` is what the Windows and Linux views
  share. Each crate is empty off its platform (`stave-ui-x11` off unix:
  macOS compiles it, for checking).

**The formats**

- `stave-vst3` and `stave-clap` — the plugin side of each ABI, written
  by hand (no C++ SDK, no bindings crate). Authors never name them:
  `stave::export!` emits their entry symbols.
- `stave-vst3-sys` and `stave-clap-sys` — the raw ABIs and nothing
  else: `#[repr(C)]` types, constants, vtables, IIDs and the layout
  assertions that pin them to the SDK headers; `no_std`. A host reads
  VST3 through `stave-vst3-sys` without linking the plugin wrapper or
  its platform crate.

**Measurement, hosting and verification**

- [`stave_measure`] — measurement as pure functions over buffers:
  levels, spectra, THD, latency, transfer functions, dynamics,
  modulation. No file I/O.
- [`stave_host`] — host any VST3 (or any catalogued stave-dsp stage) as
  an [`Instance`](stave_host::instance::Instance), then drive, measure
  and validate it. `stave-bench` and `stave-play` are its CLI and GUI
  (see [the host tools](crate::howto::host_tools)).
- [`stave_test`] — the checks every plugin mounts and the harness that
  drives a plugin in-process (see [testing](crate::concepts::testing)).
- `stave-model` — protocol models of the plugin ↔ host contracts,
  checked by a small explicit-state model checker and used as oracles
  for real traces (see [verification](crate::concepts::verification)).

The worked example is `examples/tilt`; the tools are `tools/stave-bench`,
`tools/stave-play`, `tools/storybook` and `tools/docsite` (which builds
this site). This guide is the
`stave-guide` crate: Markdown included as module docs, so its links and
examples are checked with everything else.

## The format wrappers

A DAW calls a plugin through raw function pointers, from whatever
thread it likes, all on one instance pointer. The shared runtime in
`stave_core::runtime` turns that into a sound Rust protocol rather than
a fabricated `&mut`:

- **Shared state is atomic.** The fields both the main thread and the
  audio thread legitimately touch (sample rate, block size, whether
  the plugin is prepared, latency) are plain atomics; interface objects
  read them through `&self`.
- **The plugin is claimed, not assumed.** The audio-side state sits in
  a cell claimed by an atomic token. A compliant host never overlaps
  `process` with the calls that reconfigure it; a host that does gets a
  refused call, not a data race.
- **In-place hosts go through scratch.** Many hosts pass the same memory
  as input and output. The wrapper detects the overlap and copies
  through preallocated scratch, so a [`Block`](stave_core::Block)'s
  input and output never alias.
- **Bypass, latency and tail belong to the framework.** Bypass is a
  10 ms equal-power crossfade to a latency-matched dry path, keyed on
  the param declared with
  [`BoolParam::bypass`](stave_core::params::BoolParam::bypass).
- **Faces are embedded in the host's window.** VST3 `IPlugView` attaches
  to an `NSView` on macOS, an `HWND` on Windows and an
  `X11EmbedWindowID` on Linux; CLAP's `gui` extension offers exactly one
  window API per platform (`cocoa`, `win32`, `x11`). Both wrappers edit
  through a `PluginController` whose gesture sink is the host's
  automation path, and both publish the wrapper's taps (input and output
  peaks, CPU load) and the capture frame to the instance's telemetry.
  A plugin that takes notes gets one CLAP note port.

The wrappers' ABIs run under Miri (`tests/wrapper_abi.rs`,
`tests/table_abi.rs` in `stave-vst3` and `stave-clap`); the lock-free
protocols in `stave-core` and `stave-telemetry` are modelled with Loom
(`tests/loom.rs`).

What a host keys a plugin on (saved sessions, presets, automation) is
its identity, readable with [`stave::identity`](stave::identity):

```rust
use stave_tilt_vst3::Tilt;

let id = stave::identity::<Tilt>();
assert_eq!(id.clap, "com.ryanbahan.stave.tilt");
// The VST3 class ids are derived from the same id, and differ.
assert_ne!(id.vst3_component, id.vst3_controller);
```

## Diagnostics

[`stave_core::diagnostics`] is an instance's account of what went
wrong, for whoever debugs it inside a DAW.

- **Refused calls are logged.** Every host call a wrapper refuses is a
  [`CallError`](stave_core::diagnostics::CallError), logged as
  `"<call>: <error>"` before the wrapper returns the format's failure
  code.
- **The audio thread only counts.** What `process` could not do (a
  concurrent call, a block longer than declared, dropped events, a
  block cut to fit its scratch) is a
  [`ProcessFault`](stave_core::diagnostics::ProcessFault): one atomic
  add, reported as a log message by the next main-thread call.
- **A panic never reaches the host.** Every entry point that runs plugin
  code is guarded by `catch_unwind`. The first panic is logged with its
  location and disables the instance: `process` writes silence, every
  later call takes its refusal, and the plugin's state is never touched
  again. A panic on the audio thread is recorded into preallocated slots
  and logged from the main thread. Release builds unwind for this
  (`[profile.release]` in the workspace `Cargo.toml` leaves `panic` at
  its default).
- **Where messages go.** The instance's log file
  (`<log dir>/<plugin id>.log`), the CLAP host's `clap.log`, and the
  telemetry's message ring, which stave-host reads
  ([`Instance::messages`](stave_host::instance::Instance::messages)).
  The log directory is `$STAVE_LOG_DIR`, else the platform's per-user
  log directory; the workspace's `.cargo/config.toml` points cargo-run
  plugins at `target/logs`.

```rust
use stave_core::diagnostics::{log_dir, CallError};

std::env::set_var("STAVE_LOG_DIR", "/tmp/stave-logs");
assert_eq!(log_dir(), Some("/tmp/stave-logs".into()));

assert_eq!(CallError::UnknownParamId(7).to_string(), "no parameter has id 7");
```

## Platforms

| | macOS | Windows | Linux |
|---|---|---|---|
| VST3 and CLAP from one `cdylib` | yes | yes | yes |
| Face shown by | `stave-ui-cocoa` (an `NSView`; text by CoreText) | `stave-ui-win32` (an `HWND` child window, GDI blit) | `stave-ui-x11` (an X11 child window on the host's run loop) |
| Text | the system's | embedded fonts (`stave-font`) | embedded fonts (`stave-font`) |
| CI | `ci.yml` (tests, clippy, rustdoc, validators) | `platforms.yml` | `platforms.yml` (faces driven on Xvfb); `ci.yml`'s tree rules, Miri and Loom |

On every platform CI builds and tests the whole workspace and runs
clap-validator and Steinberg's validator on the plugin binaries; on
Windows and Linux it also opens the bundle with stave-host. The faces are drawn by the same portable
rasteriser everywhere; only text and the blit differ.
