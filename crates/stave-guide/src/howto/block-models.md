# Capture and fit a block model

A block-oriented model is two linear filters around a static curve
(Wiener–Hammerstein), or one filter per power of the input summed
(the polynomial Hammerstein model). It is small: a few biquads and a
curve, tens of numbers, tens of nanoseconds a sample. It is also
readable: its filters are responses you can plot and its curve is a
transfer you can see. It cannot hold memory inside the nonlinearity,
such as a corner that moves with level or bias that shifts after a
burst. The fit measures that before you trust it. The models are
`stave_dsp::learned::block`, the fitting is `stave_measure::identify`,
and `stave fit` runs both (DSP.md M3, M4; also an MCP tool, `fit`).

## From a device

1. **Write the stimulus.**

   ```text
   stave fit stimulus capture.wav --reference 0.05
   ```

   `capture.wav` is about 30 s long, 48 kHz, mono, 32-bit float. It holds
   random-phase multisines at −20, −6, 0, +6 and +12 dB re the reference
   RMS (four realisations each, every period played four times so the
   last is steady state), a synchronized exponential sweep from 5 Hz
   whose peak is the 0 dB sine's, and noise bursts from −20 to +12 dB
   that the fit never sees and that serve as validation.
   `capture.wav.capture` beside it describes the design, and the fit
   rebuilds the stimulus exactly from that description. Choose the
   reference so that +12 dB drives the device as hard as you play it.

2. **Record it through the device.** Play the file from its start and
   record the device's output on channel 1. If you can, also record the
   interface's loopback of the stimulus on channel 2. Keep the
   recording outside the repository. Third-party captures are never
   committed.

3. **Fit.**

   ```text
   stave fit --stimulus capture.wav \
       --capture recording.wav --out pedal.json
   ```

   The latency comes from `--latency`, if given. Otherwise it comes
   from the cross-correlation peak against the loopback, which is exact.
   Without a loopback it comes from the peak against the device's own
   output, which adds the device's group delay, so record a loopback
   when the phase matters. `--structure` chooses the model:
   `wiener-hammerstein` (the default), `wiener`, `hammerstein`, or
   `parallel` (the polynomial Hammerstein model, from the sweep).
   `--export-pair DIR` also writes the aligned input and output as WAVs,
   for training a network out of tree (Neural Amp Modeler trains from
   such a pair; inference is `stave_dsp::learned::nam`).

## From a stave circuit

```text
stave fit --circuit tube_screamer --out ts.json
```

This fits any R17 deck (`crates/stave-test/spice/<name>.cir`), measured
through `Circuit<Dk>` at 4× and brought back to 48 kHz band-limited, with
0 dB at the circuit's knee. From code, `Capture::run` measures any system
you can tick, such as a stave `Stage` or a plugin, the same way.

## Reading the report

The note at the top of the model file repeats the report:

- **BLA order and split.** The best linear approximation's rational fit
  (its worst error is near Nyquist, where no low order follows an analog
  phase) and which poles and zeros went before the curve and which after.
- **In-band error by level.** The fit's error over 10 Hz–20 kHz at each
  multisine level, in dB re the output.
- **BLA shape against level.** This is the structure test. The BLA of a
  Wiener–Hammerstein system moves only in gain as the level rises
  (Schoukens & Ljung 2019). The report gives the third-octave deviation
  that remains beyond twice its standard error over the realisations.
  If it stays near 0 dB, the structure fits. If it grows by several dB
  at the top level, the device's filtering moves with drive and the fit
  will fall short there.
- **Held-out bursts.** The simulation error, ERB-band error and NMR
  (`stave_measure::perceptual`) of the model against the device on audio
  it was not fitted to, burst by burst.

## What to expect

Measured on stave's circuits (`crates/stave-test/tests/block_models.rs`):

| Circuit | BLA shape at +12 dB | W–H held-out −20 / +12 dB | `Circuit<Dk>` at 48 kHz | Polynomial Hammerstein |
|---|---|---|---|---|
| Common cathode | 0.03 dB | −57.3 / −30.0 dB | −57.2 / −29.4 dB | −41.2 / −2.7 dB |
| Diode clipper | 3.7 dB | −28.6 / −10.9 dB | −16.2 / −13.2 dB | −23.8 / −5.2 dB |
| Tube Screamer gain stage | 4.0 dB | −31.4 / −11.5 dB | −16.9 / −15.5 dB | −18.5 / −3.4 dB |

The triode is a Wiener–Hammerstein system to within its measurement, so
the fit matches the circuit level for level at a fraction of the circuit's
cost. The clippers' corners move with drive. Their fits are closer to
the truth than the circuit solved at 48 kHz below the knee, because the
fit was made to the oversampled circuit and so has no bilinear warping,
but they are further from it at +12 dB. A power series fitted at one
level holds near that level and nowhere else, and it aliases audibly
when driven past it.

## Kernels as an analysis

The same synchronized sweep also describes a device without fitting
anything (DSP.md M6, characteristic C12). Deconvolved, one sweep
separates one response per harmonic (Farina). Novak's conversion turns
those responses into generalised Hammerstein kernels `Gₘ(f)`, one filter
per power of the input. Sweeps at several levels also give the diagonals
of the low-order Volterra kernels, `Vₘ(f/m, …, f/m)`, because a
polynomial in the level fitted per frequency separates each order from
the higher ones that also reach its harmonic. This is an analysis, not a
model to run. It gives a compact picture of a weak nonlinearity that you
can compare across stave's models, a third-party plugin (an Audio Unit
or a VST3 as a black box) and a capture. The measurement is
`stave_measure::identify::kernels`, and the characterisation runs it at
levels relative to the knee (`stave_test::characterise::kernels`):

```
use stave_measure::suite::levels::{Criterion, Knee};
use stave_test::characterise::fingerprint::stage_plan;
use stave_test::characterise::kernels::{measure, KernelPlan};
use stave_test::characterise::Model;

// A soft clipper with a little even order, as a black box whose knee is
// a 0.5 peak.
let model = Model::Stage {
    make: Box::new(|_rate| Box::new(|x: f64| (x + 0.1 * x * x).tanh())),
    operating_point: Vec::new(),
};
let knee = Knee { amplitude: 0.5, criterion: Criterion::Distortion };
let plan = stage_plan("guide/soft", knee, 0.0);
// Sweeps at −30, −20 and −10 dB re knee (shortened here from 4.5 s).
let sweeps = KernelPlan { low_hz: 5.0, seconds: 1.0, ..KernelPlan::default() };
let set = measure(&model, &plan, &sweeps).unwrap();

// The harmonics of a 1 kHz sine at −10 dB re knee, read from that sweep:
// about 0.05·a for H2 and a²/12 for H3 at a = 0.16.
let k = set.at(-10.0).unwrap();
let (h2, h3) = (k.harmonic_dbc(2, 1_000.0).unwrap(), k.harmonic_dbc(3, 1_000.0).unwrap());
assert!((-43.0..-41.0).contains(&h2) && (-55.0..-52.0).contains(&h3), "{h2} {h3}");
// A memoryless curve is a generalised Hammerstein system, so its G2 is
// the same at every level.
assert!(set.level_dependence(2, (100.0, 10_000.0)).unwrap() < 1e-2);
// Its Volterra diagonal V2 is flat at the curve's x² coefficient.
let v2 = set.volterra.as_ref().unwrap().kernels[1].at(2_000.0).unwrap();
assert!((v2.re - 0.1).abs() < 1e-3);
```

To keep a model's kernels in its fingerprint, call
`Characterise::kernels(KernelPlan::default())` on its characterisation, or
use `fingerprint::pin_stage_with` for a black box. The model catalogue
then draws `V2` and `V3` at the knee, and `G2` and `G3` at each level.
Each is shown as its contribution at that level, which is the dBc that
harmonic of a sine at that level reads. The diode clipper, the Tube
Screamer gain stage, the common cathode and the catalogued `Triode` keep
their kernels this way.

What the kernels can and cannot say:

- **Weak nonlinearity.** The expansions hold while a short power series
  describes the device: below its knee. Past the knee, a clipper's
  kernels describe that level alone. `level_dependence` measures how far
  `G` moves between levels, which is Novak's test for whether one
  Hammerstein model holds. The Volterra diagonals' weak-signal reading
  then parts from the measured harmonics. On the diode clipper at −3 dB
  re knee it under-reads H3 by about 15 dB.
- **Time invariance and memory.** A device that changes during the
  sweep, or whose response outlasts the gap between two harmonics'
  arrivals (`½·L·ln((n+1)/n)`), smears its harmonics into each other.
  Lengthen the sweep for long memories such as tape, reverb or magnetics.
- **Band and level.** Kernels are measured from `2·order·f₁` to
  `0.9·f₂`. The default sweep from 2 Hz keeps the separation's leakage
  below −100 dB re the linear response from about 60 Hz. A level too low
  leaves the higher kernels in that floor.
- **Aliasing.** A model run at its host rate carries its own aliasing
  into the kernels. The catalogued `Triode` at 48 kHz shows it as a rise
  of G2 and G3 above a few kHz.

The harmonics the kernels read agree with the characterisation's
coherent tones at the same level to 0.08 dB or better on the common
cathode, the diode clipper and the Tube Screamer stage, at −20, −3 and
0 dB re knee, both against the model's own tables and against ngspice
(`crates/stave-test/tests/kernels.rs`).
