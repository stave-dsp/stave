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

## Models against references

A circuit model is held to ngspice on the same netlist; a model SPICE
cannot express (a magnetic core) is held to a high-precision solution of
its equations. Both go through one harness,
[`stave_test::reference`](stave_test::reference): a
[`Case`](stave_test::reference::Case) names its reference, a
[`Stimulus`](stave_test::reference::Stimulus), a
[`Metric`](stave_test::reference::Metric) and an explicit budget, and the
model runs inside it — [`stage`](stave_test::reference::Case::stage) ticks
a stage at a multiple of the reference's rate,
[`pointwise`](stave_test::reference::Case::pointwise) evaluates a static
transfer or a frequency response at each reference point. The cases live
in `crates/stave-test/tests/matches_ngspice.rs` and `matches_reference.rs`,
one test per model, named `<model>_matches_ngspice` or
`<model>_matches_reference`; a model's rustdoc names that test.

The metrics are sample errors (max or RMS absolute, normalized RMS, max
relative to the peak), `MaxDb` for magnitude responses, and the spectral
`Harmonics` and `ThdDelta`. Budgets are set a small factor above the
measured error, so a model that drifts fails long before it sounds
different; a budget is never loosened to make a change pass.

### Adding a circuit

1. Write `crates/stave-test/spice/<name>.cir`: the circuit, a `.tran`
   at the 48 kHz step (`20.8333333u`), `.ac` or `.dc`, and a `.control`
   block that ends `wrdata OUTFILE v(<node>)` (copy one beside it; the
   options line pins ngspice's tolerances).
2. Simulate it: `scripts/ngspice-simulate.py` (needs `brew install
   libngspice` on macOS). It checks ngspice on the circuits with exact
   answers first, then writes `<name>.ngspice.txt` beside every netlist.
3. Add the case and commit the netlist and its waveform together:

```rust
use stave_dsp::{Coupler, Stage};
use stave_test::reference::{Case, Drive, Metric, Stimulus};

let mut coupler = Coupler::new((1.0 / (std::f64::consts::TAU * 22e-9 * 1e6)) as f32);
coupler.prepare(48_000.0);
Case::ngspice("coupler")
    .stimulus(Stimulus::sine(1.0, 50.0)) // the netlist's source
    .budget(Metric::NormalizedRms, 2e-6)
    .stage(Drive::oversampled(1), |x| f64::from(coupler.tick(x as f32)))
    .assert();
```

Run it with `cargo test -p stave-test --test matches_ngspice -- --nocapture`
to see the measured error, then set the budget a small factor above it.
`about_operating_point` compares swings around the bias point,
`up_to_scale` allows one fitted level calibration, and
`assert_converges` requires the error to fall as the step halves, so a
model converges to the reference rather than to a model of its own. A
reference that is not a netlist is `Case::generated("<name>",
"scripts/<generator>.py")` over `crates/stave-test/reference/<name>.txt`.

### Regenerating the references

```text
scripts/ngspice-simulate.py            # every spice/*.ngspice.txt (brew install libngspice, or apt install ngspice)
scripts/ngspice-simulate.py --check    # compare instead of write
pip install -r scripts/reference-requirements.txt
scripts/ja-reference.py [--check]      # reference/*.txt (pinned NumPy, SciPy)
```

The `references` workflow installs a pinned ngspice (Ubuntu 24.04's
package of ngspice 42) and the pinned NumPy and SciPy on Linux, and runs
both scripts with `--check` whenever a netlist, a reference, a script,
the circuit code or the harness changes. A regenerated file that differs
from the committed one by more than print, platform and version noise
fails: 2×10⁻⁵ of its peak for ngspice (the committed waveforms come from
ngspice 47 on macOS, which 42 reproduces within 5×10⁻⁶), 10⁻⁸ for the
solved references (macOS and Linux agree within 2×10⁻¹⁰). A 1% component
change moves a waveform by 1.6×10⁻³ of its peak or more, and the workflow
proves it every run: it perturbs a capacitor by 1%, and a solver
parameter by 1%, and requires each check to fail. So a netlist edited
without regenerating its waveform cannot pass. A changed fixture is
committed with the change that caused it, and the model cases must still
pass at their budgets.

### Reading a failure

```text
reference case `coupler` FAILED: normalized RMS error 5.514e-7 > budget 1e-7
  worst     at t = 8.291666653e-3 s (point 398 of 4801): reference 2.817963878e-1, model 2.817972600e-1, error 8.723e-7
  reference crates/stave-test/spice/coupler.ngspice.txt (ngspice; regenerate with scripts/ngspice-simulate.py)
  stimulus  sine, 1 peak at 50 Hz
  model     1× the reference's 48000 Hz
  dump      target/reference-diff/coupler.tsv (abscissa reference model error)
```

The first line is the metric against the budget; `worst` is where the
error peaks, with both values there — an error at the first points is a
start-up or alignment mismatch, at the peaks a nonlinearity, everywhere a
level or a wrong component. The dump holds the whole comparison as
columns for any plotter (`numpy.loadtxt`, gnuplot); CI uploads it as the
`reference-diff` artifact. `STAVE_REFERENCE_DUMP=1` writes a dump for
every case, passing or not.

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
| `references.yml` | pushes and pull requests that touch a netlist, a reference, their scripts, the circuit code or the harness; not a required check, since it is path-filtered | Linux: the pinned ngspice and NumPy/SciPy regenerate every reference, which must match the committed one (see [models against references](#models-against-references)) |
| `docs.yml` | every push to main, every pull request; deployed from main | macOS: `scripts/docsite.sh` builds the documentation site — rustdoc with every warning an error; a guide module without an article, a face part without a picture, a binary whose `--help` fails or a link that goes nowhere fails it — and main deploys it to GitHub Pages |
| `canary.yml` | weekly | the newest stable toolchain's build and clippy; a failure opens an issue instead of turning main red |

Every job aims to finish in five minutes warm, so the main signal gates
every push. The overhaul ledger, `OVERHAUL.md`, records what each check
was added for.
