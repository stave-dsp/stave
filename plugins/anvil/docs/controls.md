# Anvil control architecture — the two-tier proposal

*Superseded by a later parameter-architecture proposal (not retained) and
`amplifier-model-proposal.md`. Kept as the field-problem record; its
universal async rebuild scheduler and generated contract table were
deliberately not adopted.*

*Drafted 2026-08-16, from the field session that found the WALL knob
glitching (dropout + memory spike on drag), the dead pre-wall tone
stack, and the domain confusion between them. Status: PROPOSAL —
phase 0 not yet started.*

The organizing principle: **the bottom tier is physical truth, the
top tier is declared convenience, and nothing couples anywhere
else.**

## The problems this solves

1. **WALL is three things wearing one undefined word**: it stages
   every road N dB past the push-pull pair's knee, moves the pair's
   bias class (`0.8 + wall·0.12`), and triggers a makeup recompute
   that pins loudness. Clockwise = more density at constant
   loudness, which reads as "less gain" on a knob whose name says
   nothing.
2. **Structural rebuilds run on the audio thread, per knob tick.**
   Moving bias class re-tabulates the push-pull ADAA curve tables —
   four roads × two takes × dozens of drag ticks per second, each
   allocating fresh tables while old ones are live. That is the
   measured-by-ear dropout and the memory spike. There is no
   smoothing, debounce, or crossfade; the engine treats a drag as a
   hundred consecutive amp rebuilds.
3. **The tone stack is three orthogonal biquads placed pre-wall**,
   where the all-deep wall's constant-density limiting erases them
   (measured in the 0.6.0 era: ~1.5 dB full-swing effect for bass,
   ~0.2 dB for treble — "EQ before a limiter dies"). Real high-gain
   heads place the TMB stack after the cascaded gain stages for
   exactly this reason. Presence/depth work today only because they
   already sit post-wall.

## Tier 1 — the panel

What a player touches. Automatable. Every knob must pass the glitch
law (below). One sentence of physical meaning and honest units in
the UI.

| control | physical meaning | change class |
|---|---|---|
| GAIN | preamp drive | rebuild-behind-crossfade |
| TIGHT | input HP + low-road trim (declared macro over two bench params) | block |
| SQUASH *(was WALL)* | dB past the pair's knee, **loudness-pinned** (UI shows the pinning: "+9 dB past knee, loudness pinned") | rebuild-behind-crossfade |
| FOCUS | bed↔split balance | smooth |
| BASS / MID / TREBLE | one interactive TMB stack (Yeh/Smith circuit model), post-wall | block |
| PRESENCE / DEPTH | power-section NFB shelves | block |
| SUB | difference-tone octave injection | smooth |
| EDGE | top-end character morph (harmonics-top ↔ noise-smooth-top) | smooth |
| MASTER | output | smooth |

Twelve controls — the same count as today, but every knob has one
physical meaning. The current WALL's worst sin — silently bundling
bias class with staging depth — is dissolved: bias moves to the
bench.

## Tier 2 — the bench

The hidden tab. Amp-builder territory: ~20 params, **not
automatable**, applied through the rebuild scheduler (debounced,
off-thread, crossfaded in). Organized strictly by amp section.
Every bench param is one circuit quantity with a real unit. No
macro lives down here, ever.

| section | params |
|---|---|
| INPUT | HP corner (Hz), low-road trim (dB) — the two things TIGHT conflates, separately visible here |
| PREAMP | drive floor/taper, cathode depth/τ, blocking depth + recovery (ms) per road group, coupling corners (Hz) |
| ROADS | crossover corners (250/650), guard corners/orders, bed-depth law, align allpass corner |
| POWER | BIAS class (A↔AB), pair imbalance, NFB (dB), sag depth + droop/recover (ms) |
| STACK | component-value set selector (Marshall / Fender / Mesa values into the same circuit) |
| VOICING | girth peak, modern shelf, ceilings, clip-split corner, slew rate |

Presets capture both tiers.

## How interaction stays explicit (the anti-shallow-abstraction rules)

1. **Bench params are orthogonal by definition** — one param, one
   circuit quantity. The only *physical* interaction allowed is
   inside a modeled circuit (the TMB stack's pots loading each
   other is real physics, so it is wanted, and it comes from the
   circuit solution, not from code stirring three filters).
2. **All coupling lives in the macro layer, as data.** GAIN moving
   bed depth, TIGHT touching two bench params, SQUASH's makeup
   pinning — today these are scattered through `bind()`. They
   become rows in one declaration table (`params/contract.rs`):
   each panel control lists which bench params it drives and with
   what curve. That table *generates* the param registration, the
   UI grouping, and the law harness's sweep list — docs, UI, and
   code cannot drift because they are one artifact.
3. **Every param declares its change class**, and the framework
   enforces the contract:
   - `smooth` — audio-rate ramp, zero rebuild;
   - `block` — coefficient recompute per block (biquads, tone
     stack);
   - `rebuild` — worker thread builds shadow tables, engine
     crossfades old→new over ~20 ms, old Arcs drop after the fade.
   The dropout/memory bug is precisely a rebuild-class param being
   treated as smooth-class; the taxonomy makes that a
   compile-shaped error instead of a discovered symptom.

**The glitch law** (`the_knob_never_glitches`): sweep every param
mid-render; assert no output gap beyond threshold, no audio-thread
allocation, bounded rebuild latency. Runs against the sweep list
generated from `contract.rs`, so a new param cannot dodge it.

## File / folder structure (proposed, never adopted)

*The layout below was a proposal. None of these files exist; the
shipped layout is in `plugins/anvil/src/` (see `lib.rs`).*

The code mirrors the amp sections — which also fixes an existing
lie: `power.rs` currently contains the EQ voicing, not the power
section.

```
plugins/anvil/src/
  params/
    contract.rs    ← THE table: every param — tier, unit, meaning,
                      change class, macro mappings. Single source
                      of truth; generates registration, UI groups,
                      and the law sweep list.
    panel.rs         macros → bench mappings (data, not logic)
    bench.rs         physical params
  engine/
    mod.rs           orchestration only (lanes, bind, process)
    input.rs         tight HP, trims
    road.rs          the repeated unit: preamp + pair + guards per band
    wall.rs          composition only: crossover, bed, focus, align
    tonestack.rs     placement + driving-impedance context
    power.rs         push-pull, NFB, sag (what the name promises)
    voicing.rs       post shelves/ceilings (what power.rs is today)
  control/
    classes.rs       change-class enum, rebuild scheduler,
                     crossfade swap
  tests/
    laws_knobs.rs    the glitch law + knob-truth sign laws
    laws_tone.rs
    laws_dynamics.rs
```

Two pieces land in the framework, not anvil, so tolex and the comps
inherit them:

- `stave-dsp/cells/linear/tonestack.rs` (proposed; the DSP layers have
  since been renamed `primitives/`, and this file was never written) — the Yeh/Smith TMB cell:
  third-order transfer function whose coefficients are closed-form
  polynomials in the three pot positions and the component values
  (DAFX-06, "Discretization of the '59 Fender Bassman Tone
  Stack"). Component-value constructor; pot tapers; driving- and
  load-impedance parameters; insertion loss as a *reported*
  quantity. Laws pin the response to the analytic curves at pinned
  knob settings. Cheaper at runtime than the three biquads it
  replaces.
- `stave-core` — the rebuild scheduler + crossfade swapper (change
  classes are a framework concern; every future plugin gets
  glitch-free knobs for free).

## Why a real tone stack (and where)

A real TMB stack is **one interactive third-order RC network**, not
three orthogonal bands. What "real" sounds like, concretely:

1. Knob interaction — all three pots load one ladder; mid moves the
   treble corner; the controls negotiate one curve.
2. No flat setting — the natural shape is a mid scoop (~400–800 Hz
   by component choice); the metal "V" descends from it.
3. Insertion loss — 10–20 dB that moves with the knobs, shifting
   drive into the next stage.
4. Loading — the curve depends on the driving plate impedance and
   the following grid.

Placement is post-wall, pre-power — where JCM800/5150/Recto put it,
and simultaneously the fix for the dead bass/treble knobs.
Follow-on in the same framework: presence/depth migrate from
post-EQ shelves into the PushPull NFB loop (`set_feedback_db`
already exists), which is what those knobs are in real amps.

## Execution order

- **Phase 0 (one session, no sound changes):** write `contract.rs`
  as the inventory of what exists *today*; instrument a knob drag
  (allocation counts, rebuild counts, gap depth — confirm the
  re-tabulation diagnosis with numbers); write the glitch law,
  failing.
- **Phase 1:** change-class enforcement + async rebuild/crossfade;
  split WALL into panel SQUASH + bench BIAS; glitch law green.
  Sound identical at rest (null-gated); knobs stop glitching.
- **Phase 2:** BassmanToneStack cell + post-wall placement + knob-truth
  laws re-armed. Moves voice → A/B renders and hands before
  install (THE HANDS LAW).
- **Phase 3:** file restructure, purely mechanical, gated on
  bit-identical battery renders.
- **Phase 4:** the bench tab in the editor, cards mirroring
  `contract.rs` sections.

Each phase ships independently. The two that can change the sound
(1's knob semantics, 2's stack) sit behind ear gates; the two that
must not (0, 3) sit behind null gates. Phase 0 converts every claim
in this document — including the rebuild diagnosis — into measured
numbers before anything is committed to.
