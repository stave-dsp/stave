# ANVIL — the impossible metalcore head

**Status: RATIFIED 2026-07-21. R1 (RHYTHM) in flight.**
Forge shapes metal; the anvil is what it breaks against. Feeds
tolex (the pair IS the rig; the impedance handshake is the shared
frontier).

## The thesis

The modern-metalcore rhythm sound (Loathe, Holy Fawn: crushing
walls, smooth-but-biting top, thick-but-focused, near-synth) is a
KNOWN recipe plus a known impossibility. The recipe (dual-rec /
5153 DNA): a screamer out front (tight lows, mid push), cascaded
12AX7 stages with blocking/cathode dynamics, cold-stage bite, a
V-leaning stack, a gripped power section. The impossibility: in
any real amp THE LOWS MODULATE THE HIGHS — one signal path, one
nonlinearity, so every chug intermodulates the bite into mud or
fizz. Anvil splits the wall BEFORE saturation (LR4), gives the
lows their own shorter, rounder cascade and the highs the full
five-alarm path, and recombines in phase. The chug cannot touch
the teeth: that separation is the "almost synth-like" quality —
and no tube circuit can do it. FOCUS is that knob.

Laws (R1): the_tight_gate (tight kills 60 Hz by ≥8 dB),
the_wall_stands (crest + THD floors: the wall is DENSE),
smooth_but_biting (2.5-5.5 k stands ≥12 dB over 7.5-12 k),
the_focus_holds (FLAGSHIP: focus cuts 80 Hz×1.5 kHz intermod
sidebands ≥8 dB — measured synth-ness), EQ sign laws (the tilt
lesson). Alias is MEASURED before any oversampling ships (the
triode's ADAA is the first line).

## Parameter ledger (ids forever)
1 output, 2 bypass, 3 gain, 4 tight, 5 boost, 6 bass, 7 mid,
8 treble, 9 presence, 10 depth, 11 focus, 12 wall.

## Milestones
R1 RHYTHM (this): boost→split preamps→stack→power, laws above.
R2 FEEL: true NFB loop (Loop kernel) w/ presence/depth inside,
sag modes (bold/spongy), dynamic tightness.
R3 VOICES: channel/voicing variants, anchors from reference
captures (the tolex corpus method, applied to heads).
R4 THE RIG: the anvil→tolex impedance handshake.

## The canon cascade (0.4.0 — shipped, then FIELD-REVERTED)

The topology survey (plugins/anvil/docs/topologies.md — KEPT, it's good
research) was translated wholesale into the wall: five gentle
stages per road, cold clipper, per-stage couplers, anode shelf,
pre-clip tilt, power-recovery into the squish. Every law and the
rig bracket passed; IM3 at focus 1 measured −19 → −44.6 dBc.

The field said no: **4–5× the CPU (~3–5% → 20%+) and the bite
and heaviness were gone.** Reverted to the 0.3.0 wall. The
lessons, so we don't relearn them:

* WHOLESALE TRANSLATION IS THE ERROR. Swapping four hot stages
  for five gentle ones + two extra shelves + recovery gain
  changed everything at once — when it sounded worse there was
  no way to know which mechanism to blame. The bisect law
  applies to voicing too: ONE mechanism at a time, ear-checked.
* THE LAB NUMBERS AND THE EAR DISAGREED WITH EACH OTHER'S SIGNS.
  Cleaner IM3 read as LESS heavy — some of that intermod grind
  IS the wall. Moving the tilt pre-clip fixed crest but the
  tubes compressed exactly the emphasis that read as bite. A
  measured improvement is only an improvement if the ear
  ratifies it (the tolex rule, restated for heads).
* Chasing a law bar with 0.5 dB voicing steps (crest 0.457 vs a
  0.45 bar) is overfitting to the test, and it spent the bite.
* The crest/ladder work also taught real mechanism facts that
  SURVIVE the revert: violent couplers (330/700) differentiate
  squares into pulse trains; post shelves spike square edges;
  the settle-bind-settle battery lesson.

Adjustments worth trying, ONE at a time on the 0.3.0 wall, each
behind an A/B render + user ear check before the next:
1. COLD CLIPPER only: bias_offset on one existing stage (near
   zero CPU, the character mechanism with the best pedigree).
2. ANODE SHELF only: −4.5 @ 1.8 k inside the cascade (one
   biquad; grooming fizz at creation instead of at the ceiling).
3. Per-stage couplers on the EXISTING four stages, canon-gentle
   (15/60/120/300 max), watching the tight law and the ear.
Not worth retrying as-was: the fifth stage, the ×5/×8 recovery
gains, the pre/post tilt migration.

## R2 FEEL part 1 — the level policy, sag, and the bloom hunt (0.5.0)

The field said the wall felt STIFF next to the Plini X ("note
bloom and tube feel... most notable on palm muting"). The
instrument agreed and went further (gain-vs-time on a τ-400 ms
decaying 82 Hz note): anvil's gain rose dB-for-dB as the note
died — output pinned to the ceiling, the decay contour erased —
while Plini's tracked the envelope.

Fixed at the RIGHT level (stave-dsp), not with an anvil hack:

* LEVEL POLICY (the mechanism/policy split): the triode's silent
  loudness calibration became explicit — Calibrated (effects,
  the default, forge/comp untouched) vs Physical (gain follows
  drive into a FIXED plate ceiling; the real-amp shape). Cells
  and cascades now REPORT small-signal gain and ceiling; anvil's
  makeup is a stage-walked computation over those reports plus
  declared constants (DI reference, target, knee width). The
  fitted-ladder era is over — three ladders broke in three
  architecture changes; a report pinned by its own law can't.
* SUPPLY NODE (sag): rectifier droop + slow recovery, per road,
  driving the stages' output frames. Measured twice before
  building it: with every stage pinned, neither knee ladders nor
  deeper cathode bias move the envelope — ONLY a moving ceiling
  can. Sag is that mechanism (droop 0.22, 30/160 ms; the draw is
  cap-smoothed — raw rectified draw sprayed AM into the fizz).
* THE LOUDNESS LAW, RESTATED: ±3 dB knob-flatness was quietly
  legislating the envelope-eraser. The field truth: knob 0 is
  the quietest point, span ≤ 8 dB, settle-back past the sweet
  spot ≤ 4 dB (real amps get quieter past noon — waveform
  asymmetry), dirt never falls, and the naked knob buys EDGE
  (h6-20 grows 11× across the sweep).

What the instruments taught (plugins/tolex/docs/corpus.md instrument
discipline, applied to feel):

* A 4-stage high-gain cascade STRUCTURALLY absorbs ~20+ dB of
  dynamics (each stage soaks its escalation before the next
  unpins) — level-bloom at RHYTHM gain is not recoverable by
  static staging; it lives in sag + the knee region at low-mid
  gain, which Physical restored honestly.
* An idempotence guard with a bad sentinel (boost_amt init 0.8 =
  the default knob) silently ran the 808 at the WRONG gain and
  cost 5.7 dB of bite — a day of ghost-chasing. Guards need
  sentinels that cannot equal a real value.
* The emphasis axis has a BIFURCATION near RHYTHM depth (+1 dB
  of tilt flipped the wall's limit cycle, bite −25.4 → −29.2).
  Voicing near a cliff is fragile; the bite margin came from the
  fizz ceiling instead (linear, monotone: 9.5 k → 8.4 k).

Numbers (44.1 k, RHYTHM): IM3 focus 1 = −57.9 dBc (0.3.0 shipped
−19; the canon detour −44.6); focus 0.7 = −48.4. Ladder: knob 0
quietest, 5 dB span, 2.8 dB settle-back. Battery settle is now
convergence-gated (no magic counts). Perf 2.6 µs DEFAULT (12.7%
budget; 0.3.0 was 1.8 µs — sag + physical staging, ~45% up,
still far from the canon detour's 4-5×). Worst alias spur
−43.5 dBc (the 2× internal-Nyquist fold family) — the steeper
decimator / 4× core remains the queued fix.

Still queued for R2: the ear verdict on THIS wall, then tail
policy (gate/expansion), true NFB loop, LTP PI, sag depth as a
FEEL param, supply-node sharing modes (per-road vs global).

## R2 FEEL part 2 — REACTIVENESS, diagnosed (research, not yet shipped)

The field: "same-y — no reactiveness switching between palm mutes
and open chords; Plini's mutes come out darker, tone and envelope
evolve after the attack." The instrument (analysis/react.py: a
damped-PM proxy and a bright open-chord proxy through both amps at
matched settings) agrees precisely:

  input differentiation (centroid, open minus PM, at sustain):
    input +65 Hz -> anvil -9 Hz -> plini +106 Hz
  tone drift across an open note: anvil ~0, plini -130 Hz
  envelope into sustain: anvil flat, plini swells +2.4 dB

Mechanisms found (each measured, none shipped yet — the first
integration attempt repeated the canon-cascade mistake, trading
the just-ratified EQ balance for the new metric, and was reverted
same-day):

* THE LADDER POINTS UP: stage 1 must run near its knee so the
  input's identity survives the front door (we ran it 20+ dB deep
  — the eraser). Demonstrated: +50 Hz differentiation with
  soft-knee staging (divider -30, drive 2..12).
* DEEP DIRT AND IDENTITY ARE PER-BAND, NOT A TRADE: the Plini
  lead holds THD dead-flat from -18 to -6 dBFS (deep limiting!)
  yet differentiates — the low band saturates constant while the
  highs ride shallower. Our split roads are BUILT for this
  (low road deep / high road soft-knee + static lift), and a
  prototype restored di_range while keeping differentiation —
  but the shallow high road stops generating the top-end edge
  (bite/presence collapsed) and the balance needs a full
  measured re-voice around it.
* THE 808 IS THE FIRST ERASER: boost 0.8 costs ~2/3 of any
  differentiation the wall can produce (screamer = the
  consistency end of a real consistency/reactiveness trade).
* WRONG-SIGN TONE TRAJECTORY: anvil notes BRIGHTEN as they decay
  (pre-clip emphasis passes more linearly as stages unclip);
  Plini notes DARKEN (edge-derived brightness dies with the
  note). Fixing the sign means moving brightness generation from
  pre-emphasis toward the clip edge itself.

Next session: integrate per-band knee depth as ONE change with
the react + band-table + ladder instruments gating every step,
then the ear.

## R2 FEEL part 2 — REACTIVENESS SHIPPED (0.6.0)

The integration the part-2 research called for, executed under
three simultaneous gates (analysis/react.py differentiation, the
chug band table vs the Plini lead, the ladder law) so no metric
could be traded away unnoticed — the discipline the canon-cascade
and first-prototype failures bought:

* PER-BAND KNEE DEPTH: low road deep (divider -12: the chug's
  density is constant at any DI — the Plini truth that deep dirt
  and identity are per-band, not a trade), high road cascade at
  the soft knee (divider -23: a mute's weak highs pass under, a
  chord's grind through), classic between (-24).
* THE EDGE IS MADE AT THE END: the high road's squish runs hot
  (+14 dB past the power knee) and skips the de-emphasis — its
  brightness is generated at the final clip, scales with how hard
  the input drives it (mutes soft, chords edgy), and dies with
  the note (the right trajectory sign).
* Stage 1 near its knee (drive 6..13.5 across the knob — the
  identity survives the front door), static parity gains
  (high +6, low +10) between auto-leveled and tracking roads.
* Makeup = the focus-weighted power mix of every road's own
  stage-walk (a classic-only model went blind when the roads'
  staging diverged) + one declared taper-tilt constant
  (0.17 dB/knob — the rms the drive taper slides inside
  saturation, which a ceiling-walk cannot see).
* BASS AND TREBLE MOVED POST-WALL: ahead of it the deep low road
  and hot squish ate the knobs (swings measured 1.5 / 0.2 dB —
  EQ before a limiter dies; the sign law caught both). Mid stays
  pre: the scoop shapes what the wall chews.

Scoreboard (RHYTHM, boost 0.8): input differentiation -9 -> +26 Hz
(input itself +65, plini +106; boost is the consistency/
reactiveness trade — ~2/3 of differentiation returns as it rolls
back). Open-note tone now MOVES (+58 Hz across the decay; sign
still opposite plini's -130 — the remaining pre-emph story, next).
Bands vs plini: pres -0.1, bite +0.8, fizz -2.8, depth -2.6.
Ladder: 4 dB span, zero inversion, knob 0 quietest. All laws +
rig bracket green.

### 0.6.0 FIELD VERDICT: rejected, reverted (0.6.1)

The field: "worse than the last attempt — palm mutes still don't
sound palm muted; no tone or attack/compression change." Engine
restored to 0.5.1. The hard lesson, second time paid: THE
SCOREBOARD GATES ARE NOT THE EAR. +26 Hz of steady-state centroid
differentiation is evidently inaudible next to whatever carries
"palm mute" perceptually — likely the ATTACK (transient shape,
compression onset) and short-time dynamics, which none of the
three gates measured. Also suspect: the tight HP (110 Hz default)
sits ABOVE the 82 Hz fundamental that palm mutes live on.

Protocol change before any third attempt: candidate configs get
rendered through the user's own DI clips and EAR-CHECKED as
bounces BEFORE anything ships — the instruments narrow the search
space, but only the ear closes it.

## THE FEEL CONTRACT (R2 part 3 — ratified direction, ranges not points)

The design philosophy, ratified in the field: primitives with
KNOWN behavior curves, composed intentionally; EE/amp design is
the floor, not the ceiling; impossible behaviors are the point,
blind compensators are the failure mode; targets live in RANGES
and the ear closes them. This amp wants dynamics AND the high-
gain metalcore baseline — both, by design.

Reference data (the user's own DI, rb riff testing: chugs with
31.5 dB inter-note troughs, mutes 98-193 Hz darker than opens):

  metric                        input    plini(ref)   anvil 0.5.1
  inter-chug trough depth       31.5 dB     9.8 dB       5.2 dB
  PM-vs-open centroid gap       -98 Hz     -58 Hz       +13 Hz
  open-note decay drift        (-7 Hz)    -130 Hz       +17 Hz
  release, first 80 ms           17 dB      2.2 dB       2.6 dB

Contract ranges (ear closes within these):
* ARTICULATION: output troughs 8..14 dB on the reference riff.
  The gap floor keeps falling after the initial release — the
  first 80 ms is NOT the differentiator (measured), the
  120-400 ms region is.
* DYNAMIC TRANSFER: limiting plateau over the top 12..18 dB of
  input (the wall's promise), then an expansion region — below
  the plateau knee, output falls at >= 1.5:1.
* SPECTRAL IDENTITY: PM-vs-open centroid gap lands -30..-120 Hz
  (sign matters: mutes come out DARKER).
* TRAJECTORY: open-note centroid drift over a decay <= 0
  (darkens), magnitude up to ~150 Hz.
* TONE FLOOR: the existing laws stand (crest, bite-fizz, ladder,
  rig brackets) — feel may not be bought with the ratified voice.

Process: the primitive atlas characterizes the
behavior curves of what we own; configurations are designed from
those curves, predicted, measured, and only then bounced for the
ear. A prediction miss means the understanding is wrong — fix
that, not the symptom.
