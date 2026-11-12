# TOLEX corpus laws — what 1,814 real IRs say

**Measured 2026-07-20** over the York Audio reference population
(12 packs = **15 speakers**, 48 kHz natural-phase singles: **1,696
close-mic + 118 ambient captures, 0 unparsed**; instruments:
`plugins/tolex/analysis/{naming,analyze_corpus,aggregate}.py`,
reference WAVs untracked in `plugins/tolex/refs/`). These are the
NUMBERS behind M2–M4's design decisions (plugins/tolex/docs/design.md). Values are
p10/median/p90 unless stated. Identity stats group by SPEAKER (the
dual-speaker packs — MV30a/b, ESD H/M, M25/SH — count separately);
ambient captures are excluded from every close-mic law and reported
in §8.

## 1. Identity is modal — the M2 architecture is confirmed

Same speaker, different mic position: **69%** of prominent spectral
peaks (≥1.5 dB, 500–8000 Hz) persist within 1/24 octave
(0.50/0.69/0.84); same mic, moved: 73%. Chance baseline across
different speakers: 43%. Fine-structure residual correlation: 0.65
same-speaker vs 0.17 cross-speaker. The MIC barely moves the peaks;
the POSITION does, and only partially.

**Design consequence**: a speaker is a FIXED modal set (frequencies
and Qs are its fingerprint); mic position modulates mode WEIGHTS
and adds path-difference interference on top. The ~1/3 of peaks
that do move with position are the interference combs — delays, not
resonators. M2 builds exactly this: per-speaker mode table,
position-dependent radiation weights, geometric combs.

## 2. The ring law — decay time vs frequency

Per-1/3-octave Schroeder decay (T20→T60-style, ms), WITH the
instrument's own floor (a 1/3-octave filter rings by itself —
bandwidth and decay are Fourier-dual; the floor is the same
measurement run on a delta):

| band | med | floor |  | band | med | floor |
|---|---|---|---|---|---|---|
| ≤800 Hz | — | — | | 2.5 k | 24 | 7 |
| 1 k | **75** | 18 | | 4 k | 19 | 5 |
| 1.26 k | **77** | 15 | | 6.3 k | 20 | 3 |
| 1.6 k | 35 | 12 | | 10 k | 10 | 2 |

**Every band at or below 800 Hz is FLOOR-LIMITED** (median < 2×
floor) — the first read's LF rows measured the analysis filter, not
the speakers, and are withdrawn. LF ring targets come from T/S
alignment physics (Qtc), which the motor already implements exactly.
The trusted finding stands and strengthens: **1–1.3 kHz rings
4–5× above the floor and ~2× its neighbors** (the cone/surround
transition — the depth band), with a secondary 5–6.3 kHz breakup
ring. Modal Q from trusted bands: ≈ 35 at 1 kHz, ≈ 25 at 2.5 kHz,
≈ 36 at 4 kHz. **M2's mode Qs are drawn from the trusted rows of
T(f), never uniform** — uniform Q is the fingerprint of a fake.

## 3. The mode budget

Prominent modes (≥1.5 dB) per octave, 500–8000 Hz: 3/6/4/6 median
per rising octave (13–27 total, p90 27). Prominence 1.9/4.7/12 dB.
Spectral-width Q 12–20 median, p90 up to 60 (width-limited by
smoothing; the decay-based Qs in §2 are the truth for ring). A
32–48 mode bank with prominence-shaped gains covers the population
with headroom.

## 4. Cepstral structure is cone-scale, then box-scale

(Re-issued: the first read's 0.3 ms search bound truncated the
distribution — 98% of the dominant lags sit below it.) With the
window opened to 0.05–6 ms: strongest lag per IR 0.06/0.19/0.23 ms
(2–8 cm — dust-cap / cone-cavity geometry); of all detected lags,
56% fall in 0.1–0.3 ms, 33% in 0.3–1.0 ms (baffle/box scale), and
essentially none beyond 1 ms. Sub-0.1 ms lags ride the cepstrum's
spectral-envelope boundary — indicative, not literal geometry.
Read: the dominant comb structure is the CONE's own (the modal/
radiation model produces it naturally); discrete box reflections
are a real but secondary layer. M3 budget: 2–3 taps in 0.3–1.0 ms;
nothing at room scale (that lives in §8's ambient regime).

## 5. Real cab IRs are minimum-phase

Excess group delay (vs the minimum-phase construction, bulk delay
removed, 200–5000 Hz): RMS 0.03/**0.04**/1.05 ms — essentially
zero. (The non-min-phase tail that inflated the first read was the
ambient captures, since separated out: stereo room pairs run ~20 ms
excess GD — see §8.)
The modal-bank + short-delay architecture is min-phase by
construction, which is exactly right; no phase machinery needed.

## 6. Energy timeline

Direct (<0.5 ms) 74%, early (0.5–5 ms) 22%, late (>5 ms) 4% of
energy; t50 0.12 ms, t90 1.1/2.1/4.2 ms. Depth is not reverb — it
is the 22% early window (interference + mode onset) plus band-wise
ring per §2.

## 7. Position macro calibration (M2's mic-position knob)

Cap center → cone edge is a **−4.6 dB tilt swing** (300–600 Hz vs
2.5–4 kHz: CNT +0.85 → CNE −3.77 dB) with the −12 dB corner moving
6.79 → 5.79 kHz. Off-axis sits between (−0.50 dB, 6.21 kHz). The
position knob's macro path is calibrated to this; the micro path is
§1's weights + combs.

**Negative result, worth its keep**: the numbered-position packs
(P1–P5, 831 IRs) are NOT an ordinal cap→edge sweep — median tilt is
flat across rungs (−1.0 … −1.8 dB, non-monotonic) and within-rung
spread dwarfs between-rung differences. They are curated VARIETY
spots. Only the letter classes carry the cap→edge axis; ordinal
assumptions about "position 1..5" would have silently miscalibrated
the knob.

## 8. Ambient captures — the M4 room/rear targets

118 captures, their own population (never pooled with close mics):

| kind | n | t90 ms | late frac | excess GD ms | tilt dB |
|---|---|---|---|---|---|
| ROOM (mono) | 48 | 7.3/13.6/30.7 | 0.15/0.27/0.48 | 0.5/2.2/24.9 | −5.3 |
| REAR | 28 | 3.9/20.7/27.8 | 0.07/0.37/0.62 | 0.3/2.2/12.8 | −10.6 |
| ROOM pair L/R | 24 | 16/25/40 | 0.36/0.49/0.60 | 11.5/19.9/28.5 | −6.6 |
| SIDE | 14 | 8.8/15.6/23.3 | 0.18/0.31/0.36 | 1.1/1.6/2.6 | −7.1 |

Room mics are a DIFFERENT physics regime: 27–49% late energy,
strongly dark (−5 to −11 dB tilt), and the stereo pairs are far
from minimum phase (~20 ms excess GD — real reflections, not modal
ring). M4's room voice cannot be an EQ'd close mic; it needs its
own sparse-reflection stage. REAR is the darkest and most
late-heavy — consistent with through-the-back radiation.

## 10. The config layer — complexity rises with the cabinet

(Added 2026-07-21.) Grouping close mics by configuration:

| cfg | n | extrema/oct | deep notches | t90 ms | resid rms dB |
|---|---|---|---|---|---|
| 1×10 | 70 | 4.2 | 6 | 1.0 | — |
| 2×10 | 108 | 4.8 | 7 | 1.3 | — |
| 2×12 | 574 | 5.5 | 10 | 1.6 | 2.83 |
| 4×12 | 944 | 6.1 | 11 | 2.5 | 3.27 |

Monotonic in every dimension (412 vs 212: p < 0.001 on notch count
and residual rms). CAVEAT, stated plainly: config is CONFOUNDED
with speaker model — no pack captures one driver in two configs —
so cross-config deltas bound the effect but cannot isolate it. The
model's config law (tests/config_properties.rs) therefore asserts STRICT
ordering only for the mechanism the model owns cleanly (shared-
interior cross-talk → residual rms), parity-tolerance on extrema,
and no-collapse on the box-band tail, where two real mechanisms
oppose (alignment ring shrinks with box size while box-mode tail
grows; a same-driver cross-config capture would arbitrate — worth
commissioning).

## Amendments

Same rule as everywhere: dated amendments, never silent edits.
Re-run the instruments after adding reference packs.

## 9. Instrument validity — how these numbers are kept honest

Checks run against the instruments themselves (2026-07-20):

- **Decay floor**: every T20 band carries the delta-impulse floor of
  its own filter; bands with median < 2× floor are marked
  FLOOR-LIMITED and never used as targets (§2 re-issued after this
  check withdrew the LF rows).
- **Tail integrity**: reference tails are clean recorded noise at
  ≈ −110 dB re peak, no digital truncation or fades (n=150 sampled)
  — Schroeder integrals are valid.
- **Search-window truncation**: the cepstral window was widened
  after the first read's p10 landed exactly on the search bound
  (§4 re-issued).
- **The same-instrument rule**: tolex's rendered IR is exported by
  the fidelity test (`tolex_ir.f32`) and measured by THIS pipeline
  (aggregate §9) — model-vs-population rows never mix instruments.
  The Rust fidelity gate keeps its own thresholds for fast in-repo
  regression only; the Python pipeline is the instrument of record
  for corpus-law comparisons.
- **Known limits, open**: single-vendor corpus (one shooting
  methodology; cross-vendor pack wanted before treating macro stats
  as universal); metrics are physical, not perceptually weighted
  (no ERB/loudness weighting yet); marginal distributions only (no
  joint-covariance check yet); linear snapshots only — the
  nonlinear "living leg" needs multi-level captures, which no IR
  pack can provide. Each lifts when its milestone needs it.

## 10. The config layer — complexity rises with the cabinet

(Added 2026-07-21.) Grouping close mics by configuration:

| cfg | n | extrema/oct | deep notches | t90 ms | resid rms dB |
|---|---|---|---|---|---|
| 1×10 | 70 | 4.2 | 6 | 1.0 | — |
| 2×10 | 108 | 4.8 | 7 | 1.3 | — |
| 2×12 | 574 | 5.5 | 10 | 1.6 | 2.83 |
| 4×12 | 944 | 6.1 | 11 | 2.5 | 3.27 |

Monotonic in every dimension (412 vs 212: p < 0.001 on notch count
and residual rms). CAVEAT, stated plainly: config is CONFOUNDED
with speaker model — no pack captures one driver in two configs —
so cross-config deltas bound the effect but cannot isolate it. The
model's config law (tests/config_properties.rs) therefore asserts STRICT
ordering only for the mechanism the model owns cleanly (shared-
interior cross-talk → residual rms), parity-tolerance on extrema,
and no-collapse on the box-band tail, where two real mechanisms
oppose (alignment ring shrinks with box size while box-mode tail
grows; a same-driver cross-config capture would arbitrate — worth
commissioning).

## Amendments (continued)

**2026-07-20 (same day)**: parser rewritten (`naming.py`) to cover
the FULL corpus — first read parsed 834/1,814 filenames and pooled
ambient captures into close-mic stats. Now: 1,696 close + 118
ambient, 0 unparsed, identity grouped by speaker (15, not 12 packs).
All numbers re-issued; the identity signal STRENGTHENED (match 67→69%,
correlation 0.58→0.65), the ring law and reflections barely moved
(they were robust), excess-GD p90 cleaned up (the tail was room
pairs), and §7's negative result + §8 are new.

**2026-07-20 (same day, validity pass)**: instrument audit (§9)
withdrew the floor-limited LF decay rows (§2 re-issued — the
1–1.3 kHz depth band SURVIVES at 4–5× floor), re-issued the
cepstral read with the widened window (§4), and instituted the
same-instrument rule. tolex M1 through the instrument of record:
0 modes/oct (pop 3.6/4.8/5.8 at this instrument's 1.5 dB
prominence), decay @1 kHz 17.7 ms ≈ the 18.2 ms floor (i.e. NO
ring; pop median 75 ms), excess GD 0.002 ms (min-phase, matching
the population's 0.04).
