# The DSP library

`stave-dsp` (re-exported as [`stave::dsp`](stave::dsp)) is a vocabulary
rather than a bag of filters: layers ordered by the claim a model makes,
one processing contract for every leaf and composite, and composition
forms whose laws are pinned by tests.

## Layers, pointing down

Each layer may use only the layers before it. The folders under
`crates/stave-dsp/src/` name the claim (and so the verification), and
`scripts/check-view-bindings.sh` fails the build when shipped code in a
layer reaches `crate::<layer>` of a layer above it (test modules may
compose across layers; a missing layer directory fails the gate rather
than silently checking nothing).

| Layer | What it holds | May use |
|---|---|---|
| 0 [`units`](stave_dsp::units) | quantities as types: [`Hz`](stave_dsp::Hz), [`Db`](stave_dsp::Db), [`Gain`](stave_dsp::Gain), [`Seconds`](stave_dsp::Seconds), [`Samples`](stave_dsp::Samples), [`SampleRate`](stave_dsp::SampleRate), [`Ratio`](stave_dsp::Ratio), [`Semitones`](stave_dsp::Semitones), [`Cents`](stave_dsp::Cents), and the circuit units [`Volts`](stave_dsp::Volts), [`Amps`](stave_dsp::Amps), [`Ohms`](stave_dsp::Ohms), [`Farads`](stave_dsp::Farads), [`Henries`](stave_dsp::Henries) | nothing |
| 1 [`kernel`](stave_dsp::kernel) | the [`Stage`](stave_dsp::Stage) contract and [`Tail`](stave_dsp::Tail); the composition forms; the denormal and non-finite guards | units |
| 2 [`primitives`](stave_dsp::primitives) | filters ([`Biquad`](stave_dsp::Biquad), [`Svf`](stave_dsp::Svf), [`OnePole`](stave_dsp::OnePole), [`DcBlocker`](stave_dsp::DcBlocker), [`Allpass1`](stave_dsp::Allpass1), [`LinkwitzRiley4`](stave_dsp::LinkwitzRiley4), [`Crossover3`](stave_dsp::Crossover3)); time ([`DelayLine`](stave_dsp::DelayLine), [`Comb`](stave_dsp::Comb), [`DelayAllpass`](stave_dsp::DelayAllpass)); control (LFOs, envelopes, followers, smoothers, [`RandomWalk`](stave_dsp::RandomWalk)); [`Osc`](stave_dsp::Osc) and [`Waveform`](stave_dsp::Waveform); spectral ([`Fft`](stave_dsp::Fft), [`Convolver`](stave_dsp::Convolver), [`ImpulseResponse`](stave_dsp::ImpulseResponse)); gain and pitch helpers; seeded randomness | units, kernel |
| 3 [`behavioral`](stave_dsp::behavioral) | models fitted to measurement or chosen by ear: tabulated curves with ADAA, [`soft_clip`](stave_dsp::soft_clip) and the [`shapes`](stave_dsp::shapes) library, [`SmoothSat`](stave_dsp::SmoothSat), [`Slew`](stave_dsp::Slew), [`Hysteresis`](stave_dsp::Hysteresis), [`NonlinearRc`](stave_dsp::NonlinearRc) | units, kernel, primitives |
| 4 [`circuit`](stave_dsp::circuit) | devices, solver methods, netlists and topologies derived from schematics | units, kernel, primitives, behavioral |
| 5 [`physical`](stave_dsp::physical) | models derived from mechanics or acoustics (empty until the first lands) | the layers above |
| 6 [`learned`](stave_dsp::learned) | models trained from captures: [`WaveNet`](stave_dsp::learned::WaveNet), [`Lstm`](stave_dsp::learned::Lstm), [`Gru`](stave_dsp::learned::Gru), and Neural Amp Modeler's `.nam` files ([`nam::parse`](stave_dsp::learned::nam::parse)) | the layers above |
| 7 [`effects`](stave_dsp::effects) | algorithmic designs that model no hardware: [`reverb`](stave_dsp::effects::reverb) (FDN, Dattorro plate, Schroeder/Moorer) and [`dynamics`](stave_dsp::effects::dynamics) (detectors, gain computer, compressor, look-ahead limiter) | the layers above |
| 8 [`structures`](stave_dsp::structures) | [`Chain`](stave_dsp::Chain) (serial composition with addressable slots), [`chain!`](stave_dsp::chain), [`ModMatrix`](stave_dsp::ModMatrix) | all of the above |

The crate root re-exports the public vocabulary flat, so
`stave_dsp::Biquad` and `stave::dsp::Biquad` are the same type, and the
most common pieces are in [`stave::prelude`](stave::prelude).

## The Stage contract

Every unit of DSP, leaf or composite, implements one trait. This is its
whole surface, verbatim:

```rust
# use stave_dsp::Tail;
pub trait Stage {
    fn prepare(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32;
    fn process(&mut self, block: &mut [f32]) {
        for s in block {
            *s = self.tick(*s);
        }
    }
    fn latency(&self) -> u32 {
        0
    }
    fn tail(&self) -> Tail {
        Tail::None
    }
}
```

- **One step verb.** [`tick`](stave_dsp::Stage::tick) advances one
  sample. [`process`](stave_dsp::Stage::process) is the block form, and
  an override must stay bit-identical to the tick loop
  (`crates/stave-dsp/tests/block_api.rs` checks every block path over
  ragged block sizes). A generator (oscillator, LFO, envelope) ignores
  its input and is also a [`Generator`](stave_dsp::Generator), read with
  `next()`. Multi-output filters keep extra methods
  ([`Svf::taps`](stave_dsp::Svf::taps),
  [`Crossover3::split`](stave_dsp::Crossover3::split)); some circuits add
  an `f64` `tick_f64`.
- **The rate contract.** [`prepare`](stave_dsp::Stage::prepare) is told
  the rate `tick` will run at (inside an oversampler, the oversampled
  rate) and caches every rate-derived coefficient. It may allocate;
  everything else is realtime. Setters take musical units only, never a
  sample rate. Before `prepare`, filters assume 48 kHz.
  `crates/stave-test/tests/rate_contract.rs` pins that a re-prepared
  stage is indistinguishable from one built fresh at the new rate.
  Storage that sizes itself does so in an explicit
  `allocate(samples)` ([`Compensator::allocate`](stave_dsp::Compensator::allocate)).
- **Latency and tail are reported, and forwarded.**
  [`latency`](stave_dsp::Stage::latency) is group delay in samples at
  the prepared rate. [`Tail`](stave_dsp::Tail) is `None`,
  `Samples(n)` or `Infinite`, the same three cases a plugin reports to
  its host. Filters estimate the samples to fall 120 dB from their
  poles ([`Tail::from_pole`](stave_dsp::Tail::from_pole)).

Here is the block form agreeing with the tick loop, bit for bit:

```rust
use stave_dsp::{Biquad, FilterSpec, Stage};

let spec = FilterSpec::Peak { hz: 1_000.0, db: 6.0, q: 1.0 };
let (mut blockwise, mut tickwise) = (Biquad::new(spec), Biquad::new(spec));
blockwise.prepare(48_000.0);
tickwise.prepare(48_000.0);

let input: Vec<f32> = (0..256).map(|n| ((n * 37) % 101) as f32 / 50.0 - 1.0).collect();
let mut block = input.clone();
blockwise.process(&mut block);
let ticked: Vec<f32> = input.iter().map(|&x| tickwise.tick(x)).collect();
assert_eq!(block, ticked);
```

## Composition forms

The kernel's forms build graphs out of stages. Each owns the invariant
that makes it correct, and `crates/stave-dsp/tests/kernel_properties.rs`
asserts those invariants with probe leaves.

| Form | Type | Law |
|---|---|---|
| Sequence | [`Serial`](stave_dsp::Serial) | prepare cascades; latencies sum; tails add (an infinite tail absorbs) |
| Branch | [`Par2`](stave_dsp::Par2) + [`Compensator`](stave_dsp::Compensator) | both paths arrive at the join aligned; each branch rings on through its pad |
| Loop | [`Loop`](stave_dsp::Loop) | feedback only through a `NonZeroU32` delay; tail infinite unless a bound is declared |
| Oversampled | [`Oversampled2x`](stave_dsp::Oversampled2x) | the interior is prepared at twice the rate; latency and tail are divided back, the tail rounding up |
| Lanes | [`PerChannel`](stave_dsp::PerChannel) + [`ChannelLayout`](stave_dsp::ChannelLayout) | one state per lane; lanes never mix |
| Control | [`Source`](stave_dsp::Source), [`Route`](stave_dsp::Route), [`Router`](stave_dsp::Router) | routes displace from the knob's base value |
| Voices | [`VoiceRack`](stave_dsp::VoiceRack), [`Voice`](stave_dsp::Voice), [`VoiceEvent`](stave_dsp::VoiceEvent) | a fixed pool under a declared [`Steal`](stave_dsp::Steal) policy (`tests/voice_properties.rs`) |

The latency arithmetic, checked:

```rust
use std::num::NonZeroU32;
use stave_dsp::{Compensator, Loop, Oversampled2x, Par2, Serial, Stage, Tail};

// Two fixed delays, 3 and 5 samples.
let pad = |n| { let mut c = Compensator::new(); c.allocate(n); c };

// Serial: latencies and tails sum.
let serial = Serial((pad(3), pad(5)));
assert_eq!(serial.latency(), 8);
assert_eq!(serial.tail(), Tail::Samples(8));

// Parallel: the join waits for the slower branch; the faster is padded.
let mut par = Par2::new(pad(3), pad(5));
par.prepare(48_000.0);
assert_eq!(par.latency(), 5);
assert_eq!(par.tail(), Tail::Samples(5));

// A loop cannot know its own stability: infinite until a bound is declared.
let feedback = Loop::new(pad(0), NonZeroU32::new(64).unwrap());
assert_eq!(feedback.tail(), Tail::Infinite);
assert_eq!(feedback.with_tail_bound(4_800).tail(), Tail::Samples(4_800));

// Oversampling: the front half-band pair costs 66 samples; a 4x stack
// adds the nested pair's 36 inner samples, 18 at the outer rate.
let x2 = Oversampled2x::front(());
assert_eq!(x2.latency(), 66);
let x4 = Oversampled2x::front(Oversampled2x::nested(()));
assert_eq!(x4.latency(), 84);
```

[`Oversampled2x`](stave_dsp::Oversampled2x) is the only oversampler:
[`front`](stave_dsp::Oversampled2x::front) is the full-steepness pair
for the outermost region and
[`nested`](stave_dsp::Oversampled2x::nested) the relaxed pair inside it.
[`tick_with`](stave_dsp::Oversampled2x::tick_with) runs a closure once
per inner sample, so per-sample context reaches the faster domain
without being parked in fields.

[`ModMatrix`](stave_dsp::ModMatrix) is a [`Router`](stave_dsp::Router)
whose targets are a chain's stage axes. [`VoiceEvent`](stave_dsp::VoiceEvent)
is the kernel's own note vocabulary; plugins convert host notes with
[`ToVoiceEvent`](stave::ToVoiceEvent) and tune with
[`hz_of_key`](stave_dsp::hz_of_key) or their own map.

## Chains with named slots

[`chain!`](stave_dsp::chain) declares a serial topology whose shape in
the source is the signal path. It expands to a struct around a
[`Chain`](stave_dsp::Chain): one [`StageId`](stave_dsp::StageId) field
per slot (`amp.voicing`, for routes, swaps and sense reads), one
accessor per slot (`amp.voicing()`, the typed stage), and a
[`Stage`](stave_dsp::Stage) impl so the whole chain nests in any form.

```rust
use stave_dsp::{Biquad, FilterSpec, Slew, Stage};

stave_dsp::chain! {
    /// A slew limiter into a high shelf, in signal order.
    pub struct Voicing {
        limit: Slew = Slew::new(10.0, 10.0),
        shelf: Filter = Biquad::new(FilterSpec::HighShelf { hz: 3_000.0, db: -3.0 }),
    }
}

let mut v = Voicing::new();
v.prepare(48_000.0);
v.shelf().set(FilterSpec::HighShelf { hz: 2_000.0, db: -6.0 });
let y = v.tick(0.25);
assert!(y.is_finite());
// Every join is a sense endpoint: the slot's output, by its id.
assert_eq!(v.as_chain().sense(v.shelf), y);
```

[`Chain::sense`](stave_dsp::Chain::sense) is the one surface interior
signals are read from, for control wiring and observation alike.
[`SwapPoint`](stave_dsp::SwapPoint) parks a slot's alternate stage and
swaps it in by move, so a structural switch never allocates.

## Spectral: the FFT, convolution, the STFT and FIR design

[`primitives::spectral`](stave_dsp::primitives::spectral) holds the
workspace's one [`Fft`](stave_dsp::Fft) and the
[`Convolver`](stave_dsp::Convolver) built on it. Both plan where
allocation is allowed (a constructor or `prepare`) and never allocate
afterwards.

- [`Fft`](stave_dsp::Fft) is planned for one power-of-two length, in
  `f32` or `f64`, and serves the complex transform of that length and the
  real one (`n` samples to `n/2 + 1` bins). The inverse is unnormalised,
  as in FFTW: a round trip returns the input times `n`.
- [`Convolver::uniform`](stave_dsp::Convolver::uniform) convolves with an
  [`ImpulseResponse`](stave_dsp::ImpulseResponse) of any length in
  partitions of one power-of-two block (overlap-save with a
  frequency-domain delay line). Its latency is one block, reported
  exactly, or zero with
  [`Head::Direct`](stave_dsp::primitives::spectral::Head::Direct), which
  runs the first block in direct form. A mono response is a
  [`Stage`](stave_dsp::Stage), so it nests in any form; stereo and
  true-stereo responses run through
  [`tick_stereo`](stave_dsp::Convolver::tick_stereo).
- An [`ImpulseResponse`](stave_dsp::ImpulseResponse) loaded from a
  capture ([`from_channels`](stave_dsp::ImpulseResponse::from_channels),
  or [`stave_host::wav::read_impulse_response`] for a WAV) carries the
  rate it was recorded at, and the convolver resamples it to the rate it
  is prepared at. [`normalized`](stave_dsp::ImpulseResponse::normalized)
  and [`minimum_phase`](stave_dsp::ImpulseResponse::minimum_phase)
  prepare it further.
- [`Convolver::non_uniform`](stave_dsp::Convolver::non_uniform) keeps
  that latency but covers a long response with larger partitions too,
  sized for the least work: a two-second hall at zero latency costs about
  half of what the uniform schedule costs at 256 samples of latency.

Convolving with a short response, checked against the direct sum:

```rust
use stave_dsp::primitives::spectral::Head;
use stave_dsp::{Convolver, ImpulseResponse, Stage};

// A short decaying response, longer than one partition.
let h: Vec<f32> = (0..100).map(|j| 0.97_f32.powi(j) * if j % 2 == 0 { 1.0 } else { -0.5 }).collect();
let mut conv = Convolver::uniform(&ImpulseResponse::mono(&h), 32, Head::Partitioned);
conv.prepare(48_000.0);
assert_eq!(conv.latency(), 32); // one partition

let x: Vec<f32> = (0..400).map(|n| ((n * 37) % 101) as f32 / 50.0 - 1.0).collect();
let mut y = x.clone();
conv.process(&mut y);

// y is the direct-form convolution, one partition late.
for t in 32..y.len() {
    let want: f32 = (0..h.len().min(t - 31)).map(|j| h[j] * x[t - 32 - j]).sum();
    assert!((y[t] - want).abs() < 1e-4, "sample {t}");
}
```

The tests hold both to naive references: `tests/fft_properties.rs`
compares the transform with a direct DFT at every length from 2 to
2^16, and `tests/convolver_properties.rs` compares the convolver with
direct-form convolution over random responses and ragged blocks.

### The STFT

[`Stft`](stave_dsp::primitives::spectral::Stft) is a
[`Stage`](stave_dsp::Stage) that analyses the last `size` samples every
`hop`, hands each spectrum (`size/2 + 1` bins, DC to Nyquist) to a
closure, and overlap-adds the result back. The window is applied at
analysis and at synthesis, so reconstruction is perfect when the squared
window overlap-adds to a constant at that hop:
[`Stft::new`](stave_dsp::primitives::spectral::Stft::new) checks it and
refuses a pair that is not COLA (Hann at a quarter of its size is;
at half it is not, but `SqrtHann` is). The latency is `size` samples,
reported.

A spectral gate: bins quieter than a threshold are muted, the rest pass.
A loud tone survives, quiet noise is removed:

```rust
use stave_dsp::primitives::spectral::{Complex, Stft, Window};
use stave_dsp::{Stage, Xorshift32};

let size = 1024;
let threshold = 20.0; // in unnormalised bin magnitude
let mut gate = Stft::new(size, size / 4, Window::Hann, move |bins: &mut [Complex<f32>]| {
    for z in bins.iter_mut() {
        if z.norm() < threshold {
            *z = Complex::default();
        }
    }
})
.expect("Hann is COLA at a quarter of its size");
gate.prepare(48_000.0);
assert_eq!(gate.latency(), 1024);

let mut rng = Xorshift32::new(1);
let tone = |t: usize| (core::f32::consts::TAU * 1_000.0 * t as f32 / 48_000.0).sin();
let mut residual = 0.0f32;
for t in 0..8 * size {
    let y = gate.tick(tone(t) + 0.05 * rng.next_bipolar());
    if t >= 2 * size {
        residual = residual.max((y - tone(t - size)).abs());
    }
}
assert!(residual < 0.01); // the noise is gone; the tone is untouched
```

### FIR design

[`Fir`](stave_dsp::primitives::spectral::Fir) designs a filter's taps
once, off the audio thread, from a specification; the
[`Convolver`](stave_dsp::Convolver) runs them. Frequencies are fractions
of the sample rate (`hz / sample_rate`, `0` to `0.5`), since a design is
made for one rate.

- [`Fir::kaiser`](stave_dsp::primitives::spectral::Fir::kaiser) takes a
  [`Passband`](stave_dsp::primitives::spectral::Passband) (lowpass,
  highpass or bandpass), the stopband attenuation in dB and the
  transition width, and picks the length and the Kaiser window for you;
  [`Fir::windowed_sinc`](stave_dsp::primitives::spectral::Fir::windowed_sinc)
  takes the length and any [`Window`](stave_dsp::primitives::spectral::Window).
- [`Fir::least_squares`](stave_dsp::primitives::spectral::Fir::least_squares)
  fits weighted [`Band`](stave_dsp::primitives::spectral::Band)s, leaving
  the gaps between them free.
- Both are linear phase: symmetric taps, `(len − 1)/2` samples of delay.
  [`Fir::minimum_phase`](stave_dsp::primitives::spectral::Fir::minimum_phase)
  keeps the magnitude and gives up linear phase for the least delay.

A lowpass at 4.8 kHz (at 48 kHz), 80 dB down from 6.72 kHz, checked
against its specification and run at zero latency:

```rust
use stave_dsp::primitives::spectral::{Fir, Head, Passband};
use stave_dsp::{Convolver, Stage};

let sample_rate = 48_000.0;
let fir = Fir::kaiser(Passband::Lowpass { cutoff: 4_800.0 / sample_rate }, 80.0, 0.04);
assert!(fir.gain_db(1_000.0 / sample_rate).abs() < 0.001); // passband: flat
assert!(fir.gain_db(8_000.0 / sample_rate) < -79.0); // stopband: 80 dB down

// Linear phase delays every frequency by half the length; the
// minimum-phase version keeps the magnitude and arrives at once.
let quick = fir.minimum_phase();
assert!((quick.gain_db(1_000.0 / sample_rate) - fir.gain_db(1_000.0 / sample_rate)).abs() < 0.001);
assert!(quick.gain_db(8_000.0 / sample_rate) < -77.0);
let peak = |f: &Fir| (0..f.len()).max_by(|&a, &b| f.taps()[a].abs().total_cmp(&f.taps()[b].abs())).unwrap();
assert_eq!(peak(&fir), (fir.len() - 1) / 2);
assert!(peak(&quick) < 16);

let mut conv = Convolver::uniform(&quick.impulse_response(), 64, Head::Direct);
conv.prepare(sample_rate as f32);
assert_eq!(conv.latency(), 0);
```

## The circuit layer

[`circuit`](stave_dsp::circuit) follows SPICE practice: `devices/`
(model cards: the [`Koren`](stave_dsp::Koren) and
[`Dempwolf`](stave_dsp::Dempwolf) triodes, the
[`Shockley`](stave_dsp::Shockley) diode, the
[`JilesAtherton`](stave_dsp::JilesAtherton) magnetic core), `methods/`
(load lines, Newton with junction limiting, wave digital filters,
Wright-omega closed forms) and `topologies/` (common cathode, triode
stage, push-pull pair, power supply, coupler, tone stack, diode clipper,
op-amp clipper, transformer).

Any circuit can also run straight from its netlist:
[`Netlist::parse_spice`](stave_dsp::circuit::netlist::Netlist::parse_spice)
reads the SPICE subset the ngspice decks are written in, and
[`Circuit<Dk>`](stave_dsp::circuit::Circuit) solves it by the nodal DK
method — the state space computed once per prepare, Newton only on the
devices' controlling voltages. The hand-derived topologies are held to
`Circuit<Dk>` on the same netlist (`dk_matches_ngspice.rs`).

The models are held to references outside the code:

- `crates/stave-test/tests/matches_ngspice.rs` compares against ngspice
  waveforms that `scripts/ngspice-simulate.py` regenerates, after first
  checking ngspice itself on circuits with exact answers; CI regenerates
  them with a pinned ngspice and fails on a stale one. Every case runs in
  one harness, with its metric and budget (see
  [models against references](crate::concepts::verification#models-against-references)).
- `crates/stave-test/tests/matches_reference.rs` compares what SPICE
  has no model for (the magnetic core, and the transformer built on it)
  against SciPy solutions of their equations from `scripts/ja-reference.py`, and requires the error to
  fall as the step shrinks.
- `crates/stave-test/tests/solver_conformance.rs` requires every
  Newton-solved topology to converge at the host rate under a hot
  signal: no solve may end at its iteration cap.

Physical behaviour belongs to the library; a product that wants
exaggeration owns it in its own code.

## Units

Quantities are types ([`units`](stave_dsp::units)), so a frequency
cannot be passed where a level belongs. Each is a `Copy` newtype,
`#[repr(transparent)]` and inlined, so it costs nothing; constructors
read as a datasheet writes the value (`ms(30.0)`, `khz(2.4)`,
`kohm(4.7)`, `nf(22.0)`), check the range in debug builds only, and
`get()` reads the scalar back. Conversions are named methods, and the
rate-derived ones take the one prepared-rate type,
[`SampleRate`](stave_dsp::SampleRate) (an
[`Environment`](stave::Environment) converts into one with
`SampleRate::from(env)`):

```rust
use stave_dsp::units::{db, hz, kohm, ma, ms, nf, semitones, volts, SampleRate};

let sample_rate = SampleRate::new(48_000.0);
assert_eq!(ms(10.0).samples(sample_rate).get(), 480.0);       // time → samples
assert!((db(-6.0).to_gain().get() - 0.501).abs() < 1e-3); // dB → linear gain
assert_eq!(hz(500.0).period(), ms(2.0));               // frequency → period
assert_eq!(hz(440.0) * semitones(-12.0).ratio(), hz(220.0)); // pitch
assert_eq!(volts(9.0) / kohm(4.5), ma(2.0));           // Ohm's law
let tau = kohm(10.0) * nf(100.0);                      // RC is a time constant
assert!((tau - ms(1.0)).get().abs() < 1e-15);
assert_eq!(hz(2_400.0).to_string(), "2.40 kHz");       // reads as a Unit::Hz param
assert_eq!(kohm(4.7).to_string(), "4.70 kΩ");
```

Operators exist only where they read as physics. Adding a level to a
frequency, or a duration to a sample count without a rate, does not
compile:

```rust,compile_fail
use stave_dsp::units::{db, hz};
let nonsense = hz(440.0) + db(-6.0);
```

Audio-rate quantities (`Hz`, `Db`, `Gain`, `Ratio`, pitch, `SampleRate`)
are `f32` like the samples and setters they meet; durations
(`Seconds`, `Samples`) are `f64` because they become sample counts,
which `f32` stops counting exactly past 2²⁴; circuit units are `f64`
like the solvers. The setters still take bare `f32`s named `_hz`,
`_ms` and `_db` (below) until they move to these types.

## The learned layer

[`learned`](stave_dsp::learned) runs networks trained out of tree (in
Python) as ordinary stages: [`WaveNet`](stave_dsp::learned::WaveNet),
[`Lstm`](stave_dsp::learned::Lstm) and [`Gru`](stave_dsp::learned::Gru),
f32, one sample per tick, no allocation after loading.
[`nam::parse`](stave_dsp::learned::nam::parse) reads a Neural Amp
Modeler `.nam` file's text into a [`Network`](stave_dsp::learned::nam::Network);
load it off the audio thread and hand it over.

A network knows the rate it was trained at. Prepared at another, it
outputs silence and [`rate_mismatch`](stave_dsp::learned::nam::Network::rate_mismatch)
says why: run the network at its rate (an
[`Oversampled2x`](stave_dsp::Oversampled2x) region when the host runs at
half of it), or choose [`RatePolicy::Any`](stave_dsp::learned::RatePolicy)
knowingly. `crates/stave-dsp/tests/nam_parity.rs` holds every
architecture to the trainer's own forward pass on files it exported
(`scripts/nam-reference.py`), within 1e-5, and to an independent f64
forward pass on random shapes.

## Fractional delays

A [`DelayLine`](stave_dsp::DelayLine) reads between its samples through
the interpolator named in its type:
[`Linear`](stave_dsp::primitives::time::interpolators::Linear) by default,
[`Lagrange3`](stave_dsp::primitives::time::interpolators::Lagrange3) and
[`Lagrange5`](stave_dsp::primitives::time::interpolators::Lagrange5),
the [`Thiran`](stave_dsp::primitives::time::interpolators::Thiran)
allpass (flat magnitude, one read per line) or a windowed
[`Sinc`](stave_dsp::primitives::time::interpolators::Sinc)`<N>`. Each
has a measured error budget for a sine's magnitude and phase in
`interpolators/tests.rs`. The
[`Delay`](stave_dsp::primitives::time::delay::Delay) stage is a line
written, read and advanced once per sample: the voice of a chorus,
flanger or vibrato.

```rust
use stave_dsp::primitives::time::delay::{Delay, DelayLine};
use stave_dsp::primitives::time::interpolators::{Lagrange3, Thiran};
use stave_dsp::Stage;

// A chorus voice: 10 ms through four-point Lagrange, gliding.
let mut voice = Delay::<Lagrange3>::new(20.0, 10.0);
voice.prepare(48_000.0);
let mut y = 0.0;
for n in 0..4_800 {
    voice.set_ms(10.0 + 2.0 * (n as f32 / 4_800.0));
    y = voice.tick(0.5);
}
// The weights sum to one: a constant reads back as itself.
assert!((y - 0.5).abs() < 1e-6);

// Storage with an allpass read: an impulse read 2.5 samples late
// arrives spread over its neighbours, its energy kept.
let mut line = DelayLine::<Thiran>::default();
line.allocate(64);
let mut out = Vec::new();
for n in 0..32 {
    line.write(if n == 0 { 1.0 } else { 0.0 });
    out.push(line.interpolated(2.5));
    line.advance();
}
let energy: f32 = out.iter().map(|y| y * y).sum();
assert!((energy - 1.0).abs() < 1e-4);
```

## Effects: reverbs and dynamics

The [`effects`](stave_dsp::effects) layer holds algorithmic designs
that model no hardware, held to analytic properties instead of a
device. They are reached by path.

[`effects::reverb`](stave_dsp::effects::reverb) has three reverbs:
the [`Fdn`](stave_dsp::effects::reverb::Fdn) (eight modulated lines
through a Householder or Hadamard [`Mixing`](stave_dsp::effects::reverb::Mixing)
matrix), the [`Dattorro`](stave_dsp::effects::reverb::Dattorro) plate
and [`Schroeder`](stave_dsp::effects::reverb::Schroeder) (with
[`Schroeder::moorer`](stave_dsp::effects::reverb::Schroeder::moorer)).
Each takes its decay as a T60 in seconds, is wet only, and is tested to
decay at the T60 it is set to, at every sample rate from 44.1 to 192 kHz:

```rust
use stave_dsp::effects::reverb::{Fdn, Mixing};
use stave_dsp::{Stage, Tail};

let mut room = Fdn::new(Mixing::Hadamard);
room.set_t60_s(1.2);
room.set_hf_ratio(0.5); // highs die twice as fast
room.prepare(48_000.0);
let (left, right) = room.tick_stereo(1.0);
assert!(left.is_finite() && right.is_finite());
// The tail is two T60s (−120 dB) plus the longest line.
let Tail::Samples(n) = room.tail() else { unreachable!() };
assert!(n > 2 * 57_600);
// An infinite T60 freezes the loop.
room.set_t60_s(f32::INFINITY);
assert_eq!(room.tail(), Tail::Infinite);
```

[`effects::dynamics`](stave_dsp::effects::dynamics) has the detectors
([`Peak`](stave_dsp::effects::dynamics::Peak),
windowed [`Rms`](stave_dsp::effects::dynamics::Rms),
[`ProgramDependent`](stave_dsp::effects::dynamics::ProgramDependent),
stereo [`Link`](stave_dsp::effects::dynamics::Link) modes and the
[`Sidechain`](stave_dsp::effects::dynamics::Sidechain) filter), the
[`GainComputer`](stave_dsp::effects::dynamics::GainComputer) static
curve, the [`Compressor`](stave_dsp::effects::dynamics::Compressor) in
feed-forward or feedback [`Topology`](stave_dsp::effects::dynamics::Topology),
and the look-ahead [`Limiter`](stave_dsp::effects::dynamics::Limiter),
which holds sample or true [`Peaks`](stave_dsp::effects::dynamics::Peaks)
under its ceiling and reports its latency exactly:

```rust
use stave_dsp::effects::dynamics::{Compressor, GainComputer, Limiter, Peaks, Topology};
use stave_dsp::Stage;

// 4:1 above −20 dB with a 6 dB knee: 0 dB in comes out at −15 dB.
let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
assert_eq!(curve.output_db(0.0), -15.0);
let mut comp = Compressor::new(curve, Topology::FeedForward);
comp.prepare(48_000.0);
let y = (0..48_000).fold(0.0, |_, _| comp.tick(1.0));
assert!((20.0 * y.log10() + 15.0).abs() < 0.01);

// A true-peak limiter at −1 dBTP: the latency is its lookahead plus the
// interpolator's look forward and hold, and an impulse under the
// ceiling arrives exactly that late.
let mut limiter = Limiter::new(Peaks::True, -1.0);
limiter.prepare(48_000.0);
assert_eq!(limiter.latency(), 16 + 71 + 16);
let out: Vec<f32> = (0..200).map(|n| limiter.tick(if n == 0 { 0.5 } else { 0.0 })).collect();
assert!((out[limiter.latency() as usize] - 0.5).abs() < 1e-5);
```

## Conventions

- **Units in names.** `sample_rate` is `f32` everywhere (state may be
  `f64`); frequencies end `_hz`, times `_ms`, levels `_db`; filters take
  `q` (Butterworth is 1/√2), never "resonance"; phase is in cycles,
  `0 ≤ t < 1`. One [`Waveform`](stave_dsp::Waveform) (sine, triangle,
  saw, square) serves oscillators and LFOs.
- **Time constants come from the rate.** Every smoother, envelope and
  lag uses [`tau_coeff`](stave_dsp::tau_coeff)`(seconds, sample_rate)`
  = `1 − e^(−1/(seconds · sample_rate))`, never a bare per-sample
  coefficient.
- **Capacity is a type.** Fixed-capacity pushes
  ([`Chain::push`](stave_dsp::Chain::push),
  [`Router::push_route`](stave_dsp::Router::push_route)) return
  `Result<_, Full>`, never a sentinel.
- **Preconditions are asserted, not repaired.** An input outside a
  method's domain (a `q` at or below zero, a crossover whose low corner
  is above its high one) is a caller bug caught by `debug_assert!`, not
  a silent clamp.
- **Configuration is data.** Multi-value setup takes a struct with
  named fields ([`FilterSpec`](stave_dsp::FilterSpec),
  [`AdsrConfig`](stave_dsp::AdsrConfig),
  [`RcClipperConfig`](stave_dsp::RcClipperConfig),
  [`PowerSupplyConfig`](stave_dsp::PowerSupplyConfig)); variants are
  enums ([`AdaaOrder`](stave_dsp::AdaaOrder),
  [`Bias`](stave_dsp::Bias), [`LevelPolicy`](stave_dsp::LevelPolicy)).
- **f64 where it accumulates.** Accumulators, coefficients, phase and
  anything persistent are `f64`; audio-rate interfaces are `f32`.

```rust
use stave_dsp::{amp_to_db, db_to_amp, tau_coeff, Tail};

assert_eq!(db_to_amp(0.0), 1.0);
assert!((db_to_amp(-6.0) - 0.501).abs() < 1e-3);
assert_eq!(amp_to_db(0.0), -180.0); // the floor of f32 audio
// A pole at radius 0.5 falls 120 dB in 20 samples.
assert_eq!(Tail::from_pole(0.5), Tail::Samples(20));
// After one time constant a step has covered 1 − 1/e of the way.
let c = tau_coeff(0.001, 48_000.0);
let y = (0..48).fold(0.0_f32, |y, _| y + c * (1.0 - y));
assert!((y - (1.0 - (-1.0_f32).exp())).abs() < 1e-3);
```

Every primitive's price is measured: `crates/stave-dsp/tests/datasheet.rs`
prints its cost in nanoseconds per sample (it skips in debug builds; run
`cargo test --release -p stave-dsp --test datasheet -- --nocapture`).
