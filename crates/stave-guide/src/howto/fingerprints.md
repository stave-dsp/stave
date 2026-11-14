# Fingerprints and perceptual metrics

A budget says a model is close enough to its reference; it does not say
the model still sounds the way it did yesterday. A refactor can move a
harmonic by half a dB, well inside every budget, and the sound that was
chosen by ear has drifted. A **fingerprint** catches that. It is the
model's measured character, committed as data. CI measures the model
again on every commit and fails when anything moves beyond numerical
noise, until someone re-blesses it on purpose. The **perceptual metrics**
are the numbers that say whether a difference, or a model's own
distortion, is something a listener would hear. This page covers both,
the budgets they state as audibility (the null test), and the aliasing
budget that the metrics make audible.

## What a fingerprint holds

Every circuit with an ngspice deck, every hand topology and every
catalogued stage has one, under `crates/stave-test/fingerprints/`
(`<circuit>.fingerprint`, `<circuit>/hand.fingerprint`,
`stages/<name>.fingerprint`). It is the model's **T0 characterisation**
(DSP.md, "Measurement design"):

- the operating point and the small-signal response;
- an odd-odd multisine at −40, 0 and +12 dB re the circuit's knee (the
  best linear approximation and the distortion on its detection bins);
- a 1 kHz tone at −20, 0 and +12 dB re the knee: gain (compression),
  each harmonic H2–H15 in magnitude and phase, THD+N, and the inharmonic
  floor and its ANMR (aliasing).

These are kept per rate: oversampled (the solver's accuracy) and at
48 kHz with no oversampling (what a plugin that ships it that way would
have). They are stored in the same table format the ngspice references
use (`stave_test::characterise::Table`). The file adds the R19 metrics,
computed from those tables, and the points the docs site's model
catalogue plots (each fingerprint's response, harmonics against level,
compression and aliasing, drawn against its reference at build time):

```text
# stave fingerprint v1: diode_clipper
model Circuit<Dk> on spice/diode_clipper.cir
tier t0
reference ngspice 42, spice/diode_clipper.cir through spice/diode_clipper.plan (…)
playback 92 1.06997119e0
versions nmr-bs1387-basic/1 anmr-kahles2019/1 erb-band-error/1 rnonlin-tan2004/1 gedlee-2003/1
metric 48000 tone/1k/+12 nmr_total_db -2.71e1
metric 48000 tone/1k/+12 gedlee 3.63e0
plot harmonics 48000 12 3 -1.22e1
table 384000 oversampled
… the R17 table at that rate …
end
```

A stage has no deck, so it is measured as a **black box**: its knee is
found at 1 kHz (where the gain moves 1 dB or THD reaches 1 %), and the
same plan runs at levels relative to that knee. The knee is recorded,
and a later run measures at the recorded knee, so like is compared with
like. If the knee itself moves, that is reported too.

## How CI diffs one

`cargo test -p stave-test --test characterise --test fingerprints`
measures each model again and compares its tables with the committed
ones, characteristic by characteristic. The comparison is the
characterisation's own, with the committed table taken as the
reference. Each characteristic has a numerical tolerance
(`stave_test::characterise::fingerprint::TOLERANCES`): 2e-5 V on an
operating point, 1e-3 dB and 0.01° on a response, 0.05 dB and 0.5° on a
harmonic (both held at −120 dBc), 0.5 dB on the aliasing floor and the
ANMR. The tolerances pass the difference between macOS's and Linux's
`libm` and fail anything audible. A failure names every characteristic
that moved, by how much, and where:

```text
fingerprint `diode_clipper` moved — the model sounds different from the one pinned. …
  C5 harmonics 48.0 kHz shipping: 3.120e-1 dB (tolerance 5e-2) at tone/1k/+12 bin 63: -40.123 vs -39.811 dBc
  C4 compression 48.0 kHz shipping: 2.004e-2 dB (tolerance 1e-3) at tone/1k/+12: …
```

If the change was intended, re-bless the fingerprints and review the
diff like any other change to the sound:

```sh
STAVE_BLESS=1 cargo test -p stave-test --test characterise --test fingerprints -- --skip plans
```

The metrics are derived from the tables, so the per-commit diff needs
only the tables. A separate test, `fingerprints_are_current`, computes
every committed fingerprint's metrics and plots again from its own
tables. A metric whose implementation changed (its version, or its
numbers) therefore shows up as a stale fingerprint to re-bless, never as
a model that moved.

The same diff works in code:

```rust
use stave_test::characterise::fingerprint::{diff, of_stage};

// A soft clipper as a black box at 48 kHz, and a slightly harder one
// measured at the first one's knee: the harmonics and the compression move.
let (soft, plan) = of_stage("doc/tanh", "tanh(2x)", None, |_| Box::new(|x: f64| (2.0 * x).tanh()), 48_000.0, None);
let (hard, _) = of_stage("doc/tanh", "tanh(2.1x)", None, |_| Box::new(|x: f64| (2.1 * x).tanh() / 1.05), 48_000.0, soft.knee);
assert!(diff(&soft, &soft, &plan).is_empty());
let moved = diff(&soft, &hard, &plan).join("\n");
assert!(moved.contains("C5 harmonics") && moved.contains("knee"), "{moved}");
```

## Golden digests: the stages a fingerprint cannot hold still

The T0 plan assumes a stage settles: a multisine or a tone, measured
once it is steady. A reverb's tail, a modulated delay, a pitch shifter,
a granulator, a compressor's attack and a bowed string do not settle,
so they are pinned by a **golden digest** instead (DSP.md R9,
`stave_test::golden`). Each such catalogued stage runs at 48 kHz over one
fixed, seeded stimulus of 0.68 s: an impulse and its tail, white noise
at −30 and −6 dBFS RMS with silence after each, and a 20 Hz–20 kHz
sweep. The output is kept as 33 Hann frames by 11 octave bands of
power in dB, plus its RMS and peak: about 4 KB of text under
`crates/stave-test/golden/`.

`crates/stave-test/tests/golden.rs` holds 60 stages to their digests
(reverbs, dynamics, EQ, modulation, pitch, granular, the physical models
and instruments, the tape transport, the photocell, the tuned
resonators). A band moves when it changes by more than 0.01 dB plus an
absolute error 100 dB below the digest's loudest band; macOS on aarch64
and Linux on x86_64 agree to the written 0.001 dB within 60 dB of the
loudest band. A failure lists each frame (as times) and band (as
frequencies) that moved, by how much and against what tolerance, largest
first. Re-bless with `STAVE_BLESS=1 cargo test -p stave-test --test golden`
and review the diff.

The nonlinear learned and physical stages (the NAM WaveNet and LSTM, the
GRU, both block models, the tape machine without flutter, the driver on
Klippel's curves) have fingerprints like the other stages. A network's
knee search starts at −60 dBFS (`pin_stage_from`), because its output
sits on a DC offset whose `f32` rounding looks like 1 % THD under a 10 µ
tone.

## Pinning a sound chosen by ear

In `stave-play`, **bless** in the Voicing tab
([Voicing and listening tests](crate::howto::voicing)) writes the
candidate's fingerprint into the bless record, along with its settings,
the material and the take. The candidate is fingerprinted as a black
box at the session's rate: a plugin or catalog stage, one sample at a
time, with a tenth of a second of silence before each run because a
plugin cannot be reset through the host API.
`stave-play --verify-blessed RECORD` runs the null test against the take
and measures the fingerprint again at the recorded knee. It lists every
characteristic that moved. The take pins the sound on one piece of
material; the fingerprint pins it on the whole measurement suite.

## The perceptual metrics

All of them live in `stave_measure::perceptual`. Each states its
source and carries a version that fingerprints record.

**NMR** (noise-to-mask ratio): ITU-R BS.1387's basic ear model as
Kabal's 2002 examination reads it. A residual's energy in each of 109
critical bands is set against the masking threshold the reference sets
there. The report gives BS.1387's total NMR, the worst band in any frame,
and the fraction of frames whose worst band is above 1.5 dB ("disturbed
frames"). Absolute level matters to an ear model, so the playback level
is stated: a full-scale sine plays at 92 dB SPL unless told otherwise.
Fingerprints play the reference's loudest T0 run at full scale.

```rust
use stave_measure::perceptual::{nmr::Nmr, Playback};

let rate = 48_000.0;
let tone: Vec<f64> = (0..24_000).map(|i| 0.5 * (std::f64::consts::TAU * 1_000.0 * i as f64 / rate).sin()).collect();
// A 0.1 dB level error hides under the tone; 10 dB does not.
let louder = |db: f64| tone.iter().map(|x| x * 10f64.powf(db / 20.0)).collect::<Vec<_>>();
let nmr = Nmr::new(Playback::default());
assert!(nmr.between(&tone, &louder(0.1), rate).max_db < 0.0);
assert_eq!(nmr.between(&tone, &louder(10.0), rate).disturbed, 1.0);
```

**ANMR** (A-weighted NMR, Kahles, Esqueda & Välimäki 2019): a tone's
inharmonic remainder (aliases, noise) under the mask its own harmonics
set, both A-weighted. Aliasing is audible above **−10 dB**
(`AUDIBLE_ANMR_DB`). Every characterisation's tone runs carry it at the
shipping rates, and `stave_measure::aliasing::anmr` measures it for any
capture:

```rust
use stave_measure::aliasing::SteppedTone;

// A hard clip at 44.1 kHz with no oversampling: audible aliasing over
// the top of the piano.
let sweep = SteppedTone { low_hz: 1_000.0, steps: 3, ..SteppedTone::piano(44_100.0, 3) };
let clip = sweep.anmr_through(|| |x: f32| x.clamp(-0.1, 0.1));
assert!(clip.worst().anmr_db > -10.0);
```

**ERB-band error**: two spectra compared in bands one ERB wide
(Glasberg & Moore). It reports the worst band, the RMS and the mean, in
dB. A level offset reads as a mean; a tilt shows in the bands it
touches.

```rust
use stave_measure::perceptual::erb::ErbError;

let x: Vec<f64> = (0..48_000).map(|i| ((i * 7919) % 1000) as f64 / 1000.0 - 0.5).collect();
let louder: Vec<f64> = x.iter().map(|v| v * 10f64.powf(1.0 / 20.0)).collect();
let e = ErbError::between(&x, &louder, 48_000.0);
assert!((e.mean_db - 1.0).abs() < 1e-6);
```

**Rnonlin** (Tan, Moore et al. 2004): how much nonlinear distortion is
heard. It is the level-weighted correlation, in 40 gammatone bands and
30 ms frames, between what went into a system and what came out. 1 is no
distortion and lower is more. It correlates with listeners at about 0.95
where THD and IMD manage about 0.45 (Moore 2020). A delay or a gain
leaves it at 1. A harder clip lowers it. At equal distortion power, a
higher order lowers it more.

```rust
use stave_measure::perceptual::rnonlin::rnonlin;

let x: Vec<f64> = (0..9_600).map(|i| 0.5 * (std::f64::consts::TAU * 220.0 * i as f64 / 48_000.0).sin() + 0.2 * (std::f64::consts::TAU * 1_330.0 * i as f64 / 48_000.0).sin()).collect();
let clipped: Vec<f64> = x.iter().map(|v| v.clamp(-0.3, 0.3)).collect();
assert!(rnonlin(&x, &x, 48_000.0).value > 0.999);
assert!(rnonlin(&x, &clipped, 48_000.0).value < rnonlin(&x, &x, 48_000.0).value);
```

**GedLee** (Geddes & Lee 2003): a static transfer curve's `G_m`. Its
second derivative weights higher orders more, and a `cos²` weights small
inputs more. A circuit with memory has no static curve, so fingerprints
take the curve that a tone's harmonics describe (their Chebyshev
coefficients), divided by the fundamental's gain.

```rust
use stave_measure::perceptual::gedlee::GedLee;

let gm = GedLee::of_curve(|x| x + x * x * x).gm;
assert!((gm - (36.0 * (1.0 / 3.0 - 2.0 / std::f64::consts::PI.powi(2))).sqrt()).abs() < 1e-5);
```

## Budgets by audibility: the null test

Every model-vs-reference comparison carries a verdict on what a listener
would hear (DSP.md R16). The model is matched to the reference in level
and latency, the way a listening test lines two takes up. Then the
residual, the reference minus the aligned model, is judged by its NMR
with the reference as the masker. The model is **inaudible** when the
total NMR and the worst band are both at −10 dB or less
(`stave_measure::perceptual::null::INAUDIBLE_DB`). At 0 dB the residual
sits at the masked threshold the ear model predicts. The 10 dB margin
is the one perceptual coding keeps for transparency, and the one
listening tests set for aliasing. It is a model's prediction, never a
listening result.

```rust
use stave_measure::perceptual::{null, Playback};

let rate = 48_000.0;
let x: Vec<f64> = (0..24_000).map(|i| 0.5 * (std::f64::consts::TAU * 440.0 * i as f64 / rate).sin()).collect();
// The same take, 1.5 dB quieter and three samples late: aligned out.
let model: Vec<f64> = (0..24_000).map(|i| if i < 3 { 0.0 } else { x[i - 3] * 10f64.powf(-1.5 / 20.0) }).collect();
let n = null::between(&x, &model, rate, Playback::default(), 1e-3);
assert!((n.alignment.gain_db() - 1.5).abs() < 0.01 && n.inaudible());
```

Where it is reported:

- **The characterisation** (`Check::Audibility`, "R16 null NMR"): each
  tier's multisines, tones and two-tone pairs, resynthesised from the
  lines both tables carry, one alignment over all of them, at the
  fingerprint's playback level. Every case has a budget of −10 dB total
  and worst band. A case over it is a finding, held at its measurement
  plus 5 dB. The numeric budgets stay beside it as regression guards.
- **Fingerprints**: `null_nmr_total_db` and `null_nmr_max_db` per run,
  the alignment under the run `alignment`. The model catalogue prints
  the verdict next to each fingerprint and its numbers. When a pinned
  fingerprint moves, the failure also says how audible the move is.
- **The reference harness** (`stave_test::reference::Case`): every
  time-domain comparison at an audio rate prints an `audible` line next
  to its numeric error.
- **`stave-play --verify-blessed`**: the null of the build against its
  blessed take.
- **Program material** (T1, `null_on_program_material`): with the
  reference corpus present, each circuit at the corpus's rate as it
  ships, against itself at 16×, on its `guitar_di`, `vocal` and
  `drums_loop` roles. C13's `stave_test::corpus::c13::NullNmr` prints
  the aligned verdict beside its own figures.

## Aliasing budgets by audibility

The anti-aliasing budgets (`crates/stave-dsp/tests/aliasing_budgets.rs`)
hold every nonlinear stage, oscillator, circuit and learned stage to an
ANMR of −10 dB or less. The ANMR is measured with each one's default
anti-aliasing over the piano's range (27.5 Hz to 4186 Hz). A row above
−10 dB is a **finding**: it is recorded with a budget 3 dB above its
measurement, so it cannot get worse unnoticed. Today that covers the
comparison rows at 1× and 2× and the polyBLEP oscillators. It also
covers the first-order ADAA curves at 44.1 kHz, the triode at 1×, and two
learned fixtures. The older budgets in dB below the fundamental stay
beside them as a second check.

The characterisation's T0 carries an ANMR check at 48 kHz for every
circuit (the 1 kHz tone at −20, 0 and +12 dB re its knee). T1 sweeps
every semitone of the piano at +12 and +20 dB re the knee at 44.1 kHz.
There, most circuits alias audibly at the host rate without
oversampling, which is the case for the plugin's oversampling or ADAA,
and each is recorded as a finding.

## What they cost

The metrics are offline analyses, priced per analysed sample at 48 kHz
on the datasheet (the docs site's Cost page):
`"Nmr (BS.1387 basic, 48 kHz)"`, `"ErbError (ERB bands, 48 kHz)"`,
`"Rnonlin (40 gammatone bands, 48 kHz)"` and `"GedLee (15 harmonics)"`.
The per-commit fingerprint diff computes none of them. It compares
tables, and the ANMR of a T0 tone run adds about 2 ms per run, so the
T0 tier stays under half a second per circuit. The null test adds
about 25 ms to a T0 characterisation (both rates, six runs each). Re-blessing and
`fingerprints_are_current` compute all of them: a few seconds for the
whole tree.
