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
and `stave-fit` runs both (DSP.md M3, M4).

## From a device

1. **Write the stimulus.**

   ```text
   cargo run --release -p stave-fit -- stimulus capture.wav --reference 0.05
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
   cargo run --release -p stave-fit -- fit --stimulus capture.wav \
       --capture recording.wav -o pedal.json
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
cargo run --release -p stave-fit -- fit --circuit tube_screamer -o ts.json
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
