# ANVIL — the existing art (high-gain topology survey)

**2026-07-26.** Before optimizing impossible states, the canon of
what REAL high-gain amps do — from the schematics literature and
the Fractal-modeling references — audited against anvil. Sources:
the Rectifier design-concepts series (warpedmusician), Fractal
forum/wiki modeling notes, the SLO/5150 schematic lore.

## The mechanism lexicon (what the great ones share)

1. **Stages have ROLES, never uniform character.** Input stage:
   warm, full headroom, fully bypassed cathode. Voicing stage(s):
   PARTIAL cathode bypass (e.g. 1 µF ∥ enough R that only >100 Hz
   gets the gain boost) — per-stage brightening, distributed, not
   lumped. THE COLD CLIPPER: one stage biased extremely cold (SLO/
   5150: 39 k cathode; Recto V2b: "the heart of the preamp") — no
   bypass cap, undersized load, asymmetric cutoff clipping, ~49%
   H2 at drive. Driver/recovery last.
2. **Progressive coupling — the tightness IS the couplers.** Big
   coupler into stage 1 (0.022 µF ≈ 15 Hz); SMALLER couplers after
   stages 1–2 (0.0047/0.0022 µF → ~70–150 Hz), tight final slopes
   into the driver (several hundred Hz on the hottest channels).
   Lows clip early and round, then leave; the chug never reaches
   the fizz stages. (Anvil ran 12 Hz couplers EVERYWHERE — the
   vintage-loose configuration — and compensated with a lumped
   emphasis hack.)
3. **Harshness is controlled INSIDE the cascade**: the Recto hangs
   a shelving cut (~1.6 kHz) directly on the cold stage's anode —
   the harmonics are groomed at the point of creation, not swept
   up by an output ceiling.
4. **Bright caps are level-dependent** (across the gain pot: more
   bleed at low gain), and partial bypasses make brightness a
   PER-STAGE budget.
5. **The 5150's last stage "is basically a diode clipper"** —
   negative feedback to the grid makes it clip hard; third-to-last
   is the 39 k cold stage. Gain character = WHERE each hardness
   lives in the chain.
6. **Power section is a feel engine, not a volume knob**: long-tail
   PI with its own clip and imbalance (even harmonics at the onset
   of power clip), class-AB crossover texture, global NFB with
   presence/depth INSIDE the loop (presence = less HF feedback =
   more HF gain AND distortion), sag (tube vs silicon rectifier =
   spongy vs bold), and — Recto Modern mode — REMOVING the NFB
   entirely: loose, huge, with the speaker impedance printing
   through (the tolex handshake's other half).

## Anvil audit vs the canon

| mechanism | anvil today | verdict |
|---|---|---|
| staged roles | 4 identical stages | ✗ uniform |
| cold clipper | none | ✗ missing character stage |
| progressive couplers | 12 Hz everywhere | ✗ vintage-loose |
| in-cascade harshness control | output ceilings only | ✗ groomed too late |
| distributed brightening | lumped ±8 dB emph/deemph | ~ crude ersatz |
| level-dependent bright | none | ✗ |
| 808 front + blocking + cathode dynamics | house cells | ✓ |
| stack post-preamp | approx TMB | ✓ (approx) |
| PI + push-pull + NFB + sag | 1×12AU7 squish, shelf EQ | ✗ = R2 |

## The schools, beyond Mesa (amended after the wider survey)

- **British cascade (Plexi → SLO → Friedman BE)**: cathode follower
  driving the stack — the CF is a compressor whose character rides
  its load (and its grid-clip is much of "crunch"); Friedman adds a
  stage plus switchable tight caps (C45) and bright/fat coupling
  variants. MECHANISM WE LACK: the cathode-follower compression
  cell.
- **5150 school**: 39 k cold stage third-to-last; the FINAL stage
  made a near-diode-clipper by feedback to its grid. Hardness has
  an ADDRESS in the chain.
- **German school (Uberschall / VH4 / ENGL)**: SEVEN stages each
  doing LESS — the wall built from many gentle clips instead of
  few hard ones (the smoothest huge sound, very Loathe); aggressive
  frequency-shaping caps per stage; LOCAL feedback caps around
  individual stages (Uber: 39 n around stage 3 — stage-level LF
  linearization = tightness nobody credits).
- **Trainwreck school (the reactivity masters)**: no master volume,
  an 820 Ω series resistor where Marshall uses 470 k — stages
  deliberately UN-isolated, loading each other, the amp tuned to
  the edge of instability so the PLAYER is a control input;
  capacitively-coupled CF whose compression rides its load. This
  school's magic is not a curve — it is COUPLED STATE, which is
  exactly what stave's state systems (blocking, cathode, sag,
  thermal, the tolex impedance handshake) can do honestly.
- **Modern extreme (Fortin / Driftwood / Revv)**: built-in boost,
  savage input HP ("grind"), very cold stages, drop-tuning depth.
  We already live partly here (the 808 front, tight).

## The stave reflection — what is POSSIBLE and likely to sound great

1. **Recipes as data.** Per-stage voicing (coupler corner, bias
   offset, brightness, local shelf) makes a SCHOOL a declarative
   table over one law-tested engine. Fractal ships hundreds of
   models; stave can ship schools-with-brackets — and the split
   rig rides on top of every one of them.
2. **The German insight fits the brief best**: Loathe's wall is
   smooth-huge — many gentle stages (compose `RcCoupledTriodeCascade` segments:
   2+2+2 = 6 stages, no dsp change) may beat 4 hot ones.
3. **Composition beats new DSP**: in-cascade shelves, interstage
   brightness, cold segments — all expressible as cascade segments
   with anvil-side glue. Only genuinely new cells: the cathode
   follower (compression-with-load) and local stage NFB.
4. **Reactivity is our endgame moat**: Trainwreck feel = coupled
   state on many timescales. We already own blocking + cathode +
   (in tolex) thermal + excursion; R2's sag + NFB + the impedance
   handshake completes the loop no static modeler closes.

## The plan (this pass = preamp canon; R2 = power canon)

- Anvil `RcCoupledTriodeCascade`: per-stage coupler corners + per-stage bias
  offset (defaults = today, forge untouched).
- Road recipe: [warm 15 Hz] → [voicing 120 Hz] → [COLD 330 Hz,
  bias offset] → in-cascade 1.6 k shelf → [driver 700 Hz] → squish.
  Lumped emphasis reduced toward what partial bypasses would give.
- R2 (power canon): LTP PI + push-pull + Loop-kernel NFB with
  presence/depth inside + sag modes + Modern (no-NFB) switch that
  couples to tolex's impedance when the handshake lands.
