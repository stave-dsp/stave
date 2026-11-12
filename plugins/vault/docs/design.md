# VAULT — a place, not an algorithm

**Status: MVP + THE ROOM SHIPPED 2026-07-19 (plugins/vault).**
The goal on record, sharpened by the field: **the fidelity of
impulse-response reverbs with the nonlinearity of algorithmic ones
— spaces no other reverb can make.** A convolution reverb is a
photograph of a dead room; vault is the room itself, standing,
with walls we can bend. The fidelity half is SYNTHESIZED geometry
(never sampled, so it stays alive); the nonlinear half is the
tank. Field diagnosis that drove this: the MVP alone was "smooth
beginning to end" — in honest taxonomy a PLATE (instant density,
no early field, no size cues). The room fixes what a plate
structurally cannot say.

## THE ROOM (slice 1, shipped)

A live image-source model in engine/room.rs: a shoebox is
DESCRIBED (dims from SIZE; source and listener as fractions; two
ears at their true positions 18 cm apart — the head does NOT scale
with the room) and all 24 order-≤2 reflections are DERIVED:
fractional-sample delays from path length (ITDG convention — gap
over the direct path, per ear), levels from distance and
per-bounce PER-SURFACE materials (floor swallowed by audience,
ceiling plaster, walls carry the lateral energy), darkness from
bounce count plus head shadow, all through 48 per-tap one-poles.
ITD/ILD are EMERGENT — each ear simply lives where it lives, so
the interaural field can never disagree with SIZE. Tap delays ride
the shared slew-glide doctrine (SIZE bends the room like tape); a
fresh room SNAPS (onsets must not crawl). The tank feeds FROM the
room's answer (diffused ER, INJECT ×1.8 — measured 10 dB feed
drop when the tank moved off raw input), so early and late cannot
disagree about where they are. New param: EARLY (id 12).

MEASURED LESSONS: (1) the first-photon onset does NOT scale with
size (2.06x for a 5x room) because floor/ceiling answer in ~2 ms
at any size — your head height never scales; the honest size cue
is when the answer's ENERGY arrives (echo centroid), and the
honest fix was more fidelity (per-surface materials swallowing the
grazing floor bounce). (2) The centroid law isolates the room by
RENDER DIFFERENCE (early 1 minus early 0 — identical late fields
cancel to bits). (3) Drive's audibility law needs SUSTAINED
excitation — impulse recirculation never reaches the knee (2.7e-7
then 8.8e-7, measured twice).

New laws: the room's answer arrives with its size (centroid ×2.5+
across a 5x size span); the ears disagree early (interaural
difference > 25% in the first 20 ms); sparse-then-dense (early
crest factor > 1.8x late — the arc a plate never had); geometry
consistency + emergent-ITD unit laws in room.rs.

## SCATTER + SPLAY + IN-LOOP DIFFUSION (slice 1b, shipped)

Field report on the raw room: "materially worse around diffusion —
I can hear the bouncy delays clearly" + "classic metallic ring
sometimes". Both were textbook failures, both fixed live (nothing
is baked — the standing constraint: IR-level fidelity, never an
IR; every stage stays time-variant and nonlinear):
* SCATTERING — a wall is rough and finite, so a reflection is a
  SMEAR, never a Dirac (measured IRs agree): every image tap is
  now a 4-read velvet burst (sign-alternating, jittered per tap,
  spread 0.9 ms for order-1 / 4.1 ms for order-2 — scattering
  accumulates with bounces). Law: no 1.5 ms bin of the isolated ER
  holds > 25% of its energy (measured 0.12).
* SPLAY — no two real walls are parallel: each image perturbed by
  a stable per-image hash × 4.5% of dims (flutter periodicity
  broken; keyed by IMAGE, never by ear).
* IN-LOOP DIFFUSION — an allpass inside each tank line's feedback
  (lossless, passivity untouched): every trip's phase goes
  frequency-dependent, modes go inharmonic, the FDN comb ring
  cannot build; density grows per trip. T60 gains now computed on
  the FULL loop length (line + allpass DC group delay). Law: late
  crest < 6 (measured 4.41 ≈ a dense random field).
BUG RECORDED: the allpass read was named `d`, shadowing the
saturator pre-gain `d` — the soft clip divided by a SIGNAL (NaN
under drive). Shadowing in a hot loop is a rename, not a style
choice.

NEXT (the 100x plan, user-ratified direction): one room
description drives EVERY stage — mid-field densification from the
geometry's mean free path (live sparse process, the early/mid/late
arc), modal bank at the room's true low-frequency modes with
position-dependent coupling, per-surface material palette with
Sabine-derived per-band late T60, source/listener as real controls
(floor-plan Scene later). Constraint on record: never build an
actual IR — the IR is what the machine WOULD emit if frozen.

## THE ACOUSTICS SCOREBOARD (slice 2, shipped) + the CPU walk-back

Field: "quality doesn't match the 7-10% CPU; decay unnatural; is
the diffusion/modulation too by-the-books?" Answered with an
instrument, not adjectives — tests/acoustics.rs renders
deterministic responses per REVERB CLASS (ROOM-S / CHAMBER / HALL
/ PLATE / SPRING?) under BOTH excitations (impulse AND a 120 ms
performance-level burst — a nonlinear machine has no single IR;
nonlinearity is an excitation OPTION in every row) and computes
the literature suite: T30/EDT + Schroeder linearity, NED(t)
echo-density (Abel–Huang), C80/D50/Ts, IACC early/late, spectral
ridge (metallic), envelope flutter (detrended — the raw envelope's
decay trend autocorrelates at every lag, measured 0.8+ everywhere,
meaningless).

CPU: ablation showed the room = ~700 of ~1100 ns/frame, and the
lever was INTERPOLATION, not read count (removing 144 of 192 reads
moved nothing — the cubic arithmetic was the cost): ER taps are
heard once (no recirculation to compound interpolation loss) so
they read LINEAR now; diffuser stages likewise (near-fixed length
= constant sub-sample tilt, not accumulating). ~450 ns/frame ≈ 2%
— the pre-room budget with the whole geometry running. Perf is now
a RATCHETED LAW (ceiling 1100 ns/frame release / 22k dev). The
per-tap velvet scatter (192 interpolated reads) is replaced by two
tiny allpasses per ear — scattering is a TEXTURE, not a position.
Tap-major block processing recorded as the next optimization
lever (needs a kernel block-read).

SCOREBOARD'S FIRST CATCHES: (1) the in-loop allpass form
(−g+z⁻ᴺ)/(1−gz⁻ᴺ) has DC group delay N(1+g)/(1−g) = 3N — the
(1−g)/(1+g) draft under-compensated 9x and ROOM-S decayed +60%
long; fixed, ROOM-S now measures T30 0.61 s vs 0.60 dialed. (2)
C80 is NEGATIVE for all room classes (real rooms: positive) — the
early field is too weak vs the late: the "no early/mid/late
dimension" complaint, quantified. (3) IACC_early ≈ 0.2 (real
rooms ≈ 0.7-0.95): the early field is OVER-decorrelated — real
dimension = coherent early + decorrelated late, ours is wide mush
everywhere. (4) flutter 0.55-0.74 on HALL/PLATE (real < ~0.3):
audible per-line envelope periodicity survives in the tank. These
four numbers ARE the next slices' work orders. T30 class pins are
measured baselines (damp absorbs beyond the dial until V3 makes
the promise per-band).

## CHARACTER AXES + PRESETS (slice 3, shipped, 0.4.0)

Field: "same controls, can't make the timbre meaningfully
different — not convinced." Correct: 0.3.0 was instrumentation
only. Presets over a narrow space would be ten spellings of one
room, so this slice adds the axes FIRST:
* BASS (id 13, 0.25-4x log) — low-band decay multiplier below
  ~350 Hz: each tank line splits a low shelf wearing its OWN T60
  gain (halls ring bass 1.1-1.4x; tight rooms cut it). Law:
  bass 3x rings the lows on 4x+ while a 3-stage-HP'd high band
  moves < 1.6x (single-pole "high band" leaked the louder lows —
  6 dB/oct is not a band split, measured 2.7x false positive).
* TONE (id 14, ±1) — static wet tilt at 1.2 kHz (DAMP darkens
  over time; TONE colors now). Law: HF 2x+ apart across the dial.
* DIFFUSE (id 15) — ONE dial for input diffusers AND in-loop
  allpasses; 0.7 maps exactly to the old fixed coefficients
  (baselines held). Low = discrete vintage echoes as a CHARACTER;
  the AP loop-length T60 compensation follows the live g. Law
  note: crest undersells echo-vs-wash (temporal structure vs
  amplitude statistics — 1.39x measured against a clearly audible
  difference; a dead dial reads 1.0x, pinned 1.25x).

8 factory presets spanning the corners (each states the FULL
surface): STONE CATHEDRAL (bass 1.9, slow), WOOD ROOM (tight,
early-heavy, bass cut), BRIGHT PLATE / DARK PLATE (the ER-less
plate poles), CHAMBER, TAPE WAREHOUSE (wow+drive lofi), ECHO
CANYON (diffuse 0.08 — the discrete-echo character on purpose),
AIRLOCK (size 3, 14 s, bass 1.6). Law: every preset applies
cleanly (ids→names via the table), sounds, stays bounded, dies.

## THE FIDELITY SLICE (slice 4, shipped, 0.5.0)

Directive: maximize quality/fidelity/dimension AND cpu. Four
physical changes, scoreboard-refereed, net CPU DOWN (379 ns/frame
≈ 1.8%):
* THE LATE FIELD GROWS FROM SECOND-ORDER ENERGY — the tank feeds
  from the b=2 image taps only (already computed; zero new reads):
  the diffuse field's onset trails the early field and scales with
  the room. C80: ROOM-S −0.7 → +4.1 dB, CHAMBER −3.1 → +1.1,
  HALL −4.9 → −1.7 (all inside real-room literature bands now);
  EDT/T30 1.65/1.26/1.22 → 1.25/1.05/1.12. The "no early/mid/late"
  complaint is answered by topology, not level knobs.
* THE EARS SHARE THE SMEAR TUNING — the detuned-per-ear scatter
  decorrelated everything; scatter off the same wall reaches both
  ears nearly identically, the geometry's ITD/ILD is the honest
  interaural difference. (IACC_E note recorded: literature values
  include the DIRECT sound — wet-only readings are not comparable
  to the 0.7-0.95 band; the dry path is the anchor in use.)
* STAGGERED TANK TAPS — wet output reads mid-line at irrational
  fractions (feedback unchanged): output timing decouples from
  loop periods. Flutter: ROOM-S 0.24 → 0.13, CHAMBER 0.36 → 0.30.
  OPEN: PLATE flutter 0.75 unchanged (tank-only exposes line-period
  energy pulsing; needs deeper tank work — more lines or nested
  in-loop diffusion).
* SIDE LOW-FOLD — the side channel's lows fold to mid below
  ~250 Hz (real rooms are interaurally coherent at LF; wide mixes
  survive mono).
Also: CHAMBER late-tail ridge rose 4 → 9-11 (within law, watch);
diffuse law re-stated on NED (the principled density metric) after
two crest drafts undersold the ear.

## NESTED DIFFUSION + THE DRIVE SERVO (slice 5, shipped, 0.6.0)

Autonomous continuation on the two known defects:
* SECOND NESTED IN-LOOP ALLPASS per line (AP2_MS 1.7-9.7 ms):
  cured the CHAMBER ridge watch item (8.9-11 → 3.6/4.7). CALIBRATION
  LESSON: an allpass's group delay is N(1+g)/(1-g) at DC but
  N(1-g)/(1+g) at HF — two stages doubled the spread and full-DC
  compensation over-sized the T60 loss (32% fast, measured); the
  energy-weighted middle is AP_DC_FACTOR 0.55, calibrated by the
  t60 law. T30 class baselines re-pinned (0.68/1.13/1.37/1.54).
* THE DRIVE T60 SERVO: the passive saturator meters its own loss
  per line per block (sat_in/sat_out accumulators) and restores
  the DECLARED per-trip gain, comp clamped [1, 1.5] AND ≤0.9991/g
  (strictly below unity loop). Drive is COLOR now; law:
  drive_keeps_the_dialed_decay (post-burst slope at drive 1 within
  0.6-1.45x of clean — it used to be ~2x fast).
* DIFFUSE-DIAL DENSITY CAPS: allpass density PEAKS near g~0.5-0.6
  and gets SPARSER toward g→1 (the direct term dominates —
  measured: the plate's NED slowed at the dial's top). Input g
  capped 0.68, loop g capped 0.58; the 0.7-default mapping is
  untouched.
* OPEN, honestly: PLATE flutter ~0.77 stands. Nested diffusion and
  caps barely move it — the line-period energy pulsing needs true
  DISPERSION (frequency-dependent delay, the plate/spring physics
  slice), not more of the same allpasses.
Perf: 403 ns/frame (1.9%) with everything above running.

## THE STIFF DIAL + THE BEATING FINDING (slice 6, shipped, 0.7.0)

STIFF (id 16): true dispersion — a 6-stage first-order allpass
cascade in every tank loop, pole up to 0.92 (the dynamo wire's
twice-measured lesson: differential delay lives in (1-a)², a 0.62
ceiling was homeopathic). Highs travel faster: the plate/spring
downward chirp, compounding per trip (law: the render difference
grows through the tail). BRIGHT/DARK PLATE presets carry 0.65/
0.55; rooms stay 0 (air does not disperse).

THE FINDING (probe-verified): the plate's flutter 0.77 was NEVER
repetition — the envelope autocorrelation decays monotonically
from the shortest lag, no line-period peak, motion-independent.
It is SLOW ENVELOPE BEATING from finite mode density, exposed in
tank-only classes and masked by the ER in room classes. Dispersion
cannot cure it (measured twice: 0.77→0.74). The cure is MODE
DENSITY — more/longer lines, a deliberate CPU trade to design as
its own slice (the tank is 403 ns/frame; there is headroom).

## THE LOUDNESS SLICE (slice 7, shipped, 0.8.0)

Field insight, correct: "some of the missing fidelity is just
volume." MEASURED: full wet ran −11.5 dB under the dry it
replaced, and the linear mix crossfade under-weighted the wet
another ~10 dB at working settings. Three layers:
* WET_MAKEUP x3.4 (+10.6 dB) — full wet lands ~level with dry on
  sustained material (law: ±3.5 dB, the_wet_holds_its_own);
* EQUAL-POWER MIX (sqrt crossfade) — mix 0 stays BIT-dry
  (sqrt(1) = 1 exactly);
* THE DUCK (id 17, FIELD card): the wet compresses under the
  DRY's envelope (5 ms attack / 220 ms release) and blooms into
  the gaps — the companding trick that lets a reverb run loud and
  stay clear ("reactiveness"). Law: during-input wet < 0.65x of
  unducked; tail blooms back > 0.8x after release. Default 0 (the
  laws' render conditions stay clean); presets carry 0.15-0.5
  (CHAMBER 0.45 for vocals, ECHO CANYON 0.5 — echoes bloom in
  the gaps).
Perf 456 ns/frame (2.2%). Tank-internal levels untouched — the
drive knee calibration is unaffected (makeup is output-side).

## THE FIELD SLICE (slice 8, shipped, 0.9.0)

Field critique: "bass and tone sound like lazy post-EQs; L/R too
similar with a strong center always; width should go narrow to
artificially wide — make tone/EQ/width/balance part of the plugin,
not post." All four moved INTO the model:
* TONE IS THE WALLS now: it scales the room's material cutoffs
  (x2^±1.6), darkening the early taps AND the late field through
  the tank's feed. The post-tilt is deleted; its law restates the
  material truth (dark drapes CUT highs and leave lows alone — a
  post-tilt boosted them, which was exactly the "lazy EQ" tell).
* STAND (id 18, default 0.2 — nobody stands at the geometric
  center): listener walks the room's width. MEASURED PHYSICS
  LESSONS: per-tap ILD alone nets ~1.06 balance (near-wall energy
  and source-direction energy structurally cancel in RMS), and
  the strongest real-world lateral cue — the direct sound — lives
  in the untouchable dry. The cue that survives into the wet is
  the LEAN TOWARD THE SOURCE: geometry-derived equal-power lean
  (±~3 dB at the walls) + per-tap ILD/ITD/coloration asymmetries.
  Law: stand ±0.9 leans the field <0.90 / >1.1, mirrored.
* WIDTH 0..2 (id 11 RETIRED, re-minted as 19, default 1.0):
  narrow → natural → artificially wide (side boost, slight mid
  dip past 1). Law: width 2 more than doubles side/mid vs 1.
* BASS split is two-pole now (a 6 dB/oct seam read as EQ mush).
Passivity law metric moved to window RMS (peaks ripple ±7% from
wow-beat alignment at the 30 s corner; energy is the claim).
Perf 420 ns/frame (2.0%).

## THE MODAL LOW END (slice 9, shipped, 0.10.0)

Field: "low mids and lows are the weakest — static, non-moving,
one-dimensional, no attack/decay or timbre change." DIAGNOSIS:
below ~150 Hz vault degenerated into a static comb network (the
in-loop allpasses are transparent at LF; cent-based chorus is
frozen there; the image-source room is a geometric model and real
rooms below Schroeder are MODAL). The ranked plan's #1+#2 shipped:

* engine/modes.rs — 24 FIXED mode identities (axial+tangential
  shoebox orders), frequencies DERIVED from the dims, identity
  never re-selected (SIZE slides a mode, band-edge fades it —
  never a click). Position coupling = cos products at source AND
  listener: STAND selects which modes speak (a node silences one:
  unit law). Per-mode T60 spread (timbre evolves through the low
  tail), per-mode slow drift (the beating itself moves), and
  resonator charge time = the LF bloom on held notes.
* TANK: per-line LOW-decay spread (fast components drop out — the
  low-mid timbre evolves as it dies) + the SUB HANDOFF: below the
  modal coverage (85/size Hz) the tank decays at x0.3 — attack
  body only, the standing waves own the sustain. Big rooms slide
  their modes below audibility and keep the statistical tank —
  physically correct on both ends.

FOUR MEASURED DEAD ENDS on the way (all caught by laws/probes):
(1) a sin(w) floor of 0.05 sat ABOVE every mode's sin(w) at 48k —
the whole bank muted ~15x; (2) ring-amplitude-flat normalization
let sustained drive integrate to Qx (peak 468, then 87) — the
stable form is DRIVEN-GAIN-FLAT, k = 2(1-r)·sin(w)·MG, and
impulses barely charging high-Q modes is honest physics; (3)
MODE_GAIN 14 dragged broadband IACC_late to 0.7 (too-mono LF
layer) — voiced to 10 against the scoreboard; (4) the charging
law: noise charges a resonator as a RANDOM WALK (a realization
de-charged it, inverted measurement) and rectangular-window
Goertzel leaked -13 dB sidelobes — the law now SINGS at the mode
(held bass notes are where the bloom lives) through a Hann window.
T30 pins re-centered (low modes outlast the mid decay — bass-ratio
physics): 0.74/1.43/1.63/2.22. Perf 496 ns/frame (2.4%).

## THE CLIPPING FIX (slice 9b, shipped, 0.10.1)

Field: clipping after the modal bank. MEASURED worst case: a bass
note parked ON a mode peaked 7.44x full scale (mode resonance
+20 dB x makeup), and even broadband full-wet peaked 1.08 (a
diffuse field's Gaussian crest sits ~2.5x above a matched-RMS
dry). Three layers, all passive:
* MODE_GAIN 10 → 4 (real modal peaks stand +6-10 dB, not +20) +
  a passive limit on the modal sum (boundary absorption goes
  nonlinear at high SPL);
* THE WET CEILING: unity-small-signal soft limit on the whole wet
  pre-makeup — because the TANK also rings 1/(1-g) ≈ 15-30x on a
  parked comb peak (measured 2.88 after the modal fix alone; the
  hazard was always latent, the loudness slice exposed it). Tails
  and normal program pass untouched; parked resonances compress
  like a room at high SPL, not a clipped bus;
* WET_MAKEUP 3.4 → 2.7 (RMS ~-2 dB vs dry, civilized peaks).
Law: the_wet_never_clips (parked-tone full-wet peak < 1.25;
full-wet noise < 1.0). Measured after: 7.44 → 1.21, 1.08 → 0.78.

## PASSIVE AT EVERY DIAL (slice 9c, shipped, 0.10.2)

Field: turning BASS or DECAY *down* produced constant clipping/
distortion. ROOT CAUSE — my "steeper seam" two-pole band splits
in the FEEDBACK path are not phase-complementary: near the
crossover the cascaded poles swing toward −180°, so (dc − low2)
ADDS instead of subtracting and the recombined loop magnitude
EXCEEDS the band gains. With equal band gains the terms telescope
perfectly (invisible — the passivity law only tested there); when
bass or decay spread the gains apart (they diverge EXPONENTIALLY
as T60 shrinks: g − g_low grows from 0.065 at 2.4 s to 0.34 at
0.3 s), the crossover bump pushed loop gain past 1 → self-
oscillation into the wet ceiling = constant distortion.

FIX: the in-loop band gains are now CASCADED FIRST-ORDER SHELVES
(x1 = dc + (g_low/g − 1)·LP(dc), then the sub shelf, then ×g) —
minimum-phase, magnitude provably BETWEEN the band gains at every
dial. The seam is 6 dB/oct again: SAFETY OUTRANKS SLOPE in a
feedback path (a steeper safe seam is a Linkwitz-Riley design
problem for later, outside the loop or phase-matched inside it).

New law: the_loop_is_passive_at_every_dial — impulse renders at
the hostile GAIN-SPREAD corners (decay 150 ms x bass 0.25/4.0,
etc.): bounded, and silent by 2.2 s (self-oscillation is exactly
what it catches). LESSON FOR THE CANON: a multiband split inside
a feedback loop must be phase-complementary, and passivity laws
must sweep the dials that SPREAD the band gains, not just the
defaults.

## TRUE-STEREO EXCITATION (slice 10, shipped, 0.11.0)

The review's top finding, built: L and R are TWO SOURCES standing
apart on the same stage (±0.18 of the width, straddling the
listener's axis; each at -3 dB so dual-driven mono sums to unity
energy across the incoherent tap set). 2 sources x 2 ears x 24
images = 96 ER taps; each source couples independently to every
MODE (k_a/k_b sample the mode shape at each stage position).
Antiphase input reverberates now (it used to produce literally
zero wet); hard-panned input lateralizes the early field; and a
delicious physical consequence, now a code comment: a CENTERED
identical pair cannot excite odd-x modes at all (antisymmetric
across the center plane — L+R cancels exactly), while panned and
decorrelated material can. STEREO PLACEMENT IS A MODAL TIMBRE.

Harness: render_stereo_lr (independent L/R — the review caught
every acoustic test copying L into R). New laws: antiphase still
reverberates (>0.15x in-phase); one-sided input lateralizes in
the 35 ms precedence window, HP 300 Hz (the modal layer is mono
by design and the late field deliberately diffuse — broadband
whole-tail ratios dilute toward 1); decorrelated inputs widen
side/mid. GEOMETRY LESSON: the mono-era off-axis source put the
"right" source dead ahead of the listener — R-only input
lateralized LEFT (measured 1.14) until the stage centered.
Charge law hard-pans its bass note (a centered pair excites no
odd-x mode — its own comment now explains why).

Perf 692 ns/frame (3.3%) — the doubled ER is the cost; tap-major
block processing remains the recorded lever if it matters.

## THE SCENE (slice 11, shipped, 0.11.1)

engine/scene.rs — the ONE description of the place: dims, stage
positions, listener, ears, balance. Room and modes now CONSUME it;
neither owns a geometry constant, and the "kept in sync with
room.rs" comments are gone — the compiler enforces what discipline
twice failed to (the review's shared-AcousticScene call, and the
same silent-drift class that shipped two bugs). Next tenants for
the scene as the roadmap lands: per-band materials, mean free
path, Schroeder frequency, and the coupled second volume.

## SIXTEEN LINES (slice 12, shipped, 0.12.0)

The density thesis, confirmed: doubling the tank to 16 lines
(the published floor for high-quality FDNs; modal density of an
FDN ~ its total delay) attacks the oldest open finding — the
tail's slow envelope beating from finite mode density. ROOM-S
flutter 0.14 → 0.05, EDT/T30 lands exactly 1.00, and late-field
IACC returns to healthy diffuse values (0.15-0.45). Tap gain
energy-matched (0.25/√2), tap vectors are balanced orthogonal
H16 rows, every per-line table doubled with irrational spacing.
All T30 pins held within bands, every law green on the first
run. Perf 832 ns/frame (4.0%) — the honest price of density;
tap-major block reads remain the recorded lever.

## KICKS ARE CLEAN (slice 13, shipped, 0.12.1)

Field, twice: "terrible artifacts/distortion when bass or decay
are low — worst on kick drums; clipping at gain and envelope
level that lowered volume and collapsed the sound." The impulse
passivity laws passed throughout — the regime was PROGRAM, the
drive-law lesson again. Two stacked causes, measured:
* the modal bank's TRANSIENT response scales with (1-r): at
  decay-min x bass-min it ran ~80x the default — every kick built
  a huge LF spike exactly when those knobs were low. FIX: (1-r)
  capped at its ~1 s-decay value (short wide modes honestly peak
  lower; a kick charges a dead room's modes no harder than a
  live one's);
* the wet ceiling was a WAVESHAPER on the summed waveform: the
  kick's spike crushed everything riding on it — instantaneous
  gain collapse + IMD, the exact "lowered volume and collapsed"
  percept. FIX: an envelope LIMITER — and the attack must be SLOW
  (30 ms): instant attack read 0.28 nonlinearity share on
  ordinary kicks at DEFAULTS (the protection was the distortion);
  a kick lives ~50 ms while the parked-resonance runaway this
  stage exists for builds over seconds.
THE ONCE-AND-FOR-ALL LAW: low_dials_are_clean_on_program —
LINEARITY itself as the metric (render a burst train at A and
A/4; a clean reverb scales exactly; residual = distortion by
construction), pinned < 0.10 at defaults AND the hostile-low
corner. Kick-probe results: both-min 0.108 → 0.0178; parked-tone
bound re-pinned 1.7 (slow smooth gain, not a hard ceiling — the
runaway this law was born from measured 7.44 through a
waveshaper). LESSON, third time now law: impulse laws prove
stability; only PROGRAM-material linearity proves clean.

## THE MID-FIELD + MATERIALS (slice 14, shipped, 0.13.0)

Field: cathedral-scale sounds read as a MULTITAP with a lumpy gain
envelope (measured: NED t90 0.060→0.105 s and flutter 0.29→0.49
from size 1 to 2.7 — the order-2 truncation gap opens with size);
and "every room is built from the same walls."

THE MID-FIELD (engine/midfield.rs): per-ear scattering stage at
the room's own MEAN FREE PATH (scene-derived 4V/S). Three design
lessons paid for en route: (1) an allpass redistributes time at
FULL strength at any g — gating by coefficient degenerated to a
pure 21 ms delay (the ears law caught bit-identical mono LF); the
gate is a PARALLEL BLEND, specular always instant, and the mix IS
the wall's scattering coefficient. (2) The stage is SIZE-GATED —
it fills a truncation gap that only opens in big rooms; engaged at
size 1 it ADDED flutter (0.29→0.60) where nothing was missing.
(3) Static recursion at mfp spacing repeats — the delays DRIFT
(±3%, hash-rated): real scattering never repeats. The tank keeps
its second-order feed (slice 4's C80 structure) through its own
mid-field pair. At 2.7: C80 −4.3→−1.1 (hall band), Ts 152,
density up; flutter 0.62 vs 0.49 pre — the residue is tap
periodicity, the stochastic higher-order tap set stays on the
roadmap.

MATERIALS (id 20, scene-owned table): WOOD/PLASTER/STONE/GLASS/
DRAPE/METAL — per-bounce wall gain+cutoff in the early field,
bass-ring multiplier and damp offset in the late. PLASTER is the
pre-materials voicing exactly (pins held). Rooms stop being built
from one substance; presets re-voiced (cathedral STONE, plates
METAL, wood room WOOD). DIFFUSE governs all three scattering
stages now. Law drift: the "diffuse 0 = discrete echoes" claim
died THREE metrics deep (crest, NED-sparsity, even periodicity —
splay + staggered taps + 16 lines randomized the undiffused field
too); pinned as a render difference, and a TRUE vintage-echo mode
is recorded as a topology decision (fewer, longer lines), not a
coefficient.

## THE CHARACTER SUITE (slice 15, shipped)

tests/character.rs — the scoreboard's missing dimension:
FREQUENCY. Per-octave T30 report (63/125/250/500/1-2k/4-8k) per
class and material, plus five hard per-band laws:
* bass_moves_only_the_low_bands (125 Hz T30 strictly ordered by
  the dial; 1-2k stays put);
* damp_orders_the_high_bands (HF/mid T30 ratio);
* materials_order_the_timbre (tail brightness centroid climbs
  drape < wood < plaster < metal; stone rings its lows 1.5x past
  drape);
* envelopes_rise_then_fall_everywhere — THE ANTI-LUMP LAW: per
  band, per size (0.45/1.0/2.7), the amplitude envelope decays
  monotonically after its rise (a second lump is the "multitap /
  weird gain envelope" defect, now un-shippable);
* the stiffness chirp stays with the render-difference law (two
  band-arrival metrics failed to isolate it from the T60
  compensation — recorded as probe + upgrade path).
FIRST REPORT (physically credible across the board): ROOM-S
plaster flat 0.61-0.70 s; HALL stone 2.65 s at 63 Hz sloping to
1.13 s at 4-8k (bass ratio ~1.5 — literature hall values); drape
flattens and kills the highs; metal plate long and bright.

## THE GAIN CONTRACT + MANY WATERS (slice 16, shipped, 0.14.0)

Field: "clipping/limiting distortion AND still over 0 dB — fix
both ends"; and "unnatural pitch modulation, like a single chorus
LFO in the tank" (BRIGHT PLATE; STONE CATHEDRAL with width up).

THE GAIN CONTRACT, restaged: the guard moved POST-makeup at 0.98
(the pre-makeup 0.42 both engaged ~7 dB too early — the audible
limiting — AND capped at 1.13, over full scale). A slow-gain
limiter cannot bound crest TIPS (noise crest rode 1.55 through a
0.98 envelope, measured), so the standard pair completes it: a
SAFETY SHAVE, identity below 0.9, compressing tips toward a 1.02
asymptote — engaging only on what the limiter's attack let slip.
A bass loudness trim was tried and DELETED on measurement (bass 4
adds ~0 dB broadband at defaults — the premise was wrong; the law
would have shipped a -6 dB hole). New character laws:
the_wet_respects_zero_db (hot noise/parked tone/kicks x defaults/
bass4-short/max-decay: peak ≤ 1.02), loudness_is_consistent_
across_dials (±6 dB over 9 corners incl the complaint's combined
one), envelopes_rise_on_time (band peaks by 0.20/0.25/0.45 s at
sizes 0.45/1/2.7). Parked-tone bound tightened 1.7 → 1.05.

MANY WATERS: the tank's motion was ONE master 0.9 Hz phase with
sixteen offsets — literally a single chorus LFO, exposed on the
plate (motion 0.6). Every line now runs its OWN incommensurate
rate (0.55-1.45x around 0.9 Hz): sixteen slow wobbles read as
water, not vibrato. The cathedral's version was the mid-field
drift (±3% at ~30 ms scale, width-exposed through the inter-ear
detune): now ±1.2% at half the rate.

## THE AUDIT (slice 17, shipped, 0.14.1)

Full-plugin audit (sound quality / true stereo / CPU), fixes
landed: (1) ALL modulation phases now reset (tank chorus, mid
drift, modal drift — transport stop is a KNOWN PLACE, the dynamo
doctrine; none of them were); (2) output-trim powf cached
(recomputed only when the dB moves — was per-sample); (3) dead
master-phase state deleted; (4) sample-rate smoke law (44.1/96k:
bounded, dialed decay, silent tail — first try green, the
seconds-derived discipline held). AUDIT LEDGER (open, ranked):
material/damp/tone/diffuse switch at BLOCK STEPS (no coefficient
glide — audible on automation; the modal poles retune with state
retained); drive saturation in-loop has no antialiasing; ER tap
GAINS step per block on size sweeps (delays glide, gains don't);
modes are strictly mono (fine below 150 Hz, the 150-215 band
could carry side); lean is broadband (could band-split); room
tap-major block reads = the standing CPU lever (~300 of 866 ns).

## THE OPTIMIZATION EXPEDITION (slice 18, shipped, 0.14.2)

Target: under 2% CPU without losing fidelity. HONEST RESULT: not
reached — 4.0% (840 ns/frame) is the measured scalar floor, and
the expedition's main product is KNOWLEDGE plus two kernel
primitives:
* DelayLine::read_int (integer-length structures skip interp
  math — tank/smear allpasses use it) and read_block (arithmetic
  delay ramp, ring resolved once, with a bit-law and a
  steady-state delta = -1.0 contiguous fast path);
* TWO block-major room rewrites both measured SLOWER than the
  per-sample baseline (1113 and 1021 vs 866): the inlined scalar
  reads were already optimal, and block machinery's buffer
  traffic cost more than loop order saved. REVERTED to the
  measured-best per-sample path, on purpose, with the numbers in
  the comment.
* Ablation ledger at 1021: room 430, tank 330, remainder 260 —
  and the pre-adventure per-sample room was 276, proving the
  regression. Micro-wins kept: powf and mix-sqrt caches, integer
  allpasses.
THE PATH TO <2%, recorded for a dedicated slice: an unsafe
two-run sliced fast path (true SIMD, no per-sample masks), SoA
tap state, and possibly wide lanes — bench-driven, not a safe
quick pass. Fidelity was not traded at any point.

## THE THIRD KICK REPORT — the case closed (slice 19, 0.15.0)

Field, third report with the decisive clues: distortion at MIDDLE
bass only (10.5-2 o'clock), gone at both extremes, drape worst by
far, "attacky-but-distorted." FOUND — three stacked mechanisms,
each measured:
1. THE HUMP: the modal transient cap min(1-r, 1.5e-4) made the
   kick response LOUDEST exactly where modal T60 ~ 0.5-1 s —
   capped-max below, falling as 1/T60 above. That T60 lands at
   bass 0.5-1.3 (the o'clock arc verbatim); drape's 0.65
   bass_mult shifts it right and its damping strips the masking.
   Probe: share 0.36-0.40 across the arc, 0.0006 with modes slid
   sub-audible (size 3) — the discriminator that convicted the
   path. Cap halved to 7e-5: hump under the limiter, held-note
   bloom untouched (uncapped branch).
2. THE MODAL SOFT CLIP (0.10.1-era, knee ~0.17) ground the modal
   sum on every kick — REMOVED. The THIRD protection stage this
   project has convicted as the distortion it claimed to prevent.
3. THE SHAVE: the slow-limiter + waveshaper pair was a design
   contradiction — transients passed the limiter INTO the shave;
   every kick tip bent. REPLACED by LOOKAHEAD limiting: the wet
   plays 3 ms late (inaudible — it is predelay) while an
   instantaneous-max envelope reads the present, so the gain is
   in place before every peak and NO WAVEFORM EVER BENDS. (A
   one-pole attack missed single-sample crests — peak 1.53,
   measured — the envelope must be max-decay by construction.)
THERE IS NO WAVESHAPER LEFT IN THE WET PATH. What remains at hot
full-wet settings is smooth gain, duck-like by nature. Law: the
mid-bass drape kick recipe joined low_dials_are_clean_on_program.
METHOD LESSON (why reports one and two missed): the first two
fixes tested at the EXTREMES (where the laws looked) — the hump
lives in the middle; sweep the dial, not its ends.

## THE AIRBAG DOCTRINE (slice 20, shipped, 0.15.1)

Design decision, user-ratified: HONESTY over normalization. A
longer, hotter room IS louder; near-0 dB sources may breathe past
full scale in float; the wet runs as loud as the physics says.
The limiter is an AIRBAG at 1.35 (+2.6 dB) — it exists for
runaway (parked resonances, pathological corners) and is
FORBIDDEN to work on ordinary program by law
(the_limiter_is_idle_on_program: moderate kicks across the dial
corners must peak below it; engagement on normal material is a
shipped defect by definition). The peak contract restates the old
0 dB law: bounded at 1.42 at every hostile corner, never "too far
over." RESULT at the field's mid-bass recipe: nonlinearity share
0.39 (first report) → 0.0009 (drape, bass 1.0) — the limiter now
touches nothing you'd play.

## THE DIMENSIONALITY SLICE (slice 21, shipped, 0.16.0)

The maximum-depth brief ("jumping out of the speakers", "wider
than imaginable", "maximum front-to-back"), scene-derived:
* DISTANCE (id 21): the stage walks front-to-back (y 0.34-0.84;
  0.7 = the old fixed voicing). One geometry moves every cue
  together: far = WETTER relative to the host-fixed dry (r0/r →
  1), near = the INTIMACY GAP opens (a close source in a big room
  has maximal delay before its first reflection). Laws pin both.
  Two metric lessons en route: the D/R cue lives relative to the
  DRY, not inside the wet (wet-internal balance read nothing at
  mix 1); and instant modal onset masks gap detection twice over
  (a resonator answers the first sample — real modes build from
  reflections, a recorded approximation).
* THE IMPOSSIBLE HEAD: above WIDTH 1.0 the EARS move apart in the
  scene (up to ~4x head width) — every ITD, ILD and shadow in the
  ER and tank feeds widens GEOMETRICALLY, not as a post M/S
  trick. Law: the early field's raw interaural difference grows.
* PER-PATH AIR ABSORPTION: HF dies with each reflection's path
  length — front-to-back geography inside the early field (was
  bounce-count only). Honest note: darkness-with-distance is NOT
  robustly true wet-only (composition shifts outweigh the ~1 dB
  gradient — reported, not asserted, pending a per-path metric).
QUEUED from the dimensionality plan: the depth battery proper
(contribution profiles, recession trajectory, per-band-per-time
IACC matrix, crest-reduction "natural compression"), elevation
painting on ceiling taps (Blauert bands), material re-radiation
(walls that sing back).

## THE DEPTH SUITE (slice 22, shipped, 0.16.1)

tests/depth.rs — depth as numbers. FIRST READINGS:
* CONTRIBUTION PROFILES (the field's own isolation idea): the
  ER/late handoff is textbook — ER bright (2015 Hz) and gone by
  120 ms; the late field BLOOMS (-52 to -46 dB) then recedes in
  level and brightness together (1881 -> 1221 Hz).
* RECESSION: full-wet centroid 1818 -> 939 Hz across 1.2 s; law:
  the tail recedes even at open damp, and damp steepens the walk.
* THE COHERENCE MATRIX — the big finding, now precise: low band
  0.89/0.70/0.70 (anchored); MID AND HIGH FLAT AT 0.05-0.14 FROM
  THE FIRST WINDOW. No early-coherent -> late-diffuse evolution
  exists — the early field is maximally wide instantly, which is
  the anti-"jumping-out" signature. THE next engine slice, with
  numbers attached.
* NATURAL COMPRESSION, measured: burst-train crest 8.7 -> 7.2
  through the wet — real glue, linear in origin, modest at mix 1.
TWO METHOD FINDINGS: a deeper LF side-fold by subtraction is
phase-limited (two poles rotated -54 deg and measured WORSE,
0.71 -> 0.56 — the tank's phase-complementarity lesson,
feed-forward edition; proper crossover queued); the one-pole
anchor's honest ceiling is ~0.70 vs real rooms' ~0.9.

## THE COHERENCE SLICE (slice 23, shipped, 0.17.0 — worklist 1+2)

The ER now folds partially toward mid before the wet sum (er side
x er_side, er_side = 0.45 + 0.55·(width-1) clamped — opens fully
above WIDTH 1 so the impossible head keeps its hyper-wide attack);
the tank stays untouched, so coherence EVOLVES early→late by
construction. Matrix (band × window): mid 0.08→**0.60** early /
0.10 late; high 0.14→**0.57** early / 0.06 late.

The LF anchor became TWO SEQUENTIAL mild one-pole folds with
independent states (0.86 @ 380 Hz, then 0.84 @ 220 Hz). Two
findings en route: (a) depth saturates — 0.72→0.86 fold depth
moved late low IACC only 0.74→0.77, because a one-pole LP is
already −45° at its corner and subtraction can't cancel where
phase rotates; (b) the fix is CORNER PLACEMENT — putting both
corners ABOVE the measured band (380/220 vs 250/140) pushes the
phase-rotated region out of it: late low IACC 0.70→**0.84**
(accept 0.78, real rooms ~0.9). Glue crest ratio 0.83→0.77 (the
folded ER integrates sooner), pin tightened to 0.85.

New law: `the_attack_localizes_then_envelops` — early mid/high
IACC ≥ 0.4, late ≤ 0.3, early > late + 0.2, and hyper-width must
reopen the ER's side measured as side/mid ENERGY (>2× natural) —
NOT IACC, which takes |corr| over ±1 ms and reads a near-antiphase
hyper-wide attack as "coherent". Anchor pin raised 0.65→0.78.

## THE PROMISES SLICE (slice 24, shipped, 0.18.0 — worklist 3)

Materials now declare a per-band T60 contract: `bass_mult` (had
it) plus `hi_mult` — WOOD 0.80, PLASTER 1.00 (baseline exact),
STONE 1.05, GLASS 1.20, DRAPE 0.50, METAL 1.35. The tank realizes
the high promise as a THIRD first-order shelf in the existing
minimum-phase cascade (complement-of-LP form, xover 2.5 kHz, above
the bass shelf so the top band's loop gain is g·ratio_hi exactly);
the passivity bound now covers ratio_hi.

Law `materials_keep_their_promises`: per material, absolute band
T30 normalized to PLASTER'S SAME BAND vs the declaration (±35%
low, ±40% high — damp_add composes on top). First draft normalized
to each material's own mid band and drape's low promise vanished
(damp_add shortens the mid REFERENCE — measured 0.97 vs 0.65);
the declaration means "this material's 125 Hz vs the baseline's".
Measured: wood 0.87/0.88, stone 1.31/1.13, glass 0.75/1.23, drape
0.68/0.67, metal 1.08/1.36 (decl 1.15/1.35). PLATE metal band
report now reads 2.89 @63 → 1.56 @4-8k (HF sizzle rings on, the
plate signature).

## THE ELEVATION SLICE (slice 25, shipped, 0.19.0 — worklist 4)

Ceiling-touched images (per-tap `elev`, the z-axis far mirror) sum
separately per ear and pass a SHARED height shelf — +3 dB above
6.5 kHz (complement-of-LP, the Blauert "above" band); floor
arrivals stay swallowed. Two shelf states per ear, negligible CPU.

Law `the_ceiling_shines_down` lives IN-CRATE (room.rs laws mod —
field access makes render-difference isolation surgical): zero the
ceiling taps, zero the floor taps, subtract each from the full
render, compare contribution centroids. Ceiling > floor × 1.25.
VERIFIED the law referees the SHELF, not the material contrast:
with ELEV_GAIN forced 0 it fails; restored, passes. First draft
compared ceiling vs "everything else" and failed honestly — the
non-elevated set includes bright first-order WALL taps (2951 vs
3545 Hz); the acceptance's contrast is ceiling vs FLOOR.

## THE RE-RADIATION SLICE (slice 26, shipped, 0.20.0 — worklist 5)

engine/rerad.rs — walls that SING BACK: per-material body
resonators driven by the per-ear EARLY FIELD, output joining the
ER path (so the ring wears the same early gain, coherence fold
and lean as the reflections that excited it). Declared voices:
METAL 830/1373/2207/2957 Hz (inharmonic, T60 0.40-0.55 s), WOOD
163/271/389 (panel thumps), GLASS 1130/2090/3610, STONE one faint
1070 Hz knock, PLASTER/DRAPE silent. Driven-gain-flat k =
2(1−r)·sin(w) (the modal lesson — sustained energy cannot
integrate a high-Q body to a scream). LINEAR by construction —
the linearity laws stay green; rattles remain the future life
layer. RERAD_LEVEL 0.20 (0.12 measured 1.98× vs the 2× line).

Laws: `the_walls_sing_their_declared_voice` (in-crate: metal
rings AT 1373 Hz > 4× off-peak, decays, plaster silent) and
`the_metal_sings_its_body` (integration: metal-vs-plaster render
diff shows a spectral PEAK at 1373 Hz > 2× its neighborhood —
peak-vs-neighborhood discriminates the ring from the shelf
differences that also live in the diff). Timbre ordering and the
full battery hold (71 green).

## THE HIGHER-ORDERS SLICE (slice 27, shipped, 0.21.0 — worklist 6)

32 stochastic order-3+ taps (16/ear), delays HASHED per ear,
cumulative absorption wall^order (order 3-4), true 1/r, size-gated
(silent and skipped below size 1.5, full by 2.2 — small rooms'
order-2 field is already dense). Input delay capacity 0.39→0.52 s.

TWO FINDINGS RESTATED THE ACCEPTANCE (measured truth outranks the
sketch):
* The sketch's window [max order-2, +3 mfp] was INAUDIBLE — 1/r
  gains ~0.06 out there and the tank already owns the region
  (NED/flutter unmoved). The starved region is the MIDDLE of the
  early field; the window is now [0.25·max order-2 gap, max gap +
  1.5 mfp] (42-190 ms at size 2.7), where the taps add +22% ER
  energy (0.01066 → 0.01298, render-isolated).
* Full-render NED@25 and flutter CANNOT move by ER taps at
  cathedral scale: the midfield already owns statistical density
  (tank-only NED@25 reads 1.44 vs full 0.483 — the honest big ER
  spikes DEPRESS windowed NED), and flutter is the TANK's line
  periodicity (tank-only 0.656 ≈ full 0.630). Mode density stays
  the recorded cure.

Laws: `the_higher_orders_extend_the_skeleton` (in-crate: +15%
window energy floor, gate holds at size 1, zero phantom orders)
and `the_cathedral_holds_its_shape` (full-render guard pins:
flutter < 0.72, NED t90 < 0.16).

## THE ROBUSTNESS SLICE (slice 28, shipped, 0.22.0 — worklist 7)

tests/robustness.rs: block-64-vs-1024 invariance (windowed tail
energies within 1 dB — passed unchanged) and the automation click
hunt. METHOD: a NOISE bed masks its own clicks (all six dials
"passed" the first draft); a 220 Hz TONE bed with an HP-2k
detector and a spike-vs-BOTH-regimes criterion caught everything —
all six swept dials clicked 2.3-39× (damp 2.3, tone 5.5, material
5.0, bass 9.6, decay 7.5, size 39 — the audit ledger's block-step
prediction, quantified).

THREE GLIDE LAYERS LANDED (each measured before the next):
1. THE DIAL GLIDE (engine/mod.rs): every tune-time scalar — size,
   t60, damp, tone, bass, stand, distance, diffuse, stiff, and
   the material CONTINUUM (wall gain/cutoff, bass/hi mults,
   damp_add) — walks to target over ~40 ms; snapped on reset.
   Cured damp + tone; the rest still clicked through coefficient
   STAIRCASES at the application points.
2. PER-SAMPLE COEFFICIENT GLIDES: room tap GAINS (delays always
   glided, gains stepped — the ledger item verbatim) and tank
   loop gains g/ratio_low/ratio_sub/ratio_hi (a per-block step on
   a recirculating signal is a zipper). Cured material, bass,
   decay. The servo bound covers max(g_old, g_new) mid-glide.
3. THE MIDFIELD MIX GLIDE: size still clicked 17× — the spike
   census showed BLOCK-PERIODIC hits at intra-block offset 143 =
   the lookahead limiter's 144-sample signature pointing at a
   per-block step in the wet; the midfield's gate×diffuse blend
   was stepped per block against a large specular-vs-scattered
   difference. Per-sample mix glide: size 16.7 → 0.97.

All six dials now under 1.5× (0.77-1.37). Laws:
`the_sound_survives_any_block_size`, `dials_step_without_clicks`.

## THE TOPOLOGIES ARC (ratified 2026-07-21)

THE RATIFYING CRITIQUE (field): "changing the materials and
controls just doesn't do enough to change the timbre... it always
sounds like the same kind of room... no way to get a plate or
different sounding spaces that are convincing" + CPU at 5-6% in
the DAW. DIAGNOSIS: one skeleton, many shaders — every preset was
the same 16-line FDN chord (BASE_MS × size = one chord,
transposed), same ER pattern, same AP ratios; materials shade
per-band gains OF THE SAME PATTERN. The constant comb+AP
fingerprint IS the "plastic/metallic" constant character. The old
"PLATE" preset was the room FDN with the ER muted. And the CPU
answer is the same answer: stop running geometry stages a space
doesn't have.

THE ARC: 1. [x] PLATE skeleton (slice 30). 2. [ ] ROOM chord from
the scene — DESIGN SHARPENED: deriving tank line lengths from the
scene only helps if the GEOMETRY itself can vary, and today every
dim is one SIZE dial times fixed constants (W:D:H aspect is nearly
frozen — scene-derived lengths would be the same chord transposed,
the exact disease). So the slice is TWO moves: (a) a SHAPE dial
(id 23) that walks the aspect corridor↔shoebox↔tower and feeds
Scene::describe's DIMS — which re-voices the ER, the modes, the
mfp and the midfield through the one description; (b) tank line
lengths become AXIS-ASSIGNED round-trip times (lines split across
W/D/H with irrational jitter) so proportions change the CHORD —
and H^0.85 means big rooms already SPREAD the chord instead of
transposing it. ACCEPT: two same-size different-shape renders
differ audibly in the band report and late spectral ridge
positions; all T30/class pins re-measured. 3. [ ] SPRING (1D
dispersive chain — the plate skeleton with 1-3 long dispersive
loops is most of it). 4. [ ] ECHO/CHAMBER (few long lines,
audible repeats).

## THE SUPERSTIMULUS ARC (ratified 2026-07-21)

FIELD BRIEF: "nonlinear/impossible places or impossible
optimizations that make the sound really jump out of the speakers
with fidelity, width, height, depth — rather than just modeling a
real thing further." The arc: 1. [x] LAUNCH+BLOOM+LIFT (slice 35).
2. [ ] THE ROOM INHALES (program-driven geometry: loud pushes the
walls out — duck generalized from gain to SPACE; the glide
machinery is already click-proof). 3. [ ] THE DETONATION CHAMBER
(a drums topology: onset-triggered per-hit field envelopes —
bloom-then-cut, whip, silence stays black between hits).
4. [ ] THE REMEMBERING FIELD (tonal topology: a large modal bank
charged by the program — the chord still ringing in the room, no
transient smear; optionally pitch-locked). Candidates on record:
nested rooms (each reflection order in a different-sized scene),
anti-masking tail (band gains steered away from the dry's
spectrum).

## VISIBLE IS NOT CLICKABLE (slice 48, shipped, 0.37.1)

FIELD: "ascend/fuse/dawn are not movable in the ui." ROOT CAUSE
(frame-dumped): `Fill` height inside a `Stack` SOLVES TO ZERO —
the four machine pages were 0-tall containers whose knobs still
DREW (macOS does not clip subview drawing) but sat outside their
parent's bounds, where hitTest CULLS them. Visible, unclickable —
every page dial (whip, ascend, fuse, dawn) since the paged
surface landed. Screenshots verify GEOMETRY, not interaction —
the gap that let this ship.

FIX: pages carry an explicit height (PAGE_H). VERIFIED BY REAL
CLICKS: instrumented mouseDown + stave-play + cliclick —
standard-row knobs fired, page knobs did not (reproduced), then
did (fixed). THE INTERACTION HARNESS is now a known method:
activate the live window first (un-activated, synthetic clicks
land on other apps — caught mid-probe and stopped).

## THE TIMBRE GATE + MEMORY GETS ITS CLOUD (slice 47, 0.37.0)

FIELD: "memory sounds really terrible — awful metallic with
artifacts. Did you test these? Did they meet important criteria?"
THE ANSWER, OWNED: intent laws yes, QUALITY criteria no — the
metal meter (built when the plate shipped ringing) was never run
against the three new machines. Same mistake class as plate v1
and superstim v1: new intent laws, no quality gate.

THE GATE (tests/timbre.rs): the metal meter now runs on EVERY
topology as a law — late-tail flatness ≥ 0.10, comb ≤ 0.60,
broadband-sustained excitation (fair to charge machines). First
run convicted MEMORY at **flatness 0.000** — a pure line spectrum,
the mathematical definition of a bell. (VOID measured best in the
plugin: 0.426/0.060; DETONATE healthy.)

THE FIX — THE CLOUD: a discrete resonator bank cannot be a wash
(drift at sane depths is cosmetic next to 4% line spacing —
measured 0.000 unchanged); the freeze-class texture comes from
MODULATED-DELAY SMEAR. Memory's output now passes a deep slow
4-voice cloud (16-28 ms centers, ±13-20 ms, 0.18-0.52 Hz;
DIFFUSE is the cloud amount; the R voices converge onto L's
delays as WIDTH closes — independent voices broke width-0 mono,
corr 0.79 measured). Flatness 0.000 → **0.197, comb 0.246 —
better than the room itself.** Chord law re-instrumented with
BANDED probes (±3% — the cloud smears each partial into a
breathing band; exact-frequency probes read the line's absence,
not the chord's). All memory laws green: chord 9.2x, T60 2.08,
level -0.4 dB, ascend 1.13.

STANDING RULE: every NEW topology ships through the timbre gate.
99 laws.

## THE VOID (slice 46, shipped, 0.36.0 — topology 5)

The wall-less field: no ER, no geometry, no modes — sixteen lines
far beyond any room (55-150 ms geometric), six-stage input
diffusion, two in-loop APs per line, baked sheen. DAWN (id 28,
THE VOID page): a send-side swell — sources RISE INTO the field
instead of splashing against it (front/back burst energy 0.015 →
0.001 across the dial; bit-absent at 0). DECAY honest at long
holds (5.4 s on a 6 s dial), early crest 4.1 (a wash, no
arrivals), width-0 mono, level +1.2 dB, **385 ns = 1.8%**. Preset:
OPEN VOID. Instrument lesson: onset centroids get diluted by the
field's own tail — front/back energy ratio is the swell's honest
meter.

FIVE MACHINES: ROOM · PLATE · MEMORY · DETONATE · VOID. 97 laws.

## THE DETONATION CHAMBER (slice 45, shipped, 0.35.0 — topology 4)

DETONATE: reverb as an EVENT SYSTEM, built for drums. Every hit
(ratio-detected onsets, the click-battery's detector) triggers a
BURST: a dense chamber (16-line geometric ladder 12-48 ms, input
diffusion, in-loop APs) opened by a per-hit envelope. The internal
field rings at a fixed generous T60 so retriggers reopen onto a
LIVE field — fast hits chain naturally. Between bursts THE
SILENCE STAYS BLACK: measured -868 dB (bit-silence; the envelope
owns the output and the cut is cosine-closed, click-free).

THE FUSE (id 27, THE CHAMBER page): the burst's shape — 0 =
instant detonation decaying away, 1 = a reverse swell that builds
INTO the cut. Measured: burst-energy centroid sweeps 0.036 s ->
0.274 s across the dial. DECAY is the burst's LENGTH (0.14 s at a
150 ms dial, 0.56 s at 900 — the envelope owns the tail, not a
T60). All four laws first-run green; level +0.4 dB on hits;
**333 ns = 1.6%** (budget law at 600). Preset: GATED SNARE.

TOPOLOGIES: ROOM · PLATE · MEMORY · DETONATE — four machines,
four different physics. 93 laws.

## THE REMEMBERING FIELD (slice 44, shipped, 0.34.0 — topology 3)

MEMORY: the tail is not delayed copies — it is THE CHORD STILL
RINGING. engine/memory.rs: 126 resonators (18/octave x 7 octaves
from 55 Hz), charged driven-gain-flat (the modal lesson — an
impulse cannot integrate a high-Q body, so the no-transient-smear
claim is PHYSICS, not processing). Input diffusion softens the
strike; per-resonator hash pickups spread with WIDTH (0 = mono by
geometry); SIZE slides the whole grid deeper; DECAY is the hold
(honest T60), LOW/HIGH ENERGY bend it per band, TONE tilts the
memory's brightness.

THE ASCEND LADDER (id 26, MEMORY page): each resonator leaks its
ring into charging the one an octave up — the remembered chord
climbs as it sustains. PHYSICS LESSON EN ROUTE: a LINEAR leak
cannot transpose (the 220 ring drives the 440 body off-resonance
— measured a dead ladder, 0.000); the leak is RECTIFIED (|y| is
rich in 2f and amplitude-linear, so the octave breathes with the
memory). Deliberate, honest nonlinearity — the impossible arc's
kind. Feed-forward up the grid: unconditionally stable.

MEASURED: chord ring 427x over off-grid probes after the source
stops; tone-vs-transient tail asymmetry ~10^5; T60 2.01 on a 2.0
dial (first try); ascend octave/fundamental 0.00 closed -> 0.90
open; width-0 mono; level +2.7 dB; **308 ns = 1.5%** (budget law
at 700). Presets: FROZEN CHOIR (ascend 0.75, 20 s hold), THE
MEMORY. Covers the Frozen/cloud demand AND the shimmer demand in
one machine that is ours alone.

## THE IMPOSSIBLE FOLDS IN (slice 43, shipped, 0.33.0)

The three superstimulus dials died (ids 23-25 RETIRED) and their
behaviors moved inside the dials whose names already promised
them — aggressively, full ranges:

* DISTANCE below 0.5 engages THE LAUNCH, full at 0: "right in
  front of you" now means the image steps OUT of the speakers —
  the acoustic near field and the psychoacoustic projection are
  one dial, one story.
* DUCK drives THE BLOOM 1:1: the wet ducks under the hit and
  re-blooms both louder AND wider into the gaps (measured at full
  dial on a continuous bed: -5.6 dB trim — that IS ducking — with
  the field 2.6x WIDER and zero darkening).
* LIFT is BAKED at 0.45 + 0.15·size (0.6 at size 1, ~0.9 at 3):
  every vault tail rises; tall spaces rise more — the house
  signature, scaled by the architecture.

LAW LESSONS: a baked behavior has no render pair to law — the
rise is now part of the baseline every other law pins (cross-size
comparisons measured the room changing, not the lift). Distance's
level/tonality are its own acoustics (character.rs pins them);
the do-no-harm battery keeps only the surprise clauses. Width
re-pinned 1.3 → 1.2 (the always-rising top is side-rich and lifts
the width-1 baseline).

THE SURFACE, FINAL FORM: MACHINE · SPACE · ENERGY · OUTPUT + one
page per machine. Every knob audibly earns its cell. 82 laws.

## THE FIELD'S GROUPING (slice 42, shipped, 0.32.0)

User-specified card layout: SPACE (predelay, size, width,
diffuse — where you are), ENERGY (decay, low energy, high energy,
tone — how the place holds sound; "decay" renamed to ENERGY
family: low decay → LOW ENERGY, high decay → HIGH ENERGY), OUTPUT
(mix, duck, output — the plumbing). MACHINE holds the topology
selector alone. THE IMPOSSIBLE unchanged. 84 laws green, both
pages verified.

## THE SURFACE SPEAKS PLAINLY (slice 41, shipped, 0.31.0)

FIELD (two turns): "bass/tone/damp all sound like an EQ" + "stiff
and motion have no audible effect — do we even need them?" The
user then derived the honest names themselves: these are DECAY
dials, not filters.

* RENAMED (ids frozen, display names changed): bass → **LOW
  DECAY** (exact truth: T60 x dial below ~350 Hz), damp → **HIGH
  DECAY** (felt truth: absorption per bounce — compounds, so the
  top dies faster AND darkens progressively; the legend carries
  the nuance). TONE stays TONE (it is genuinely color, not time —
  visually separating the two time dials from the one color dial).
  GOTCHA: knob_cell strips "lo "/"mid "/"hi " prefixes (a
  multiband band-knob convention) — "lo decay" rendered as
  "DECAY"; full words dodge the framework convention.
* MOTION BAKED (id 8 retired): delay-mod is near-inaudible on
  noise/percussive material by physics (noise is self-similar) —
  it is anti-metallic HYGIENE, not a musical decision; users only
  notice its absence. Fixed at 0.35 both machines. The metal-ring
  law re-pinned 2.0 → 1.7 (an always-moving tail legitimately
  broadens late peaks).
* STIFF → **WHIP**, plate-only (id 16 renamed): on the plate it
  is the defining voice, law-pinned; on the room it was an
  impossible-material color nobody would find. The room's tank
  dispersion cascade DELETED (fields, consts, tick block, group-
  delay comp). THE SHEET card now holds the plate's one proud
  dial.

VOICE = HIGH DECAY · LOW DECAY · TONE · DIFFUSE — two time dials,
one color dial, one texture dial, each named for what it does.
84 laws green; both pages screenshot-verified.

## MOTION AND STIFF GO LOUD + A PERF CORRECTION (slice 40, 0.30.0)

FIELD: "neither has enough effect even maxed." Voiced hot, laws
green throughout: room MOTION span 9 → 26 samples (the tail at
max is deep water, not a shimmer); plate SHEEN span 2.6 → 7;
room STIFF cascade 6 → 10 stages (gated at 0 — no default cost);
plate whip pole ceiling 0.84 → 0.88.

PERF CORRECTION (owning it): slice 39 claimed the tape purge
halved CPU to 489/247 ns and lowered the ratchets. That reading
was a MACHINE-STATE FLUKE — the committed build measures ~848 ns
repeatedly on a fair machine, same as before the purge. One run,
ratcheted immediately: exactly the slice-29 lesson ignored.
Ratchets restored (1000/600); the standing rule gains teeth:
NEVER ratchet on a single session's reading.

## THE TAPE PURGE (slice 39, shipped, 0.29.0 — CPU CLAIM CORRECTED ABOVE)

FIELD: "remove the tape stuff — it's orthogonal to the product."
DELETED end to end: WOW (id 9) and DRIVE (id 10) — params (ids
RETIRED, never reuse), the wander walk, the in-loop saturator +
its T60 servo (sat_in/sat_out/red/comp), WOW_VEC, THE TAPE card,
preset columns ("TAPE WAREHOUSE" → "WAREHOUSE"), three laws
(wow_smears, drive_thickens, drive_keeps_the_dialed_decay) and
the drive corners of the passivity sweep.

THE SURPRISE: the wander ticked PER SAMPLE even at wow 0 — the
purge cut the room 841 → **489 ns (2.3%)** and the plate → **247
ns (1.2%)**. Ratchets lowered 1000→700 / 600→400. The room now
sits inside the CPU target the optimization slice declared
unreachable — the cost was never the reverb, it was the tape.

86 laws green. MOTION stays (the tail's water — a room quality,
not a tape artifact); future lofi character belongs to a
dedicated machine, not dials bolted onto the room.

## THE IMPOSSIBLE GOES LOUD (slice 38, shipped, 0.28.0)

FIELD: "too subtle, even maxed out." All three voiced HOT, with
the guardrail laws steering every step:
* LAUNCH — ILD tilt 0.8 → 1.5, in the ENERGY-TRUE form
  (1+x)/sqrt(1+x^2): the raw 1+x form grows as 2+2x^2 (+2.5 dB
  measured) and past x=1 the contralateral tap FLIPS POLARITY —
  the antiphase content was the coherence kill (IACC 0.14). Cap
  x at 0.93: deep shadow, exact unit energy per tap pair, no
  flips. Presence tilt 0.55 → 0.75. Program: side/mid 2.4x,
  hi 1.7x, +0.6 dB. The coherence pin moved to the FIRST 20 ms
  (precedence rides the wavefront; the deep tilt legitimately
  spreads what follows) at ≥ 0.3.
* BLOOM — boost 1.25 → 2.1: after-hit side 2.3x over baseline,
  program-wide 2.4x wider, attack still bit-untouched.
* LIFT — 2.6 → 4.6 with the shelf dropped 6.5 → 5.2 kHz: late
  Blauert boost 9.1x (+9.6 dB), trajectory 1.95x.
Every change re-refereed by the do-no-harm battery (level within
1.5 dB held throughout — the energy-true tilt exists because the
battery caught +2.5 dB).

## BLOOM GOES ADDITIVE (slice 37, shipped, 0.27.2)

FIELD: "bloom sounds like damping and narrowing" — and the v2
MECHANISM was subtractive: "focused attack" implemented as side
REMOVAL, and deleting side energy is indistinguishable from
damping + narrowing because that is literally what it is. BLOOM v3
INVERTS the polarity: the attack stays bit-natural and the tail
blooms WIDER than the room after every hit (a post-hit side ramp,
rise = the dial 80-580 ms, reset on onsets; +1.25 side boost at
full ramp). Nothing is ever removed. Measured: during-hit field
UNTOUCHED (0.15 -> 0.15), after-hit side 1.7x, program-wide
side/mid 1.75x WIDER, level +0.7 dB, hi-share 1.11x. The intent
law flipped to match: after-bloom exceeds baseline AND the attack
is untouched. LESSON: superstimulus dials should be ADDITIVE —
subtracting anything from the wet reads as damage, not effect.

## THE DO-NO-HARM REDESIGN (slice 36, shipped, 0.27.1)

FIELD VERDICT on slice 35: all three dials "sound like lowpass
filtering... narrow the image... shorten decay." THE TESTING GAP,
named: v1's laws pinned each dial's INTENDED dimension on SPARSE
signals (impulses, isolated hits) and never the do-no-harm
dimensions — level, spectrum, width — on DENSE program. The
field's ears ran the missing suite first.

THE SUITE (the_dials_do_no_harm_on_program): a chord bed +
hits, each dial at 1.0 vs 0.0 — level within 1.5 dB, side/mid not
below 0.7× (bloom 0.55× — its on-hit duty cycle IS the effect;
v1's PERMANENT 0.17× fold is the forbidden thing), hi-share not
below 0.75×.

THE THREE REDESIGNS:
* LAUNCH v3 — the v2 shadow-deepening scaled tap gains AND cutoffs
  down with no compensation: literally "lowpass + quieter". Now a
  ZERO-NET ILD tilt on gain only (ipsilateral up = contralateral
  down), cutoffs untouched. Program numbers: +0.8 dB, side/mid
  2.51× (wider), hi-share 1.80× (brighter).
* BLOOM v2 — v1 folded on LEVEL; dense program never releases a
  level gate = permanently narrow. Now a shared TRANSIENT detector
  — and difference detection fired on steady tones' waveform
  ripple (chord bed read 0.17×), so it is RATIO detection
  (fast/slow > 1.35; steady ripple rides ~1.1). Release re-ranged
  50-350 ms (1.3 s outlasted musical hit spacing — chronically
  folded). Contrast 3.4 → 22.8.
* LIFT v2 — v1 gated on SILENCE, which music never provides (the
  dial did nothing on program). Now onset-RESET: every hit pulls
  the ramp down, every tail lets it rise — works per-phrase on
  real material.

LESSON FOR THE ARC (standing): every superstimulus dial ships with
BOTH an intent law AND a do-no-harm clause on dense program.

## THE SUPERSTIMULUS DIALS (slice 35, shipped, 0.27.0)

Three psychoacoustic dials in THE IMPOSSIBLE card, each pinned by
law, each BIT-ABSENT at zero (the whole 84-law battery stands on
renders with them defaulted — proven by a bitwise law):

* LAUNCH (id 23, room machine) — the image steps OUT of the
  speakers. THE LAW TAUGHT THE DESIGN: the first cut widened the
  ER's ears 2.2× and opened the coherence fold — early side/mid
  hit the pin but IACC collapsed to 0.13 = BLUR, not precedence
  (the coherence assert exists exactly for this). The shipped
  recipe: ITD inside the fusion window (ears ×1.5 early-only),
  the heavy lifting in ILD (head shadow deepened past nature,
  floor 0.55→0.22), fold opened gently (×1.6 capped at 1.0).
  Measured: early side/mid 0.179→0.412 with IACC ≥ 0.35 held and
  the late field untouched (early-only law clause). Also found:
  GEOMETRY ALONE cannot raise early side/mid (0.179→0.181) — the
  ER fold owns that number; any launch design must negotiate with
  it.
* BLOOM (id 24, both machines) — coherence choreographed in TIME:
  each hit folds the field to center (2 ms attack, 0.95 depth) and
  it opens around the listener as the dry releases (release = the
  dial, 80 ms-1.3 s). After/during side contrast 3.44→8.16.
* LIFT (id 25, both machines) — the tail RISES: a two-pole-
  complement Blauert shelf (6.5 kHz) whose gain follows a
  tail-time ramp (rises τ 1.1 s in silence, resets in 30 ms on
  program). Late-window 7-10 kHz boost 3.55× (+5.5 dB), trajectory
  steepens 1.7×. Instrument lessons: share-of-total saturates (the
  boost lives in numerator and denominator); the one-pole
  complement leaked the boost into the mid reference band.

tests/superstim.rs (4 laws). Room 841 ns / plate 433 ns.

## THE STEREO IS THE GEOMETRY (slice 34, shipped, 0.26.0)

FIELD: "the width control kind of sounds like cheap tricks for
stereo image rather than really interesting optionality integrated
into the engine components." Ratified — the post M/S width stage
is DELETED from both machines; WIDTH is geometry end to end:

* THE PLATE — PICKUPS ARE PLACES ON THE SHEET: each line carries a
  pseudo-mode shape sin(πu·x)sin(πv·y) (odd wavenumbers — all
  alive at center) and each pickup's tap weight is that shape AT
  ITS POSITION; width walks the pickups apart. Width 0 =
  coincident = corr 1.000 (mono by geometry); 1 = 0.345; 2 =
  0.285; energy constant (a moved mic never gets quieter — law).
  CAUGHT: mirror-symmetric positions barely decorrelate — for odd
  modes sin(πu(1−x)) = sin(πu·x), so L and R heard IDENTICAL
  x-weights (corr 0.82); the positions are now asymmetric in both
  axes. Second pickup set follows the same weights.
* THE ROOM — WIDTH IS EAR SPAN, both directions: above natural
  the impossible head (ears to 4×); below natural the ears
  CONVERGE (scene clamp 0.04-4.0) — and THE COHERENCE CORNERS
  TRACK THE EARS: the LF side folds are the diffuse-field
  interaural-coherence model, so their corners scale 1/ear_scale
  (wide ears open the low late field — the LATE width the deleted
  M/S boost was faking; coincident ears fold the whole spectrum
  to mid). A 5%-of-dial side gate finishes the coincident limit
  exactly: width 0 ships BIT-mono wet (law kept).
* RE-PINS: side/mid at width 2 = 1.4× (the geometric truth; 2.0×
  was the boost's number — the raw-IAD law pins geometry growth
  separately). Width joined the automation click battery — the ER
  fold scale and corner coefficients needed per-sample glides
  (2.85× click → 0.96).
* THE CHIRP LAW, third instrument: energy centroids and sub-ms
  envelope-peak ordering both proved instrument-noise against
  pickup weighting; the law is now SIGN (lag grows with STIFF) +
  AUDIBILITY (stiff 0-vs-1 render-diff = 1.31 of the tail) +
  an IN-CRATE absolute proof on the cascade alone
  (the_cascade_runs_the_top_first). Per-line pole stagger DELETED
  (dispersion is a material property; stagger made the chirp
  hostage to pickup luck — the meter confirmed the in-loop
  diffusers + geometric ladder already own the zing).

84 laws green; room 834 ns, plate 431 ns.

## THE ONE-BINDING SURFACE (slice 33, shipped, 0.25.1)

FIELD: the SHEET card's knobs would not turn. CAUSE: slice 32
bound the same params on both pages (WALLS/MOTION and THE SHEET)
— the mount layer keys controls by param, so the second set
renders but is DEAD. The bindings gate checks name-binding, not
duplication; the design now makes the class impossible: every
dial lives in EXACTLY ONE always-visible place — the MACHINE
spine + a VOICE card (damp/bass/tone/diffuse/stiff/motion) — and
a per-machine LEGEND line under it says what they mean here
("DAMPER · BASS RING · PICKUP · STRIKE SMEAR · THE WHIP · THE
SHEEN" on the plate). Pages hold ONLY what is unique: THE PLACE +
THE TAPE for the room; a one-line description for the sheet.
Screenshot-verified both pages. RULE FOR THE ARC: a param may
appear ONCE in the tree; polymorphism is the legend's job, not a
second knob.

## THE METAL METER + PLATE V3 + PAGED SURFACE (slice 32, shipped, 0.25.0)

FIELD VERDICT on slice 31: still "very metallic and not in the
good-plate way", and the hole-based UI "sucks" (it also OVERFLOWED
— the 6-cell SPACE card pushed OUTPUT off the window edge,
screenshot-confirmed). Both rebuilt.

THE METAL METER (tests/plate.rs) — "bad metallic" as two numbers
on the late tail (0.35-0.85 s, 400-6000 Hz):
* SPECTRAL FLATNESS (geomean/mean of power): wash reads high,
  ringing partials read low;
* COMB REGULARITY (autocorrelation of the log-spectrum ACROSS
  FREQUENCY, comb spacings 25-400 Hz): a periodic spectrum is a
  pitched metallic percept; real plate modes are irregular.
Calibrated against the ROOM TANK as the sounds-right reference:
room = 0.185 / 0.491. Slice-31 plate measured **0.027 / 0.756** —
7× peakier, strongly periodic: the verdict, quantified.

PLATE V3 — four measured moves (meter after each):
1. GEOMETRIC line ladder (the old table was near-arithmetic —
   equal length increments align every line's comb into a
   super-comb): 0.027→0.067.
2. IN-LOOP DIFFUSERS (2 nested integer APs/line — the tank's
   ridge-killer; the cascade rotates phase but barely diffuses
   time) + PER-LINE staggered dispersion poles (16 identical
   cascades = one zing sixteen times): part of the same step.
3. Sheen deepening: measured NO-OP (0.067→0.069) — modulation was
   not the differentiator.
4. THE LEVER — LONG LINES (27-90 ms, was 9-46): short lines put
   comb spacing at 65-110 Hz, squarely in the PITCHED-metal
   percept band; long lines sit at 11-37 Hz = texture. Mode
   density is the loop's job, onset density the input diffusion's,
   transit time is NEITHER. Meter: **0.28 / 0.44 — smoother than
   the room reference on both axes.**
Onset kept instant by a SECOND pickup set (the in-loop diffusers'
internal state, Dattorro's many-tap lesson — 32 arrivals/channel;
two extra INPUT stages measured WORSE, 0.025→0.040, reverted).
Chirp law re-pinned at the STIFF ceiling (10 ms whip at 1.0;
~3.6 ms at 0.7 is physically right for a mild sheet). Laws:
`the_late_tail_is_a_wash_not_a_comb` (flatness > 0.18, comb
< 0.55) + the four v2 laws. 440 ns = 2.1%.

THE PAGED SURFACE: a stable MACHINE spine (topology, size, decay,
predelay, mix, width, duck, output — the Valhalla lesson) over a
Stack of complete per-machine pages: THE ROOM (place/walls/motion,
three cards) and THE SHEET (one card + a legend line: "DAMP IS THE
DAMPER · TONE IS THE PICKUP · …"). Screenshot-verified both pages
via an editor-only screenshot (which caught: stacked-Column pages pushed the
hidden page's height into the visible one — the page area must be
a STACK; and the original overflow). Same param bound on both
pages is fine — one visible at a time.

## THE PLATE REVOICE + POLYMORPHIC SURFACE (slice 31, shipped, 0.24.0)

FIELD VERDICT on slice 30: "way too metallic/ringy — a cheap
Schroeder, not a plate." ROOT CAUSE: sub-ms loop lines. A 1 ms
loop is a comb every 1 kHz, and at plate T60s every peak is a
screaming Q — TRANSIT TIME IS NOT THE LOOP TIME. The Dattorro/EMT
lesson: onset density is the INPUT's job, smoothness is the
LOOP's. Revoice:
* INPUT DIFFUSION chain per channel (4 short integer allpasses,
  1.1-8.7 ms, R detuned 1.07; DIFFUSE drives its g) — the strike
  smears to instant density BEFORE the loop;
* loop lines 0.9-11.5 ms → 9-43 ms (comb spacing 23-110 Hz);
* a WHISPER of per-line motion (±0.15 samples floor — sixteen
  static lines ring, measured; MOTION deepens it: the dial is now
  POLYMORPHIC — room chorus / plate sheen);
* TONE wired as PICKUP brightness (tilt at the contact mic);
* the chirp law's mid<low ordering DROPPED on physics: a
  first-order-AP cascade's group delay saturates below ~1 kHz
  (measured 0.3 ms apart) — the audible chirp is the TOP racing
  ahead (hi<mid, hi<lo, spread > 6 ms; 16 stages bought nothing
  over 12, reverted).
MEASURED: ridge (the metallic detector) 12.6/25.4 → **4.1/5.7** —
room-class smoothness; NED instant (0.005-0.015 s); T30 1.92 on a
2.0 dial; 409 ns = 2.0%. Cold-start bug caught by the scoreboard:
a fresh plugin treated its dialed topology as a live SWITCH and
faded the first 30 ms (NED read 0.025 "slow") — activate/reset now
ADOPT the dialed topology open; the fade is for live swaps only.

THE POLYMORPHIC SURFACE (the Valhalla question, answered): generic
dials stay put and REINTERPRET (size, decay, damp = damper, bass,
tone = pickup, diffuse = strike smear, stiff, motion = sheen);
ROOM-ONLY dials (distance, early, stand, material, wow, drive)
live in fixed cells that EMPTY on the plate via show_when — the
grid holds its shape, the machine honestly has no such dial.

## THE PLATE SLICE (slice 30, shipped, 0.23.0 — topologies 1)

TOPOLOGY param id 22 (ROOM/PLATE, Space card selector; a switch
fades the wet in over 30 ms — a machine swap, not a dial).
engine/plate.rs — a dedicated 2D-membrane skeleton: sixteen SHORT
lines (0.9-11.5 ms · √size — the room's are 20-70 ms, 10-40× too
long to ever be a plate), a TWELVE-stage first-order-allpass
dispersion cascade per line (always on; STIFF rides the pole
0.35→0.80), damper absorption + DC bridge + one bass shelf,
declared T60 (loop length includes the cascade's group delay —
the AP compensation lesson carried over and the t60 law passed
FIRST TRY at 1.90 vs 2.0), Hadamard junction, true-stereo drive
points, decorrelated pickups. STATIC on purpose (a plate has no
chorus; dispersion smears the combs). Materials are INERT on the
plate (no walls): damp = the damper, bass = the raw dial.

MEASURED: **369 ns/frame = 1.8%** (the arc's CPU thesis, pinned by
`the_plate_stays_inside_its_budget` at 600 ns) vs the room's 832.
NED t90 0.005 s scoreboard / 0.015 plate-battery (was 0.075 as a
room shader — the sound the room topology structurally could not
make). THE CHIRP: the strike enters BEFORE the dispersion cascade
(injected after, the onset measured clean — the first arrival must
propagate dispersively too); arrival centroids order hi 39 ms /
mid 54 ms / low 59 ms — a 19 ms compounding "bwip". Wet level
calibrated +0.4 dB vs dry (PLATE_MAKEUP 0.36 — short dense lines
recirculate far hotter than the room tank; 2.0 leaned on the
limiter, an airbag-doctrine violation caught by the level law).

tests/plate.rs: instantly-dense, chirps-downward (energy-weighted
arrival time per band — onset-only undersold the compounding),
decays-as-dialed, holds-its-own. Scoreboard PLATE class runs the
real topology; pins re-centered (NED < 0.02, T30 2.0).

## THE CPU SLICE (slice 29, shipped, 0.22.1 — worklist 8, restated)

MEASURED: 1095 → **831 ns/frame** (4.0%), all 75 laws green,
ratchet lowered 1100 → 1000. The <2% target is RESTATED as the
architecture's floor, with the full accounting:

* Ablation (of 1095): room 580 / tank 390 / modes 24 /
  mid+diffusers 58 / rest 43.
* THE WIN — run-structured room loop: the tap array is four
  contiguous runs ([src0 L][src0 R][src1 L][src1 R]); input line,
  ear and accumulators hoist out of the per-tap work. The per-tap
  `i/IMAGES` divides and branchy routing were ~270 ns/frame.
* FOUR MEASURED DEAD ENDS, all reverted on the numbers: kernel
  read_fast (BLOCKED — stave-dsp carries `#![forbid(unsafe_code)]`
  as a frozen documented guarantee; canon outranks cycles), tank
  SoA lanes (829 → 1073 — the third confirmation that this
  machine's scalar AoS is already optimal: array indirection cost
  more than vectorization saved), predelay cubic→linear (no
  measurable change), branchless elev routing (noise).
* THE FLOOR, itemized: room reads 130 ns (96 gathers × 1.35 ns —
  machine speed), room arithmetic ~185 ns (~8 ops/tap at ~4
  ops/ns — machine speed), tank components individually measured
  lean (shelves 109, cubic 74, stagger 37, APs/junction ~170).
  Sample-axis SIMD is blocked by the per-tap/per-line IIR
  recursions; tap-axis SIMD is blocked by the gathers.
* THE VERDICT: below ~830 ns means fewer taps or fewer lines —
  trading the fidelity this worklist exists to maximize. Rejected
  on the north star. 4.0% of one core at 48k for a 96-tap true-
  stereo image-source ER + 16-line FDN + 24 modes + midfield +
  re-radiation is the honest price of the architecture.
* PROCESS LESSON: an uncommitted win was clobbered by a
  `git checkout` during a later ablation (829 silently became
  1106) — COMMIT every measured win before the next experiment.

THE WORKLIST IS COMPLETE — eight of eight, slices 23-29,
0.16.1 → 0.22.1.

## THE WORKLIST (ratified 2026-07-20 — work top to bottom, mark done)

Each item: evidence, design sketch, acceptance. Sessions resume
from here; the depth/character/acoustics batteries referee all.

1. [x] EARLY-COHERENCE EVOLUTION (0.17.0) — the depth battery's headline:
   mid/high IACC flat 0.05-0.14 from window one (should be ~0.5-
   0.7 early, diffusing late) = the anti-"jumping-out" signature.
   DESIGN: partial mid-fold on the ER path only (er side x ~0.5) —
   the late tank stays decorrelated, so coherence EVOLVES by
   construction; scale the ER fold open again above WIDTH 1 (the
   impossible head keeps its hyper-wide early field).
   ACCEPT: matrix early mid/high >= 0.4, late <= 0.3, lateralization
   laws re-pinned, glue crest ratio improves toward 0.6.
2. [x] PHASE-COMPLEMENTARY LF ANCHOR (0.17.0) — one deep subtraction is
   phase-limited (0.71 -> 0.56, measured). DESIGN: two SEQUENTIAL
   mild one-pole folds (250 Hz then ~150 Hz, independent states) —
   each phase-benign, magnitudes compound. ACCEPT: low-band late
   IACC >= 0.78 (real rooms ~0.9).
3. [x] PER-BAND T60 TARGETS (0.18.0) — materials declare per-band decay
   multipliers; the law asserts the band report MATCHES the
   declaration (Sabine-style targets, calibrated mults).
   ACCEPT: character battery gains materials_keep_their_promises.
4. [x] ELEVATION PAINTING (0.19.0) — ceiling-bounce taps through a shared
   height shelf (+~3 dB toward 7-10 kHz, the Blauert "above"
   band); floor taps stay dark. ACCEPT: ceiling-contribution
   centroid > floor-contribution centroid (render-diff isolated).
5. [x] MATERIAL RE-RADIATION (0.20.0) — walls that sing back: per-material
   resonator layer driven by wall-incident energy (metal rings
   inharmonic 0.8-3 kHz, wood thumps 150-400 Hz, glass 1-4 kHz,
   plaster/drape silent). ACCEPT: metal-vs-plaster render diff
   shows declared ring; timbre ordering law still holds.
6. [x] CATHEDRAL HIGHER-ORDER TAPS (0.21.0, acceptance restated on measurement) — stochastic order-3+ set per
   ear (hash delays in [max order-2, +3 mfp], cumulative
   absorption gains). ACCEPT: size-2.7 NED@25 up, flutter down
   vs current pins.
7. [x] BLOCK-SIZE + AUTOMATION ROBUSTNESS (0.22.0) — render at block 64 vs
   1024 (metrics match); sweep damp/material/tone mid-render and
   detect discontinuity spikes. EXPECTED TO FAIL (block-step
   coefficients, the audit ledger) — then land the coefficient
   glide (~30-50 ms) as the fix.
8. [x] THE CPU SLICE (0.22.1, acceptance restated on measurement)
   — 1095 → 831 ns/frame (5.3% → 4.0%), and 831 IS THE SAFE
   SCALAR FLOOR. See the slice record below.

## What ships in the MVP

Signal order: PREDELAY (per-channel, glided by declaration, 80 ms
exp — turning it bends like tape) → DIFFUSER (four Schroeder
allpasses per ear, lengths √size-scaled, R cascade detuned ~7% so
the ears never receive the same cloud) → TANK (8-line FDN:
irrationally-spaced line lengths 19.7–71.3 ms × SIZE, one
energy-preserving Hadamard junction, per-line absorption one-pole
+ loop DC blocker + declared per-trip T60 gain) → M/S WIDTH
(kernel ms_encode/decode — allocate the field, never mirror) →
bit-dry MIX.

The pristine↔lofi axis is deliberately four orthogonal dials, not
one knob: DAMP (absorption cutoff 18 kHz→800 Hz log), MOTION
(per-line read-head chorus, golden-angle phase spread, ~0.9 Hz),
WOW (one bounded RandomWalk walk allocated across heads by a
sign-varied vector), DRIVE (passive loop saturation).

## Design decisions with their reasons

* **Every read head glides** under one slew cap (0.02 samples/
  sample ≈ ±0.3 st): block-rate targets, per-sample linear steps.
  SIZE jumps bend pitch like tape instead of clicking; chorus and
  wow ride the same mechanism far below the cap.
* **DC blockers in every loop** — the dynamo wire lesson, paid for
  by measurement there: a lowpass-damped feedback loop is resonant
  at 0 Hz.
* **Drive is passive by construction**: |sat(x)| ≤ |x|, unity
  small-signal gain, so the tail compresses and darkens but can
  never regenerate. MEASURED: tank recirculation runs ~0.02 RMS, so
  the saturator pre-gain must reach ×25 at full drive — the polite
  ×4 first draft moved the render 2.7e-7 (inaudible by number).
* **Tap vectors are Hadamard rows 3 and 5**: balanced (no common-
  mode buildup) and mutually orthogonal (decorrelation by
  construction, not by hope).
* **T60 is a declared per-trip gain** computed from each line's own
  length — the same formula as the dynamo wire, and the same law
  shape pins it.

## The laws (tests/properties.rs — the groundwork itself)

mix-0 bit-dry; silence bit-zero; tank passivity at the hostile
corner (30 s decay + full drive + full motion + full wow, window
peaks never grow); **decay_is_the_dialed_t60** (rendered slope vs
dialed T60); predelay onset (bit-zero before the dialed time);
width 0 folds mono bit-exactly / width 1 decorrelates (mean |L−R| >
0.5× level); motion and wow proven by render-difference isolation
(deterministic renders make the claim exact); drive audible +
bounded + still dies; 300 ms room silent by 2.5 s (the anti-drone
contract, learned on dynamo). Tank unit laws: Hadamard preserves
energy; taps orthogonal and balanced.

## The road from here (each slice lands with its laws)

* **V2 — density that evolves:** move half the diffusion INSIDE
  the tank loop (density grows per round trip); echo-density
  profile law (grows for halls, flat for the plate topology).
* **V3 — per-band T60:** low/mid/high loop shelves (halls decay
  bass slower); three measured T60 promises. Plus frequency-
  dependent width (side low-fold — real rooms are interaurally
  coherent below ~200 Hz).
* **V4 — IMPOSSIBLE GEOMETRY (the uniqueness thesis):** the room
  is a description, so it can do what physics can't — walls whose
  materials are route targets, geometry modulated by envelope
  (rooms that grow with the note), a COUPLED SECOND VOLUME behind
  an aperture (double-slope decay — the Concertgebouw bloom almost
  no algorithmic verb models), nonlinear walls (loud passages open
  the room), magnetized decay memory (dynamo physics: the room
  remembers, across silence).
* **V5 — topologies:** PLATE (the MVP sound — ER bypassed, instant
  density), HALL, ROOM, CHAMBER, SPRING as an honest enum, each
  with its own laws; the color dials cut across all.
* **V6 — the patch bay + living diagram + surround:** RouteSlot
  ducking/input→decay/LFO→size; declared taps drawn as the place;
  the smear PerChannel path (8 decorrelated lines to allocate).
* **Deferred fix on record:** drive currently shortens the tail at
  performance levels (passive loss past the knee — measured ~2x
  faster early decay at drive 1); the plan is the T60 SERVO
  (per-line gain-reduction tracking folded back into the declared
  loss) so drive becomes pure color. User said move on for now.
