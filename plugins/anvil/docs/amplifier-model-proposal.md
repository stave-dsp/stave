# Anvil amplifier model proposal

Status: approved; v1 implemented in `plugins/anvil/src/engine/`.

This document defines Anvil's amplifier-domain structure. It assumes only that
the framework can deliver controls honestly; it does not depend on a
particular parameter API.

## The problem with `Wall`

`Wall` is a useful product metaphor and a poor implementation boundary.

The current type owns all of these:

- difference-tone subharmonic generation and injection;
- a full-band amplifier branch;
- a three-way crossover;
- three independently saturated band branches;
- branch input/output isolation filters;
- per-branch preamp cascades;
- per-branch push-pull pairs;
- per-branch and shared power supplies;
- phase compensation;
- full-band/split-band topology blending;
- dynamic high-frequency emphasis;
- a post-sum high-frequency clipper;
- level prediction and observability.

Its `set()` method accepts panel concepts and computes circuit staging while
also triggering structural curve rebuilds. A future developer cannot tell
which invariant belongs to which mechanism.

`Road` is similarly vague. It is actually a complete amplifier branch:
preamp, output pair, supply interaction, branch-specific filtering, staging,
and level reporting.

`power.rs` is not a power amplifier. It is a collection of pre- and post-drive
biquads.

## Domain statement

Anvil is a dual-topology amplifier:

1. one full-band amplifier branch creates shared-band intermodulation and a
   dense baseline;
2. one three-band amplifier bank drives low, low-mid, and high bands through
   independent amplifier branches;
3. a topology blend moves between those outputs after phase compensation;
4. tone and output conditioning follow the nonlinear topology.

That is the domain model. "Wall", "girth", "modern", and "teeth" can remain
product language and test descriptions, but implementation types and fields
name topology, mechanism, units, or measured responsibility.

## Proposed signal structure

```text
input
  -> InputConditioner
       high-pass / input reference
  -> DifferenceToneGenerator
       optional, injected before amplification
  -> ParallelAmplifier
       full-band AmplifierBranch
       SplitBandAmplifier
           ThreeWayCrossover
           low AmplifierBranch
           low-mid AmplifierBranch
           high AmplifierBranch
           isolation filters and phase compensation
       TopologyBlend
  -> HighBandLimiter
  -> ToneSection
  -> OutputConditioner
       fixed response shaping, HF rolloff, DC block, slew
  -> level trim / computed makeup
```

The order of `ToneSection` is a product decision still requiring an audible
prototype. The proposed new interactive TMB stack belongs after the nonlinear
parallel amplifier so it remains effective at high drive. It must be one
isolated audible change, not folded into the file restructure.

## Types and responsibilities

### Per-channel engine state

Owns the per-channel signal path and nothing host-specific:

```rust
struct ProcessingLane {
    input: InputConditioner,
    difference_tone_generator: DifferenceToneGenerator,
    parallel_amplifier: ParallelAmplifier,
    high_band_limiter: HighBandLimiter,
    tone_section: ToneSection,
    output: OutputConditioner,
}
```

It accepts resolved settings in domain units. It does not read
`AnvilParameters`, normalize knobs, or know parameter identifiers.

### `ParallelAmplifier`

Owns the two nonlinear topologies and their output blend:

```rust
struct ParallelAmplifier {
    full_band_branch: AmplifierBranch,
    split_band_amplifier: SplitBandAmplifier,
    full_band_phase_alignment: Biquad,
    topology_separation_amount: f32,
}
```

It replaces `Wall`.

Responsibilities:

- apply settings to the correct branches;
- process both topologies when needed;
- return their phase-aligned blend;
- report output level from the topology actually in use;
- expose topology-level measurements.

It does not generate subharmonics, own final tone controls, or soft-clip the
summed high band.

### `SplitBandAmplifier`

Owns the three-way split and recombination:

```rust
struct SplitBandAmplifier {
    crossover: ThreeWayCrossover,
    low_band_branch: AmplifierBranch,
    low_mid_band_branch: AmplifierBranch,
    high_band_branch: AmplifierBranch,
    low_band_isolation_filter: DeepBranchIsolationFilter,
    low_mid_band_isolation_filter: DeepBranchIsolationFilter,
}
```

Its contract is measurable:

- crossover reconstruction in the linear/bypass case;
- isolation/sideband floor for the high branch;
- bounded recombination gain;
- phase agreement at both crossover regions.

`FOCUS` does not enter this type as a product word. Resolved settings use
`topology_separation_amount` and the split-band topology derives its bounded
`isolation_amount`.

### `AmplifierBranch`

Replaces `Road`. It is a complete nonlinear amplifier branch:

```rust
struct AmplifierBranch {
    input_response: BranchInputResponse,
    preamp: RcCoupledTriodeCascade,
    power: PowerStage,
    output_response: BranchOutputResponse,
}

struct PowerStage {
    pair: PushPull,
    supply: PowerSupply,
}
```

The branch owns the feedback loop by which pair draw changes the supply and
the supply changes both pair and preamp operating points. Keeping that loop
inside the branch is more important than forcing every electrical noun into a
separate object.

Branch configuration separates design from live controls:

```rust
struct BranchDesign {
    stage_count: usize,
    interstage_gain_db: f32,
    blocking_depth: f32,
    blocking_recovery_ms: f32,
    pair_bias_cold_v: f64,
    pair_imbalance: f64,
    supply: SupplyDesign,
    input_response: FilterDesign,
    output_response: FilterDesign,
}

struct BranchSettings {
    preamp_drive_db: f32,
    pair_overdrive_db: f32,
    dynamic_high_shelf_db: f32,
}
```

`BranchDesign` is applied during preparation/reactivation. `BranchSettings`
has bounded realtime cost.

Do not use booleans such as `bright` to select an undocumented recipe.
Construct named branch designs:

```rust
fn full_band_design() -> BranchDesign;
fn low_band_design() -> BranchDesign;
fn low_mid_band_design() -> BranchDesign;
fn high_band_design() -> BranchDesign;
```

Each function is an explicit table of physical quantities. Differences are
reviewable without following conditional code through `prepare()`.

### `TopologyBlend`

Owns phase compensation and the full-band/split-band crossfade.

Its settings are:

```rust
struct TopologyBlendSettings {
    topology_separation_amount: f32,
}
```

If the current focus-gated isolation behavior remains, the panel mapping may
derive `isolation_amount`, but that value belongs to
`SplitBandAmplifier`; it should not be hidden inside a blend setter.

### `DifferenceToneGenerator`

Owns the current low-pass, squaring-law nonlinearity, output isolation, and
injection gain.

Its name describes what it does. Tests can assert:

- no linear leakage when the amount is zero or input is quiet;
- generated components occur at input difference frequencies;
- output energy above the declared isolation band is bounded.

### `BlockingControlledHighShelf`

The current `EDGE` behavior reads preamp blocking voltage and periodically
moves a high shelf. That is not generic "edge".

Represent the mechanism explicitly, either as a small object inside the
affected branches or as named branch logic:

```rust
struct BlockingControlledHighShelf {
    base_gain_db: f32,
    depth_db: f32,
    update_interval_samples: u32,
    filter: Biquad,
}
```

The panel may still say `EDGE`; the resolved setting is
`blocking_high_shelf_depth_db`.

### `HighBandLimiter`

Owns the complementary 2.5 kHz split and soft clipping of the high band after
topology recombination. It is neither part of the amplifier branches nor an
output transformer.

Its contract is transient/harmonic limiting above its crossover with bounded
effect below it.

### `AnvilToneStack`

The TMB implementation is one interactive network, not three
independent filters. Its settings are normalized control positions or
electrical values, depending on the chosen DSP primitive.

Its Yeh/Smith-derived circuit equation is not its product identity. Anvil
owns a modern high-gain component design: 470 pF treble, 22 nF bass and
midrange, a 39 kΩ slope resistor, and 250 kΩ / 1 MΩ / 33 kΩ treble, bass,
and midrange potentiometers. Bass, Mid, and Treble expose honest 0–10
physical control positions rather than fictional independent decibel gains.

It owns:

- coefficient/state preparation;
- interaction between bass, mid, and treble;
- a reported small-signal response for tests and visualization.

Presence and depth do not belong here if they later become part of a true
power-stage feedback loop. Until that loop exists, they are output response
controls and should be named honestly in code.

### `OutputConditioner`

Replaces the current miscellaneous post chain. Its fields use response names:

- fixed low-mid peak;
- fixed high shelf;
- user high shelf;
- user low shelf;
- first and second HF low-pass;
- DC high-pass;
- slew limiter.

Do not name fixed filters `girth`, `modern`, or `ceiling` internally. Those
names encode an opinion, not a transfer function.

If a future output-transformer model exists, it becomes its own component with
actual transformer state and laws. A pair of low-pass filters should not claim
that domain.

## Controls versus amplifier quantities

The panel remains intentionally simple and may use evocative labels. A pure
mapping converts it to explicit amplifier settings:

```rust
struct PanelControlValues {
    preamp_gain_db: f32,
    input_low_cut_hz: f32,
    power_pair_overdrive_db: f32,
    topology_separation_amount: f32,
    bass_tone_position: f32,
    midrange_tone_position: f32,
    treble_tone_position: f32,
    presence_tone_db: f32,
    depth_tone_db: f32,
    difference_tone_amount: f32,
    high_frequency_emphasis_amount: f32,
}

struct AmplifierSettings {
    preamp_stage_count: usize,
    preamp_drive_db: f32,
    power_pair_overdrive_db: f32,
    topology_separation_amount: f32,
    reference_input_level_db: f32,
    blocking_high_shelf_depth: f32,
}

struct EngineSettings {
    input: InputSettings,
    difference_tone_injection_amount: f32,
    amplifier: AmplifierSettings,
    tone: ToneSettings,
    output_conditioner: OutputSettings,
}
```

`resolve_engine_settings(panel_controls, design) -> EngineSettings` is pure
and exhaustively unit tested. This is the only place panel macros are allowed
to couple multiple amplifier quantities.

## `WALL` split

The current ID-12 control changes both:

- `pair_overdrive_db`;
- `pair_bias_cold_v`, which rebuilds push-pull curves and resets history.

The player control should become `SQUASH` or another chosen label whose domain
result is only `pair_overdrive_db`.

Pair bias becomes part of each `BranchDesign`, applied on
prepare/reactivation. To preserve the current default pair class, the first
design uses:

```text
0.8 + 12.0 * 0.12 = 2.24 volts cold
```

The old sweep cannot remain identical at every position because removing the
bias coupling intentionally changes its semantics. Stable host ID, default,
and preset values can remain.

## Level model

Computed makeup must not ask one arbitrarily selected branch to predict the
whole topology.

Each `AmplifierBranch` reports a `LevelReport` derived from the same preamp and
pair models it processes:

```rust
struct LevelReport {
    small_signal_gain_db: f32,
    ceiling_db: f32,
    output_db_at_reference: f32,
}
```

`SplitBandAmplifier` combines the reports using its branch trims and an
explicit reference spectrum. `ParallelAmplifier` combines full-band and
split-band reports using `topology_separation_amount`. If that combined prediction proves
unreliable, makeup should be a declared fixed staging choice rather than a
misleading calculation.

The rule is: a report may simplify, but its scope and reference signal must be
named.

## Observability

Telemetry follows component boundaries:

- full-band branch input/preamp/pair/output;
- low, low-mid, and high branch output;
- per-branch pair draw and supply voltage;
- split-band recombination;
- full-band/split-band blend;
- tone-stack and output-conditioner output.

Use typed snapshots returned at block boundaries. Do not let telemetry fields
accumulate opportunistically across unrelated components.

## Source structure

Start with the splits that already have substantial independent behavior:

```text
plugins/anvil/src/
  params.rs
  controls.rs
  editor.rs
  presets.rs
  engine.rs
  engine/
    parallel_amplifier.rs             topology alignment and blend
    parallel_amplifier/
      branch.rs                       complete nonlinear branch
      split_band_amplifier.rs         crossover, isolation, recombination
    difference_tone.rs
    high_band_limiter.rs
    tone_section.rs                   Anvil mapping and level compensation
    tone_section/
      anvil_tone_stack.rs              concrete passive circuit model
    output_conditioner.rs
```

The concrete `AnvilToneStack` belongs to Anvil alongside its product
mapping, placement, and level compensation. Its constituent mathematics and
filter mechanisms may be promoted independently when another product proves
the reuse.

Do not create one file per electrical noun. `PowerStage` can remain in
`branch.rs` while it is only a pair/supply loop. Input conditioning can remain
in `engine.rs` while it is one high-pass. File boundaries follow
independently testable behavior and sustained size, not the signal-flow
diagram.

## Tests

### Pure mapping

- panel defaults resolve to the current default staging;
- each panel control changes only its declared amplifier quantities;
- `SQUASH` never changes pair bias;
- `TIGHT` coupling is explicit and monotonic;
- all resolved physical quantities remain in consumer ranges.

### Branch

- setters are idempotent;
- live settings do not rebuild structural tables;
- pair bias/configuration changes are activation-only;
- level report matches static transfer within a declared tolerance;
- supply droop and recovery follow declared time ranges.

### Multiband

- linear reconstruction;
- crossover phase agreement;
- isolation floor;
- no low-to-high nonlinear sidebands above the law's threshold;
- recombination level bounds.

### Components

- difference-tone generation and isolation;
- high-band limiter selectivity;
- interactive tone-stack direction, range, and cross-control interaction;
- output-conditioner response snapshot.

### Whole amplifier

Retain the existing sound laws, but make each failure point to a component
contract. Add live-control continuity and bounded process-time laws after the
framework parameter timeline is available.

## Migration

1. Introduce pure settings/design types around the current implementation.
   Render must remain bit-identical.
2. Extract `AmplifierBranch`; replace booleans and scattered constants with
   four explicit `BranchDesign` constructors. Render remains bit-identical.
3. Extract `SplitBandAmplifier`, `DifferenceToneGenerator`, and
   `HighBandLimiter`; keep topology alignment and blending in the
   `ParallelAmplifier` facade. Render remains bit-identical.
4. Rename post filters to objective response names. Render remains
   bit-identical.
5. Split `WALL` into player overdrive and activation-time pair bias. This is
   the first intentional semantic/audio change.
6. Prototype the interactive post-amplifier tone stack as a separate audible
   change and listening build.

### V1 implementation record

- `controls.rs` owns `PanelControlValues`, `AnvilDesign`, the pure resolver, named
  section settings, and state-semantic application gates.
- `Wall`/`Road` are now `ParallelAmplifier`/`AmplifierBranch`.
- Four named `BranchDesign` constructors replace the old `bright` boolean and
  prepare-order mutations. Pair imbalance, blocking depth/recovery, and
  high-frequency emphasis are explicit activation-time quantities.
- SQUASH retains stable parameter ID 12 and no longer changes pair bias.
  The former default bias (2.24) is an activation-time `AnvilDesign` choice.
- difference-tone generation and post-topology high-band limiting are
  independently named mechanisms owned by the per-channel engine pipeline,
  outside `ParallelAmplifier`.
- `ParallelAmplifier` is the small two-topology facade; `SplitBandAmplifier`
  owns the three frequency-band branches, crossovers, isolation filters, and
  recombination.
- the old misleading `power.rs` is now `output_conditioner.rs`.
- Anvil's `AnvilToneStack` implements the Yeh/Smith third-order passive
  network with Anvil's modern high-gain component design; `ToneSection`
  places it after the parallel amplifier and applies an explicit midband
  level recovery.
- the audio path uses concrete fields and static dispatch. Component-level
  timing and the whole-plugin performance report provide tuning seams.

## Ratified decisions

1. Keep player-facing `FOCUS`; implementation names topology blend.
2. Rename stable ID 12 to `SQUASH`.
3. Preserve the existing sound through structural steps, then isolate the
   intentional SQUASH and tone-stack changes behind laws.
4. Keep `AnvilDesign` compiled/default-only until a real bench editor or model
   selector creates a persistence requirement.
