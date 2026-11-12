# TOLEX — the cabinet that moves

**Status: RATIFIED 2026-07-20. M1 SHIPPED same day (the motor, four laws). M2 SHIPPED same day (THE VOICE + THE PICTURE). M3 SHIPPED 2026-07-21 (THE BOX: interior taps, axial air modes, panel materials) — **the linear scoreboard is CLOSED: 8/8 fidelity rows inside the York population.** M4a THE MICS SHIPPED 2026-07-21: seven models distilled FROM THE CORPUS (median vs SM57 over matched groups), law-pinned. M4b THE CHORUS SHIPPED 2026-07-21 (multi-speaker geometry, id 14, law-pinned combs + close-mic collapse). Next per impact order: M6 THE ANCHORS (the fitting campaign), then M7 THE GRIND.**

A guitar cabinet that is BUILT, not sampled: speaker motor, cone,
box, and air as a physical model, played in real time. The name is
the covering; the product is the cabinet under it.

## The thesis

A commercial IR (Fractal, OwnHammer, Two notes, York) is the
complete linear response of speaker+cab+mic — at ONE drive level,
ONE position, frozen. Nothing beats an IR *at being linear*; the
bar for the linear snapshot is therefore MATCH, measured, not
argued. But a real cabinet at volume is not linear and not frozen:
the amp's source impedance shapes the response through the
speaker's own impedance curve, excursion bends Bl and the
suspension, the voice coil heats and sags, breakup softens with
drive, and the mic hears a different cabinet at every position.
IRs render none of that. Tolex wins there, or it doesn't ship.

So the quality claim has two legs, each with teeth:

1. **The linear leg** — tolex's responses live INSIDE the measured
   population of real cabinets (the corpus laws + fidelity report:
   CLOSED 2026-07-21, 8/8 rows). Population statistics, not any one
   curve, are the target — see "the goal", below.
2. **The living leg** — behaviors no IR can render, each pinned by
   its own law: impedance-coupled resonance, excursion compression
   and its harmonic signature, drive-softened breakup, thermal sag,
   continuous mic/position morphing.

## The goal — better-than-real (ratified 2026-07-21)

The product is NOT "replicate one specific cab." The product is:
**build any cabinet you want, and let it sound better than a real
one could.** Two principles carry that:

- **The realism envelope is a floor, not a target.** The corpus
  laws exist so that INVENTED cabinets still register as physically
  real — right tooth density, right ring law, right geometry
  prints. Distribution-scoring (never curve-matching) was chosen
  for exactly this: it validates cabs that never existed.
- **Physics where it creates realism; freedom where physics is a
  compromise.** Real drivers live under coupled tradeoffs (mass
  ties the cliff to sensitivity; bass extension fights box size;
  louder breakup means harsher breakup; more excursion means more
  sag). Tolex keeps each mechanism REAL but lets the couplings be
  broken selectively: a heavy cone's smooth top over a light cone's
  punch, a small box's tightness with a big box's bloom, breakup
  that softens musically instead of tearing. Better-than-real means
  every element behaves like physics, combined in ways physics
  never offered.

Consequence for M6: fitting named reference speakers is now
CALIBRATION, not the product — a few anchor points (Greenback-class,
V30-class…) fitted into the physical space so the space between and
beyond them is meaningfully labeled. The user designs in a space
where the landmarks are real; the destinations don't have to be.

## Doctrine

- **Time-domain only.** No convolution, no FFT in the audio path —
  the smear lesson stands (spectral resynthesis can't be musical;
  resonators can). The whole cabinet is ODEs, modal banks, and
  geometric paths. Convolution exists ONLY as test-side measurement
  (offline, in this project's tests — the engine vocabulary gains
  nothing).
- **Physics first, fitting second.** The skeleton is real physics
  (lumped nonlinear electro-mechanics, modal cone dynamics, path
  geometry) so every knob is a physical quantity. Fine detail that
  physics can't predict in realtime (the exact modal fingerprint of
  a Greenback) is FITTED from measured IRs into the physical
  parameter space — so "design your own speaker" means moving
  through cone mass and edge damping, never abstract EQ.
- **Measure first, then code to the measurement** (the tilt lesson,
  at cabinet scale). Every milestone lands with laws that state its
  physics as a number.
- **Not an IR player.** No IR loader feature, ever — that market is
  saturated and it isn't the thesis.

## The model — four subsystems

**THE MOTOR** (M1) — the lumped electro-mechanical core, one leaf
cell in the NonlinearRc/triode tradition. Three states: coil
current `i`, cone velocity `v`, displacement `x`.

```
(Le)·di/dt = e − (Re + Rsrc)·i − Bl(x)·v      electrical mesh
 Mms·dv/dt = Bl(x)·i − Rms·v − x/Cms(x) − k_box·x   mechanical
   dx/dt = v
```

- Driven through **Rsrc**, the amp's source impedance — the damping
  knob. Solid state ≈ 0 Ω grips the cone; a no-feedback tube output
  ≈ tens of Ω lets the speaker's impedance curve print itself on
  the response (resonance bloom, rising top). This coupling is a
  headline feature; IRs cannot represent it at all.
- **Bl(x)** droops and **Cms(x)** stiffens with excursion — the
  large-signal (Klippel-tradition) nonlinearities. They are cheap
  (coefficient modulation on the same solve) and they ARE the
  "pushing air" sound: excursion compression + low-frequency H2/H3.
- T/S parameters derive from the speaker knobs (size → Sd and
  scale defaults; resonance → fs; q → Qts with house Qms; mass →
  Mms), then Cms = 1/(Mms·ω₀²), Bl = √(ω₀·Mms·Re/Qes). Consistent
  by construction — no free constants to drift.
- On-axis far-field pressure ∝ cone acceleration (taken from the
  solve's own derivative, not differenced).
- Trapezoidal, semi-implicit (coefficients frozen at the step's
  start state), f64 states, per-sample 3×3 solve by elimination.
  Base rate first; the alias conformance report decides whether the
  core earns a Oversampled2x (measured, not assumed).

**THE VOICE** (M2) — cone breakup as a modal resonator bank.
**The corpus laws are measured** (plugins/tolex/docs/corpus.md, 1,814 York
IRs): identity IS modal (67% of peaks persist across mic positions
vs 41% chance — fixed per-speaker mode table + position weights +
geometric combs is the confirmed shape); mode Qs come from the
measured T(f) ring law (1–1.3 kHz rings 2× its neighbors — the
depth band); 32–48 modes covers the 13–27 prominent-mode budget;
real IRs are minimum-phase (excess GD ~0.04 ms).
Original sketch: cone breakup as a modal resonator bank
(~24–48 two-pole modes per driver). Above the piston band the cone
is a bending-wave system; the dense peaky 1–5 kHz fingerprint IS
the speaker's identity. Mode distribution follows a physical law
(cone mass, profile stiffness, edge/doping damping set density,
spread, and Q envelope); per-speaker fingerprints are fitted (M6).
Drive-dependent softening: mode Q and center drift with excursion
envelope — the breakup that IRs freeze. Until M2 lands, M1 ships a
declared PROVISIONAL top end (mass-rolloff lowpass + presence
tilt), deleted the day the bank arrives.

**THE BOX** (M3) — closed back adds air-spring stiffness
k_box = ρc²·Sd²/V (alignment shift emerges from the motor — no
separate EQ); open back opens the rear path: inverted radiation
wrapped around the baffle (delay + diffraction lowpass), combing
against the front at the mic — mid-bass thinning and the dipole
roll-off emerge from geometry. Baffle step and edge ripple from
cab dimensions; panel resonances by material (birch/pine/MDF =
modal sets on the interior pressure); interior standing waves
reflected back through the semi-transparent cone.

**THE AIR** (M4) — the microphone and the geometry. Mic models
(dynamic presence peak, proximity vs distance, off-axis rolloff);
position across the cone (cap→edge interference falls out of the
modal bank's radiation weights); multi-speaker cabinets (2×12,
4×12): per-driver path lengths to the mic comb exactly as the real
thing — the 4×12 sound is mostly geometry, nearly free.

**THE HEAT** (M5) — voice-coil thermal compression: Re rises with
dissipated power (seconds-scale state at control rate), sensitivity
sags, the tone leans on the mids. The "speakers warming up" feel;
no IR can do it.

**THE FIT** (M6) — the measurement closes the loop: an offline
project-local fitting harness (tests/tools, never `tools/`) renders
tolex's IR via the plugin harness, compares against reference
measured IRs (log-spectral distance + smoothed-band error), and
fits modal fingerprints into the physical space. The linear-leg
laws land here: named reference speakers within stated distance.

**The precursor shipped with M1**: `tests/fidelity.rs`, the
COMPLEXITY report — tolex scored against the DISTRIBUTION of a
reference pack (fine-structure density, ripple energy, corners,
slope, decay shape), not any one curve. Reference IRs live
untracked in `plugins/tolex/refs/` (commercial content, never
committed); the report documents, it does not block. It has already
paid: the provisional voicing's slope (30 dB/oct, outside p90) and
corner were retuned onto the population median the day it ran.

## Skipped, on purpose

Ported/bass-reflex enclosures (rare in guitar cabs), room reverb
(a mixer's send, not the cabinet), stereo room mics v1, an IR
loader (see doctrine), FDTD/FEM realtime acoustics (offline-only
territory; modal + geometric paths carry the audible truth).

## The parameter ledger (ids are forever)

Stamped: 1 output, 2 bypass. Planned — minted only when their
milestone lands, append-only:

| id | name | group | milestone |
|----|------|-------|-----------|
| 3 | size (8/10/12/15") | Speaker | M1 |
| 4 | resonance (fs) | Speaker | M1 |
| 5 | q (Qts) | Speaker | M1 |
| 6 | mass (Mms scale) | Speaker | M2 |
| 7 | breakup (amount) | Speaker | M2 |
| 8 | doping (edge damping) | Speaker | M2 |
| 9 | drive (amp swing) | Amp | M1 |
| 10 | damping (source Z) | Amp | M1 |
| 11 | back (closed/open) | Cab | M1 |
| 12 | cab size (volume) | Cab | M1 |
| 13 | material | Cab | M3 |
| 14 | speakers (1/2/4×12) | Cab | M4b ✔ |
| 15 | mic distance | Mic | M1 |
| 16 | mic position (cap→edge) | Mic | M2 |
| 17 | model (mic, corpus-distilled) | Mic | M4a ✔ |
| 18 | mic angle | Mic | M4 |

## The road to world class — impact order (amended 2026-07-20)

Measured standing after M2: INSIDE the reference population on 12
of 13 measurable axes through both instruments (fine-structure
density sits on the population median exactly; the depth band rings
94 ms). The one outside row is low-band ripple — M3's box. From
here, ordered by audible impact:

1. **M3 — THE BOX**: closes the linear scoreboard's last row.
2. **M4a — THE MICS**: distill per-mic signatures FROM THE CORPUS
   by differencing same-speaker/same-position groups across its
   labeled mics (57/57m/57v/421/121/160/U47/U67/SM7/906…) — the mic
   axis measured in context, no datasheets. Mic selector (id 17).
3. **M6 — THE ANCHORS** (reframed 2026-07-21, "the goal"): fit a
   few reference speakers into the physical space as CALIBRATION
   landmarks — so designing "between a Greenback and a V30, but
   lighter" means something. Law: each anchor within stated
   log-spectral distance at matched position class. The product
   stays the space, not the anchors.
4. **M4b — THE CHORUS**: multi-speaker geometry (id 14) — per-driver
   path combs; law: minima at path-difference predictions.
5. **M7 — THE GRIND** (the nonlinear deepening): Doppler/FM
   (excursion phase-modulates radiation — the "cone working"
   shimmer), Le(x,i) flux modulation (asymmetric H2/IM grind), the
   LR2 semi-inductive coil ladder (drive-dependent treble
   interaction), suspension creep (the cab settles into a chord),
   breakup mode droop with excursion (the bark). Plus THE NONLINEAR
   HARNESS: multi-level swept renders → compression curves and
   harmonic-vs-drive tracks against published large-signal data
   (IRs cannot ground-truth this leg), and the full-drive alias
   report that decides whether the motor earns a Oversampled2x.
6. **M5 — THE HEAT**: two-time-constant thermal (coil seconds,
   magnet minutes), Re(T) sag + recovery, law-pinned.

Cross-cutting bars along the way: joint-structure (covariance)
checks so marginal realism can't hide unphysical combinations; a
cross-vendor reference pack to confirm the corpus laws generalize;
program-material validation (real DI through reference vs tolex,
band-envelope comparison, then ears). The demo that states the
thesis: one riff at bedroom vs gig drive — tolex's renders differ
the way a real cab does; an IR's renders are the same file, louder.

## The frontier (amended 2026-07-21) — to "ideal cab for any amp"

Execution order, after the field-driven hardening of the living leg:

1. **PROXIMITY** — the pressure-gradient bass boost, per mic
   PATTERN (a ribbon leans in hard, a cardioid moderately), applied
   RELATIVE to the corpus capture distance (~3 cm) so the distilled
   signatures stay untouched at the default. The most audible
   missing linear mechanism; the distance knob comes alive.
2. **THE NONLINEAR HARNESS** — SHIPPED 2026-07-21
   (tests/nonlinear_report.rs). First measured run: excursion-regime
   compression 0→12 dB across the drive range with H3 −58→−5 dBc (a
   real large-signal speaker story); force-regime compression ~0
   (correct — mid compression is THERMAL and FLUX physics, arriving
   with M5/M7, and the report will watch it arrive); **alias floor
   −110 dBc at full drive → the motor does NOT earn a Oversampled2x**
   (the base-rate decision is now measured, per canon — the cliff
   buries fold-back). Findings for M7: H2 is ~−100 dBc because the
   model lacks ASYMMETRY (offset coil, asymmetric suspension) —
   real speakers speak H2; add Bl(x)/Cms(x) asymmetry terms.
3. **M6 THE ANCHORS + THE DESIGNER** — fitted landmarks, morphing
   between them, and "new cabinet": population-constrained random
   build sheets — better-than-real as one click.
4. **THE FORGE HANDSHAKE** — the flagship: tolex computes its live
   impedance; forge's power stage sees it as its actual load.
   Amp and cab become one circuit — the literal "ideal cabinet for
   any amp"; no IR product can follow.
5. Fidelity ceiling: cross-rate/cross-block invariance laws;
   second-vendor corpus; ERB-weighted + joint-covariance metrics;
   the room voice from the ambient captures (§8); mic angle (id 18).
6. Product finish: presets.rs (anchors + morphs), level-matched ABX
   protocol, the fidelity report published as the spec sheet.

Still refused: IR loader, convolution in the audio path, generic
reverb. The doctrine has survived every measurement.

## The milestone ladder

- **M0 — STAMPED** (2026-07-20): scaffold, gates green.
- **M1 — THE MOTOR**: lumped nonlinear core + source impedance +
  closed/open back first physics + provisional voice. Laws:
  `the_box_stiffens_the_spring` (closed-back fc lands where
  ρc²Sd²/V says, measured against the free-air peak),
  `the_amp_lets_the_cone_bloom` (tube-Z vs solid-state-Z resonance
  bloom ≥ threshold), `excursion_compresses` (drive up 12 dB at
  90 Hz → fundamental gains < 12 dB and H3 appears),
  `open_back_thins_the_blanket` (open vs closed mid-bass energy
  drop in the wrap-comb band), plus battery/no-alloc/perf/editor.
- **M2 — THE VOICE**: modal bank + drive softening + mic position;
  provisional top end deleted. Laws: modal density/spread follow
  doping and mass; softening measured at drive extremes; position
  morphs monotonically cap→edge. **Measured targets** (the fidelity
  report, 2026-07-20, vs 1,814 York 48k natural-phase singles —
  p10/median/p90): fine-structure extrema 6.1/8.1/10.0 per octave
  over 0.7–6 kHz at ±1 dB prominence (M1: 0); residual ripple RMS
  1.6/2.1/2.8 dB (M1: 0.03); t90 energy time 2.2/3.2/5.3 ms (M1:
  1.7). The bank is DONE when tolex lands inside all three. Low-band
  ripple (0.3/1.4/2.7 dB; M1: 0.2) belongs to M3's reflections.
- **M3 — THE BOX**: interior short taps at the corpus-measured
  0.3–1.0 ms (baffle-edge and side-wall scale — the corpus shows
  ~zero discrete energy beyond 1 ms in close mics; back-wall energy
  is diffuse), panel material modes (id 13: birch/pine/MDF as
  low-mid resonator sets scaled by box size). Laws:
  the_box_prints_its_scale (low-band ripple lands inside the
  population band AND the box lag scales as ∛V),
  the_panels_have_a_voice (material moves the low-mid signature).
  Display twins + the picture-truth law extended.
- **M4 — THE AIR**: mic models + angle, multi-speaker geometry.
  Law: 4×12 comb minima land at path-difference predictions.
- **M5 — THE HEAT**: thermal Re(T) state. Law: sustained full-drive
  program sags output by the datasheet-ish dB over seconds and
  recovers.
- **M6 — THE FIT**: fitting harness + reference fingerprints. Law:
  named reference speakers within stated log-spectral distance;
  blind-render A/B docs.

## Performance envelope

M1 core: one 3×3 elimination + a handful of biquads + one wrap
delay per lane — well under a microsecond a frame. M2's bank adds
~48 biquads/lane. Budget declared in tests/perf.rs per canon; the
alias report decides oversampling, the perf report keeps us honest.
