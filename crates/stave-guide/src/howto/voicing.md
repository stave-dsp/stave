# Voicing and listening tests

Most stave models are tuned by listening: there is rarely a hardware
unit to capture, so the ear is the reference. `stave-play`'s **Voicing**
tab makes that a structured activity. It compares candidates matched in
loudness, runs blind ABX and rating trials, nulls candidates against a
reference model or a capture, logs every session, and
pins a chosen sound so that a later change to the model shows up
instead of drifting silently. This page covers starting a session, each
mode, the statistics behind the verdicts, and where the results go.

## Start a session

Open the **Voicing** tab in `stave-play`, or start there from the
command line with two or more `--compare` candidates:

```sh
# Two catalog stages against the dry signal, on the built-in guitar riff.
cargo run --release -p stave-play -- \
  --compare 'dry;ref' \
  --compare 'stage:clipper/diode' \
  --compare 'stage:circuit/diode_clipper' \
  --material 'guitar riff'

# Two settings of one plugin, driven 6 dB hotter, with a 60-trial ABX plan.
stave-play --compare 'build/StaveTilt.vst3;Tilt=3;label=bright' \
           --compare 'build/StaveTilt.vst3;Tilt=-3;label=dark' \
           --material ~/takes/di-guitar.wav --trim 6 --abx-trials 60

# What material is built in (and write it out as WAVs for a DAW).
stave-play --list-material --export-material /tmp/stave-material
```

A candidate is one string: `dry` (the material unprocessed, which is the
bypass anchor), a `.vst3` bundle, an Audio Unit (`au:NAME` or its
`.component`, macOS: a third-party contender, see [Benchmark against
another plugin](crate::howto::host_tools)), `stage:NAME`, `wav:FILE` (a recording
aligned to the material, such as a capture) or `blessed:RECORD` (a sound
blessed earlier). Pieces after `;` are param settings in the param's own
units (as `--set` takes them), `preset=NAME`, `label=TEXT`, or `ref`,
which marks the session's reference. Quote the string, because `;` is
special to the shell.

The Voicing tab can build the same setup by hand. It can add a plugin or
stage from the list, use **current settings** (the plugin loaded in
`stave-play`, with every param as it is set now), add dry, a recording
or a blessed take, and edit each candidate's preset and params with
sliders. Up to nine candidates fit, one per digit key.

## Program material

- **Built-in.** Synthesised and deterministic, so a session log can name
  the material by checksum. The set has plucked-string guitar (a
  single-note riff, palm-muted and ringing power chords, strummed open
  chords), extended keys chords, a plucked bass line, a drum loop, a
  band mix, a 220 Hz tone stepping from −36 to 0 dBFS (where does it
  break up?), white and pink noise bursts from 5 to 500 ms, and a
  20 Hz–20 kHz sweep. Every item peaks at −6 dBFS.
- **Any WAV.** It is converted to the session rate if it differs.
- **The sample library.** If `STAVE_SAMPLE_LIBRARY` is set (in the
  environment or the [local configuration](crate::howto::local_config)),
  **Scan library** lists the WAVs under its `Test Corpus/`. If the drive
  is not mounted, the list is empty and says so.
- **A corpus role.** `--material role:NAME` (or `role:NAME#N` for item N)
  plays an item of a reference-corpus role — `guitar_di`, `bass_di`,
  `drums_loop`, `mix_bus`, `vocal`, `piano`, `synth_pad`, `transients` and
  the rest in `crates/stave-test/corpus.spec` — picked from the library
  by the committed rules and, when `scripts/corpus.sh lock` has run, by
  the files and checksums the lock names. `--list-material` lists the
  roles and what this machine's library fills them with; the session log
  records the role, the item and every file's checksum.

**Input trim** sets the level the candidates are driven at, which
matters for every nonlinear model. Every take is loudness-matched
afterwards either way.

## Prepare: takes, latency, loudness

**Prepare** runs every candidate offline over the material, on a worker
thread with a progress bar, and makes one take per candidate:

1. **Settings, then a pre-roll.** The preset and settings are applied,
   then half a second of silence runs through the candidate, so param
   smoothers and DC blockers have settled before the first note.
2. **Latency compensation.** Each candidate runs `latency()` frames past
   the end of the material, and those frames are dropped from the front
   of its take, so every take lines up with the dry material sample for
   sample. A cross-correlation against the dry input then measures any
   remaining offset. A plugin that misreports its latency is flagged,
   because a misaligned take comb-filters the switch and gives itself
   away.
3. **Loudness match.** Louder reads as better, so every take is matched
   in BS.1770 integrated loudness to within **0.1 LU**, the tolerance of
   EBU Tech 3341. Each take is turned down to the quietest one, then all
   are turned down together if any would peak above −1 dBFS. The match
   is re-measured until each take is within 0.01 LU. The gains apply at
   playback; the takes themselves stay as the candidates produced them.
   **Takes and loudness** shows each take's raw loudness, gain, matched
   loudness, peak and latency.

A candidate that is silent on the material is an error, never a silent
comparison. So is an instrument (it has no audio input), a misspelled
param, or a blessed take recorded on other material or at another trim.

The measurement is [`stave_measure`]'s BS.1770 meter, the same one the
benches use:

```rust
use stave_measure::loudness::{LevelMeasure, Lufs};

let rate = 48_000.0;
let tone: Vec<f32> = (0..(10.0 * rate) as usize)
    .map(|i| 0.1 * (std::f32::consts::TAU * 997.0 * i as f32 / rate as f32).sin())
    .collect();
let lufs = Lufs::integrated().measure(&[&tone, &tone], rate).unwrap();
// A 997 Hz sine at −20 dBFS reads −23 LUFS in one channel (BS.1770's
// calibration); two channels add 3 dB.
assert!((lufs - (-20.0)).abs() < 0.1);
```

## Listening

Playback loops the material, or a region of it. Click the timeline to
seek and drag across it to loop a region; double-click it to loop the
whole material again. `[` and `]` set the loop ends at the playhead.
Switching between takes is instant and click-free. Every take shares
one playhead and the switch is a 10 ms equal-gain crossfade between
time-aligned takes, so even a polarity flip does not click. The loop
seam has its own crossfade. **Restart loop on switch** jumps back to
the loop start on every switch, for listeners who compare the same
phrase each time.

| Key | Sighted A/B | Blind ABX | Rating |
|---|---|---|---|
| space | play / stop | play / stop | play / stop |
| 1–9 | select a candidate | A, B, X | play a slot |
| A / B / X | candidate 1 / 2 | listen to A / B / X | |
| ⇧A / ⇧B, ← / → | | answer "X is A" / "X is B" | |
| ⏎ or N | | next trial | next trial |
| R | | | the open reference |
| ↑ / ↓ (⇧ for 10) | | | move the playing slot's slider |
| `[` `]` ⌫ | loop start, end, whole | the same | the same |

### Sighted A/B

You know which candidate is playing. Use this mode to find a sound and
to note a preference (**Prefer**, with notes). Sighted judgements are
dominated by expectation (Toole & Olive 1994), so confirm a choice
blind before you trust it.

### Blind ABX

Pick A and B, then **Start ABX**. In each trial X is A or B at random
(a seeded draw, logged so the run can be replayed). Listen to A, B and
X as often as you like, answer, and press ⏎ for the next trial. The
default is **40 trials**, because shorter runs mostly fail to detect
real differences. The running score (which can be hidden, since
watching it is feedback) shows the correct count, the p-value, d′ and
the hit rate's 95 % interval. The run's power is shown before you
start.

The statistics live in [`stave_measure::listening`] and are tested
against the published figures:

```rust
use stave_measure::listening::{Abx, AbxVerdict, ALPHA, ASSUMED_HIT_RATE};

// Leventhal 1986: at α = 0.05 a listener whose true hit rate is 0.7
// passes a 16-trial run only 45 % of the time, and a 40-trial run 81 %.
assert!((Abx::power(16, ALPHA, ASSUMED_HIT_RATE) - 0.45).abs() < 0.01);
assert!((Abx::power(40, ALPHA, ASSUMED_HIT_RATE) - 0.81).abs() < 0.01);
// The critical scores: 12 of 16, 26 of 40.
assert_eq!(Abx::critical(16, ALPHA), Some(12));
assert_eq!(Abx::critical(40, ALPHA), Some(26));

// 12/16 is significant, and d′ reports the sensitivity apart from bias.
let run = Abx { trials: 16, correct: 12 };
assert!(run.p_value() < ALPHA);
assert!(run.d_prime() > 1.0);

// A non-significant short run is inconclusive, not "inaudible".
let short = Abx { trials: 16, correct: 10 };
assert!(matches!(short.verdict(16), AbxVerdict::Inconclusive { .. }));
assert!(short.verdict(16).describe().contains("Not evidence of inaudibility"));
```

There are four verdicts. **Detected** means p < 0.05. **Inconclusive**
means not significant with less than 80 % power. **No difference
detected** means not significant with adequate power, and it states
that smaller differences are not excluded. **Incomplete** means the run
stopped early. No verdict ever says "inaudible". To accumulate
evidence, pool runs of the same pair across sessions
([`Abx::pool`](stave_measure::listening::Abx::pool)) instead of judging
each one (Reiss 2016).

### Rating (MUSHRA-style)

**Start rating** puts every stimulus on one screen in a fresh random
order each trial, numbered by slot. Each one is rated 0–100 on the
BS.1534-3 scale (bad, poor, fair, good, excellent). The stimuli are:

- every candidate;
- the **hidden reference**, if one is marked `ref`: the reference again,
  unlabelled, which a careful listener rates 100;
- the **low anchor**: the reference (or the first candidate) low-passed
  at 3.5 kHz, as BS.1534-3 requires, to counter range-equalising bias.

The open reference plays on R. When the trials are done, the results
are revealed: each stimulus's mean with a t-based 95 % interval, and
its median. Rating data are bounded and skewed, so the median is the
robust reading. A run whose hidden reference scored below 90 on more
than 15 % of trials is flagged, because BS.1534-3's post-screening
would exclude it:

```rust
use stave_measure::listening::{passes_screening, RatingSummary};

let ratings = [72.0, 80.0, 65.0, 77.0];
let s = RatingSummary::of(&ratings).unwrap();
assert!((s.mean - 73.5).abs() < 1e-9 && s.ci95.unwrap() > 0.0);
assert!(passes_screening(&[100.0, 95.0, 100.0]));
assert!(!passes_screening(&[100.0, 70.0, 80.0]));
```

Without a reference, the run is a preference rating with the anchor
alone. For a near-threshold question ("did this refactor change the
sound at all?"), run ABX first; when it says *detected*, a rating with a
hidden reference measures how much (the BS.1116 question).

### Null test

The **Null test** mode subtracts. It lines each candidate up with the
reference (the `ref` row, or the first candidate when none is marked)
in level and latency, then subtracts the two. What is left is the
residual, the part of the difference that a level match and a time shift
cannot remove. The reference can be another model or a capture: a
`wav:FILE` of the hardware played over the same material. For each
candidate the panel shows:

- **aligned**: the gain and delay that line it up with the reference.
  The gain is between the takes as produced, before the loudness match.
  The delay is in samples and µs, up to ±50 ms, so a capture through an
  interface's round trip still lines up.
- **null NMR**: the residual's noise-to-mask ratio under the reference,
  in total and in the worst band (DSP.md R16), from the worse of the
  two channels. At −10 dB or below on both it reads **inaudible**,
  otherwise **AUDIBLE**. The verdict is the ear model's prediction, not
  a listening result.
- **residual**: the residual's level against the reference.
- **listen**: A, B, **A − B** (the residual at its true level, as it sits
  under the reference) and **A − B raised** (the residual brought up to
  the session's loudness, to hear what it is made of; its level means
  nothing). The residual plays in the loop like any take, with the same
  click-free switching.
- **ABX**: opens Blind ABX set to this pair. Confirm the prediction with
  your ears; it is not the same question. A null can be audible on
  paper while no listener picks it out, and a difference the ear model
  calls inaudible can still be found.

The keys are those of sighted A/B.

Every null is written to the session log (`nulls`). Headless, the
command below prepares both candidates the same way and prints the
verdict, and `--residual` writes the residual (as heard) to a WAV. The
exit status is 0 for inaudible, 1 for audible and 2 for an error, so a
script can gate on it:

```sh
# A stage against a capture of the unit made from the same DI take.
stave-play --null 'stage:circuit/ts808' 'wav:~/captures/ts808.wav' \
           --material ~/takes/di-guitar.wav --residual /tmp/ts808-residual.wav

# Two models on a corpus role's first item (DSP.md R22).
stave-play --null 'stage:circuit/diode_clipper' 'stage:clipper/diode' --material role:vocal
```

The measurement underneath is
[`null::with_residual`](stave_measure::perceptual::null::with_residual).
For stereo takes, the alignment is fitted once on the mid channel with
[`null::align`](stave_measure::perceptual::null::align) and applied to
each channel with [`null::apply`](stave_measure::perceptual::null::apply):

```rust
use stave_measure::perceptual::null::{align, apply, with_residual};
use stave_measure::perceptual::Playback;

let rate = 48_000.0;
let env = |i: usize| (std::f64::consts::PI * i as f64 / 24_000.0).sin().powi(2);
let take = |i: usize, late: f64| {
    let t = (i as f64 - late) / rate;
    env(i) * (0.3 * (std::f64::consts::TAU * 220.0 * t).sin() + 0.1 * (std::f64::consts::TAU * 1_330.0 * t).sin())
};
let reference: Vec<f64> = (0..24_000).map(|i| take(i, 0.0)).collect();
// The candidate: 2 dB hot and 12 samples late.
let candidate: Vec<f64> = (0..24_000).map(|i| take(i, 12.0) * 10f64.powf(2.0 / 20.0)).collect();
let n = with_residual(&reference, &candidate, rate, Playback::default(), 0.05);
assert!((n.null.alignment.gain_db() + 2.0).abs() < 0.01);
assert!((n.null.alignment.delay * rate + 12.0).abs() < 0.05);
assert!(n.null.inaudible(), "{:?}", n.null.nmr);
// One alignment for several channels.
let a = align(&reference, &candidate, rate, 0.05);
let again = apply(&reference, &candidate, a, rate, Playback::default());
assert_eq!(again.residual.len(), reference.len());
```

## The session log

Each session writes `<id>.json` and `<id>.txt` to
`<voicing dir>/sessions/`. By default the voicing directory is
`stave-play/voicing` under the user's local data directory
(`~/Library/Application Support` on macOS, `$XDG_DATA_HOME` on Linux).
`--voicing-dir DIR`, the environment variable `STAVE_VOICING_DIR`, or
**Change…** in the tab move it. The log is never written into the
repository. It is rewritten after every trial, preference and bless, so
a crash loses at most the trial in hand.

The JSON holds the session's settings (rate, block, crossfade, loop),
the material (its name, source, and SHA-256 of the samples as played,
plus the file's own SHA-256 for a file), and every candidate with its
spec, the params it ran with, its latency, any warnings and its take's
SHA-256. It also holds the full loudness match and every ABX trial (what
X was, the answer, how often A, B and X were played, and the time) with
the run's p, power, critical score, d′, interval and verdict. Rating
trials appear with their order and ratings and the summary. Preferences,
blesses and notes complete it. The `.txt` file is the same session in a
few lines.

## Bless this sound

**Bless** (in sighted mode, or *Bless top-rated* after a rating run)
pins a candidate. It writes `<voicing dir>/blessed/<label>-<time>.json`,
which records the candidate's spec and resolved params, the material
and trim, the rate and block, and the session. Beside it goes the take
as a 32-bit float WAV, before loudness matching. Two things read the
record:

- **A later session** compares against it as a `blessed:` candidate,
  which is the earlier version's sound on the same material. It refuses
  other material or another trim.
- **`stave-play --verify-blessed`** runs each blessed candidate again
  with the current build and nulls it against its take:

  ```sh
  stave-play --verify-blessed ~/Library/Application\ Support/stave-play/voicing/blessed
  # same     stage:clipper/diode  bit-identical  (…/stage-clipper-diode-20261113T081500Z.json)
  # CHANGED  diode hot  residual -41.3 dB  (…)
  ```

  It exits 1 if any residual is above `--tolerance-db` (default −120 dB
  relative to the take, which passes numerical noise between machines
  and fails any real change).

A blessed record is plain JSON with no machine-specific content beyond
the material's path. To make a blessed sound part of review, commit the
record and its WAV next to the model or plugin and run
`--verify-blessed` on it. Blessing a plugin or stage candidate also
writes its fingerprint (DSP.md R18) into the record's `fingerprint`
field: the candidate's measured character as a black box, which
`--verify-blessed` measures again, naming every characteristic that
moved ([Fingerprints and perceptual metrics](crate::howto::fingerprints)).

## A session that holds up

- Keep it blind: settle a choice in ABX or rating, not in sighted mode.
  The analysis tabs (Spectrum, Meters…) show the output, so stay on the
  Voicing tab during blind trials.
- Plan the trial count before you start, and don't stop when the score
  looks good: stopping early inflates the false-positive rate. The log
  records how many trials were planned.
- Mark a reference and keep the anchor in rating runs.
- Loop short phrases (BS.1534-3 suggests items of about 12 s at most),
  and change the loop between runs so the result is not about one note.
- Prefer several shorter sessions, pooled, to one long tiring one.
