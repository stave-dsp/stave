# Getting started

From a fresh clone to Stave Tilt, the framework's worked example, loaded
in a DAW: build it, bundle it as a VST3 and a CLAP plugin (and on macOS
an Audio Unit), validate the bundles, install them where hosts look, and open the plugin in Stave's
own host. Then a tour of the one file that is the whole plugin.

## What you need

- **Rust.** The repository pins its toolchain in `rust-toolchain.toml`
  (with `clippy` and `rustfmt`); `rustup` installs it on the first
  `cargo` command inside the checkout. The workspace's minimum supported
  Rust version is 1.86.
- **A desktop OS.** The bundle scripts know macOS, Linux and Windows
  (under Git Bash). The paths below are the macOS ones; the Linux and
  Windows equivalents follow each step.
- **Optionally, the formats' own validators.** `scripts/install.sh` runs
  Steinberg's VST3 validator when it finds one (`$VALIDATOR`, or a
  VST3 SDK build under `target/vst3sdk`) and `clap-validator` when it is
  on your `PATH`, and says so when it skips either; on macOS it always
  runs Apple's `auval` on the Audio Unit.

## Build, bundle, validate, install

```sh
git clone https://github.com/stave-dsp/stave
cd stave
bash scripts/install.sh stave-tilt-vst3 release
```

`scripts/install.sh <crate> [release|debug]` is the one command that
does every step, in an order chosen so a stale build can never be
installed:

1. **Build.** `cargo build --release -p stave-tilt-vst3`. Tilt's crate
   is a `cdylib` (and an `rlib`, so tests can link it).
2. **Bundle.** The crate name, minus a trailing `-vst3`, in CamelCase
   names the bundle: `build/StaveTilt.vst3` and `build/StaveTilt.clap`.
   On macOS a VST3 bundle is `Contents/{Info.plist, PkgInfo,
   MacOS/StaveTilt}`, ad-hoc code-signed, and the CLAP bundle has the
   same shape; on Linux the VST3 holds `Contents/<arch>-linux/StaveTilt.so`
   and the `.clap` is the shared library itself; on Windows the VST3
   holds `Contents/<arch>-win/StaveTilt.vst3` and the `.clap` is the DLL.
   On macOS there is also `build/StaveTilt.component`, the Audio Unit
   (see [Plugin formats](crate::howto::formats)).
   The bundle identifier is `com.ryanbahan.stave.tilt.vst3` (and
   `.clap`, `.component`); set `STAVE_BUNDLE_VENDOR` to use your own prefix and
   `STAVE_BUNDLE_VERSION` for the bundle version (default `0.0.1`).
3. **Validate.** Steinberg's validator must report its result and
   `clap-validator` must report `0 failed`, or the script stops before
   installing anything.
4. **Install, with fresh inodes.** Each bundle is removed from the
   per-user plug-in folder and copied anew, never copied over: a host
   that has the old bundle mapped would otherwise see its file change
   underneath it. The folders are:

   | OS | VST3 | CLAP | Audio Unit |
   |---|---|---|---|
   | macOS | `~/Library/Audio/Plug-Ins/VST3` | `~/Library/Audio/Plug-Ins/CLAP` | `~/Library/Audio/Plug-Ins/Components` |
   | Linux | `~/.vst3` | `~/.clap` | — |
   | Windows | `%LOCALAPPDATA%\Programs\Common\VST3` | `%LOCALAPPDATA%\Programs\Common\CLAP` | — |

The steps also exist on their own. `scripts/bundle-vst3.sh`,
`scripts/bundle-clap.sh` and `scripts/bundle-au.sh` (`<crate>
[release|debug] [out-dir]`) bundle
an already-built library without building it; `scripts/bundle-all.sh
[release|debug]` builds, bundles and installs every plugin under
`examples/`; and `scripts/validate-formats.sh [vst3|clap|au|all]
[out-dir]` builds every plugin in release, bundles it into
`target/validate` and runs the validators, the way CI does.

## Open it

Stave's windowed host loads the bundle with the plugin's own face bound
to the live instance, a playable sine bank and keyboard, and live
measurements:

```sh
cargo run --release -p stave-play -- build/StaveTilt.vst3
```

`stave-play --help` lists its options (`--sample-rate`, `--block-size`,
`--set NAME=VALUE`, and a headless `--screenshot PATH`). In a DAW, rescan
plug-ins: Tilt appears as **Stave Tilt** by **Ryan Bahan**, an EQ, in
both its VST3 and its CLAP form.

## A tour of Tilt

Tilt is one file, `examples/tilt/src/lib.rs`, written from one import,
`use stave::prelude::*`. It is a tilt EQ: a low shelf at −tilt dB and a
high shelf at +tilt dB, both at 650 Hz, so one knob tips the spectrum
around that pivot. Read it top to bottom; it is the shape every Stave
plugin copies.

**Identity.** `Tilt::INFO` names the plugin to hosts. Its `id` is
forever: hosts key saved sessions on it, and the VST3 class ids derive
from it. [`stave::identity`] reads what [`stave::export!`] gave the
plugin, which is what a test pins so a change that would orphan every
saved session fails the build:

```
use stave::prelude::*;
use stave_tilt_vst3::Tilt;

assert_eq!(Tilt::INFO.id, "com.ryanbahan.stave.tilt");
assert_eq!(Tilt::INFO.name, "Stave Tilt");
assert_eq!(Tilt::INFO.vendor, "Ryan Bahan");
assert_eq!(Tilt::INFO.features, &[Feature::AudioEffect, Feature::Eq, Feature::Stereo]);
assert_eq!(Tilt::LAYOUTS, &[Layout::STEREO]); // the default

let id = stave::identity::<Tilt>();
assert_eq!(id.clap, Tilt::INFO.id);
assert_ne!(id.vst3_component, id.vst3_controller);
```

**Params.** `TiltParams` is a struct of typed parameters under
`#[derive(Params)]`, built in its `Default`: each has a forever
[`ParamId`](stave::ParamId), a name, a range, a unit and a declared
glide. Bypass is a [`BoolParam::bypass`](stave::params::BoolParam::bypass)
that Tilt never reads: the framework runs it.

```
use stave::prelude::*;
use stave_tilt_vst3::TiltParams;

let p = TiltParams::default();
assert_eq!((p.tilt.range().min(), p.tilt.range().max()), (-6.0, 6.0));
assert_eq!(p.tilt.value(), 0.0);           // nearest 0: flat
assert_eq!(p.tilt.smoothing(), Smooth::ms(30.0));
assert_eq!(p.output.value(), 0.0);
assert_eq!(p.tilt.format(3.0).as_str(), "3.0 dB");
assert!(Params::bypass(&p).is_some());
```

**The plugin value.** `Tilt` holds what processing needs: the params,
one [`Smoother`](stave::params::Smoother) per knob, a pair of
[`Biquad`](stave::dsp::Biquad) shelves per lane, and the tilt the
shelves were last voiced for. `new` builds it; `prepare` hands every
smoother and filter the sample rate; `process` takes one ramp per knob
for the block, then runs `block.lanes::<2>()`, re-voicing the shelves
only when the glide moves and skipping them entirely at a tilt of
exactly 0, so a flat tilt leaves the signal to the output gain alone.

**The face.** `Tilt::face` is a pure function of the params: a title and
a bypass footswitch over a plate holding the tilt and output knobs, on
the graphite [`Theme`](stave::face::Theme).

**Export.** The last line, `stave::export!(Tilt);`, makes the crate a
VST3 and a CLAP plugin at once.

Tilt's tests are the other half of the example. `tests/properties.rs`
states what only Tilt can promise (positive tilt brightens, the pivot
holds still, zero tilt is bit-flat); `tests/editor.rs` drives its face
as a person would; `tests/checks.rs` mounts every generic check in one
line. Run them with:

```sh
cargo test -p stave-tilt-vst3
```

## Where next

- [Your first plugin](crate::tutorials::first_plugin) builds a plugin
  from an empty file, one tested step at a time.
- [The plugin lifecycle](crate::concepts::lifecycle) and
  [Parameters and smoothing](crate::concepts::params) explain what the
  framework calls, when, and what it owns.
- [Prove it](crate::tutorials::prove_it) is how a plugin earns trust:
  its own properties, measured.
