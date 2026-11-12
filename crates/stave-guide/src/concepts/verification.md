# Verification

Every promise Stave makes is held by something that fails the build: a
type, a test, a gate script, an industry validator, a model checker. A
rule lives on the strongest rung that can hold it, and moves up when a
stronger one opens; a convention with nothing behind it is a deferred
failure. This page is the map: what holds each promise, and where it
runs.

## The ladder

1. **Types.** The failure cannot be written. A
   [`Loop`](stave_dsp::Loop) demands a non-zero delay; a
   [`PerChannel`](stave_dsp::PerChannel) lane cannot reach another lane;
   [`value()`](stave::params::FloatParam::value) is typed and bounded by
   its declaration; a tap name too long for its slot does not compile;
   private fields make reaching into another component a compile error.
2. **Property tests.** A promise the design makes, with its expected
   value derived from math, physics or the declaration — never recorded
   from a run (see [testing](crate::concepts::testing)). The DSP
   vocabulary's laws live in `crates/stave-dsp/tests/` and the
   conformance suites in `crates/stave-test/tests/`; each plugin states
   its own in its `tests/`.
3. **Gates.** Scripts for what Rust cannot express: vocabulary, layering,
   file organisation, purity.
4. **Validators, models and tools.** The industry's own suites, Miri,
   Loom, Kani, sanitizers, fuzzers, protocol models and a simulated host.

## The gates

`scripts/gates.sh` is the commit gate: the tree rules, then every test
under cargo-nextest, then the doctests. `scripts/gates.sh affected` runs
only the packages the uncommitted change can reach.

| script | holds |
|---|---|
| `check-vocabulary.sh` | the [glossary](crate::glossary): retired names nowhere in the tree; the words it reserves kept off the author surface ("render" means pixels; the lifecycle is prepare, reset, process; `sample_rate`, never an abbreviation) |
| `check-view-bindings.sh` | plugins publish through declared taps, never `probe!`; `stave-dsp`'s layers only use the layers below them |
| `check-plugin-rules.sh` | the tree rules for plugins: no milestone tags in code comments, no cast-and-clamp beside a param read, one spine per plugin |
| `check-purity.sh` | the pure crates (`stave-dsp`, `stave-ui`, `stave-raster`, `stave-face`, `stave-measure`, `stave-model`, `stave-font`) read no clock, file, environment or randomness, start no threads and keep no hidden mutable state — each exception listed with its reason |
| `check-golden-sizes.sh` | every committed golden picture together under 3 MiB |
| `check-file-sizes.sh` | a *signal*, never a failure: files past 1 000 lines (1 600 for tests) are printed, and the split is a human decision |

`scripts/hygiene.sh` adds licences, duplicate versions and sources
(cargo-deny, policy in `deny.toml`), unused dependencies (cargo-machete)
and every rustdoc link; `scripts/hygiene.sh advisories` checks the RustSec
database.

## Validators

`scripts/validate-formats.sh` builds every plugin in release, bundles it,
and runs the formats' own suites over the bundles: Steinberg's VST3
validator and clap-validator. Zero failures is the release threshold.
`scripts/check-exports.sh` loads each binary the way a host does and
resolves the entry points its platform's hosts look up. In-process,
[`plugin_checks!`](macro@stave_test::plugin_checks) runs stave-host's own
validation suite and the host-behaviour checks on every plugin.

## Unsafe code and concurrency

- **Miri** interprets the unsafe core under Rust's memory model:
  `stave-core`, `stave-telemetry`, and the full wrapper path of both
  formats — factory, vtables, in-place processing, two instances,
  release — through their ABI test binaries.
- **Loom** model-checks the lock-free protocols of `stave-core` and
  `stave-telemetry` (their `tests/loom.rs`), on the shipping types rather
  than copies.
- **Kani** (`scripts/kani.sh`) proves bounded properties over every input:
  parameter normalisation and lookup, the block splitter and lanes, the
  gesture ring, telemetry name truncation, a delay line's length — each
  harness a `#[kani::proof]` in a `#[cfg(kani)] mod proofs`.
- **Sanitizers** (`scripts/sanitize.sh address|thread`) reach what Miri
  cannot: compiled code, real threads and the real allocator, over the
  wrappers' ABI tests, stave-host hosting a real plugin, and the face
  runtime.
- **Layout assertions**: every VST3 and CLAP struct Stave mirrors is
  pinned by `const` size and alignment assertions in `stave-vst3-sys` and
  `stave-clap-sys`; `scripts/abi-layouts.sh` re-measures them from the
  official headers.

## Models and the simulated host

`stave-model` writes the plugin–host contracts as small protocol models —
the lifecycle, the gesture ring, state restore against the smoother,
telemetry slot reservation, an editor's lifetime against its instance —
checked exhaustively by an explicit-state model checker. The same models
are oracles:

- `stave-host`'s **simulated host** drives a real plugin through seeded
  random VST3 call sequences, legal and illegal — any block size, rate
  changes, in-place and null buffers, hostile parameter changes, garbage
  and truncated state — with process calls on an audio thread, and after
  every call checks the plugin's answers against the lifecycle model.
  Any failure names its seed, and that seed replays it exactly. The main
  run sweeps 48 seeds; `STAVE_SIM_SEEDS` widens it.
- **Trace validation** checks the calls of real wrapper runs against a
  model, and the [event trace](crate::concepts::telemetry) of a real
  plugin against the protocol's laws.

## Fuzzing

`fuzz/` holds cargo-fuzz targets for state loading (seeded from the
committed state fixtures), parameter text parsing, the rasteriser and
hosting a real plugin through stave-host. `scripts/fuzz.sh [seconds]
[target]` runs them; a crash leaves its input in
`fuzz/artifacts/<target>/` to replay.

## API stability

`scripts/semver-checks.sh` compares the public crates' API with a released
tag, so a change the version number does not allow fails;
`scripts/release.sh` runs it before tagging, and `deep.yml` runs it on
every push to a release branch, where a patch must not break the API it
patches.

## Where it all runs

| workflow | when | runs |
|---|---|---|
| `ci.yml` | every push to main and release branches, every pull request | macOS: every test under nextest and the doctests; clippy, rustdoc and the MSRV check; both validators. Linux: the tree rules and hygiene; the portable crates' check and the Loom models; Miri in four shards |
| `platforms.yml` | every push to main and release branches, every pull request | Linux and Windows: build and test the workspace (the X11 face in a real window on Xvfb), clippy and rustdoc, exported entry points, both validators, and stave-host opening the bundle |
| `deep.yml` | nightly and on demand; semver checks on every push to a release branch | one Kani job per harness, both sanitizers, every fuzz target for four minutes, the simulated host over 3 000 seeds, RustSec advisories, and semver checks |
| `docs.yml` | every push to main, every pull request; deployed from main | macOS: `scripts/docsite.sh` builds the documentation site — rustdoc with every warning an error; a guide module without an article, a face part without a picture, a binary whose `--help` fails or a link that goes nowhere fails it — and main deploys it to GitHub Pages |
| `canary.yml` | weekly | the newest stable toolchain's build and clippy; a failure opens an issue instead of turning main red |

Every job aims to finish in five minutes warm, so the main signal gates
every push. The overhaul ledger, `OVERHAUL.md`, records what each check
was added for.
