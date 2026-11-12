# DYNAMO — the impossible machine

**Status: RATIFIED 2026-07-07; M0–M6 + M8 SHIPPED by 2026-07-08
(M7 genome remains).**
FIELD FIXES 2026-07-19 (report: "blend implies parallel circuits,
not one unit" + "tones ring in the background forever like a
drone"): (1) the BLEND dry path is GONE — the machine is ONE unit
and the body is its only voice; param 54 `blend` retired (never
reuse), output is the wire pair's mid/side, whole. The laws that
used blend as a measurement isolator restate: blend-1 sets simply
vanish (body-only IS what blend 1 was), blend-0 "exciter only"
sets become SHORT-RING wires (a fast body, T60 50–100 ms, tracks
its drive — the comb is honest but light), and the sample-exact
start law now honors the wave's TRAVEL TIME (output is the
recirculation; the strike reaches the bridge one wire trip after
note-on, ~109 samples at 440 Hz). (2) THE DRONE is dead, three
cuts: the MOTOR powers down when the envelope closes (command 0 →
freewheel; before, every voice re-commanded its stale note's hz
forever, and the speed-scaled bearing noise kept its wires above
the alive floor — active() never went false, a self-sustaining
drone at dead pitches); CRACKLE pops only on a POWERED voice (the
ungated pops re-struck ringing wires forever); and a stale
DUPLICATE of the M4-era escapement push (raw `drive += esc_amp`,
double-decrementing esc_run) was excised — the very "second
oscillator" bug M6 fixed had left its corpse in the tick. New law:
a_released_machine_falls_silent (every life dial at 1.0, note
released → tail at 4 s is silent; strictly harsher than the
shipping defaults that droned).
M8: THE LIFE SYSTEM shipped (field-directed: "the liveliness of
real physics", granular controls over one master dial). Six dials
— hum, bearing, clank, crackle, tension, grit — each a distinct
physical subsystem, each a function of machine STATE, each voiced
through the wires (never a parallel layer): HUM is the shared
supply's ~53 Hz ripple (one engine-mastered phase, all voices
coherent), AM on the drive plus an electrical leak, deepening as
the boiler strains; BEARING is rumble + brush hiss following the
ACTUAL rotor speed, hiss bursts landing at the commutation
scallop's brush transitions; CLANK is the contact's own collision
modes (three inharmonic partials, ~12 ms, lightly keyed — a
hammer sounds like a hammer); CRACKLE is aging pops whose RATE
rides heat + magnetization; TENSION is the real-string
nonlinearity (a loud body bends its own pitch sharp — and
legitimately detunes its drive off the wire's resonance, so bent
notes charge differently: the machine bends, it doesn't snap
back). Defaults are LIVELY; the law harness gained THE BENCH
(life dials zeroed unless a law dials one in — measurements stay
surgical while the product ships alive). Five laws, one per
subsystem.
M6: THE LIVING CENTER shipped — the machine diagram in the MACHINE
card, a pure Scene of five DECLARED taps (spin, shaft, glow,
boiler, rail): the boiler needle droops under chords, the rotor's
spokes strobe with the shaft, the wires wave with ensemble energy,
the bridge glows with rail load. Taps register in
attach_telemetry, BEFORE the editor mounts (a Scene binds probes
at mount; first-process was too late — the diagram mounted blank,
measured). Scene laws: needle-is-pressure, wires-wave-with-energy,
purity. FIELD FIXES shipped with it: (1) the escapement's tooth
now re-opens the machine's OWN exciter (a raw wire push sounded
like a second oscillator — the sustained tone must wear the
commutation's timbre); (2) the escapement engages only while the
key is HELD (chaotic voices never died; transport stop/start
layered old drones — the lift-off law pins it) and Processor::
reset() powers the WHOLE machine down (heat, pressure, lags, rail,
clocks); (3) the face tightened to 1000-wide with no bottom
overflow.
M5: MACROS + MOTION shipped. FORCE joins as the sixth macro param;
TARGETS grew by the six macros (APPEND-only) as GLOBAL-scope
targets — the engine evaluates motion once per block with ensemble
source values and applies the effective build sheet to every voice
每 block (the voicing Watch now gates only the ADSR; machine
setters are cheap and idempotent). Slots grew 4→10; slots 5–10 are
the MOTION rows, defaulting one-per-macro — Atoms' per-knob motion
as ordinary route slots, zero new machinery, and the cable bay,
rings, and inspector all picked them up automatically. The face
gained the MACHINE hero card: six macro columns, each [knob →
motion source → motion depth]. Law: a slow square into FORCE gates
the machine in time (loud/gated/loud thirds measured).
M4: THE ESCAPEMENT shipped, and it took FIVE measured corrections
to tick true: (1) the wire needed a loop DC BLOCKER first — a
lowpass-damped loop is resonant at 0 Hz, and a barely-spinning
rotor's strike left a decaying displacement hump that never
crossed zero, starving the pallet (a real string cannot hold
displacement); (2) a single-sample tooth couples ~0.1% — you push
a swing through its arc: the tooth is a quarter-period HALF-COSINE
(square edges buzzed broadband); (3) the wire's voice is its
RESONANCE — returning fed+exciter double-counted the drive and let
push edges bleed to the output; (4) the pallet needs a REFRACTORY
(one tooth per swing) or it fires every crossing and locks partial
2; (5) the pallet needs INERTIA — it senses the LOW-PASSED swing,
not the jangle (raw-body crossings locked partial 3). Result: a
struck wire self-sustains at ITS OWN fundamental (probe: 30× over
every other partial), rock-steady rms from 0.2 s to 3 s — the
impossible EBow. Tooth energy scales with wire loss (equilibrium
ring-independent) AND boiler pressure (chaotic drones self-
throttle and BREATHE with a small boiler). Past 0.55 the clock
breaks: skipped/doubled teeth. STRIKE's hammer is mechanically
separate from the motor (a strike on a stopped rotor still
speaks). Laws: sustain-vs-die, harmonic entrainment (peak-scan —
the ±4-cent pair's valley sits at the nominal), broken-clock
spectrum spread.
M3: THE BRIDGE + THE SUPPLY shipped. The bridge is a shared
soft-clipped rail (one-sample feedback lag) every voice reads
MINUS ITS OWN SHARE (self-regeneration droned, caught by the
passivity law: tail > body), with transfer scaled by the wire's
own termination loss (1−g) — bridge admittance, so loop gain is
independent of the ring dial (a flat constant regenerated at long
rings, also caught). The boiler drains with ensemble level
(steam = size; floor 0.05; recovery τ 1.2 s), starves the exciter
(0.4 + 0.6·p, exactly 1 at full pressure) and deepens the sag.
Laws: droop vs boiler size (measured on the exciter path — body
resonance growth swamped it at blend); coupled-chord passivity
(peak bound + released chord dies); sympathy as render-difference
with the strike's exciter actually dead (the 1200 ms default
release kept it driving — 'sympathy' read as 245 vs 248, all of
it the strike itself). M1's sag law now pins steam=1 (starvation
ADDS to load by design).
M2.5 (field feedback: "sounds like a modulated supersaw, and it's
mono"): CONTACT MODES — GRIND (the machine never lets go — that
WAS the supersaw), STRIKE (~20 ms of contact, the body sings:
struck-string/EP/bell territory), BOW (stick-slip friction,
self-limiting). STEREO: the detuned pair IS the field (wire A
left, wire B right, mid/side WIDTH; width 0 bit-mono by law).
GRIT: machine noise INTO the wires (env-gated; silence stays
bit-zero) at presence level (0.2 was 1.7% of RMS — inaudible as
character, measured). Laws: width split, strike decays where
grind holds (with body still ringing), bow limits without dying,
grit isolated as a render DIFFERENCE (deterministic renders make
the noise exactly extractable; spectral probes sat on partial
skirts twice).
M2: THE WIRES shipped — waveguide pair (±4 cents; the shoot-out
went to the waveguide: one delay beats six SVFs and the rotor
spinning up THROUGH the wire's partials is the attack character),
4-stage dispersion cascade (pole near +1 — the differential group
delay lives in (1−a)²; one stage measured 0.0000 ratio movement),
and MEMORY as the wire's own physics: EDDY BRAKING with a slow
charge (τ≈0.8 s) and slow demagnetization (τ≈4 s) that SURVIVES
silence — the forge tape head could not serve (AC coupling drains
remanence by design, and in-loop its 0.4× small-signal gain ate
the ring in five trips, both measured). Laws: the body rings past
the envelope; T60 at the dial; dispersion as a RATIO statement
(absolute Hz confounds with loop tuning); magnetized history
brakes a quiet note's ring 3.7× while the memory-0 control holds
0.99. The detuned pair BEATS (~1 Hz at 220) — tail windows must
sit inside the first beat half-period or they read the
cancellation null (measured 0.0000).
M0: skeleton forked from chime, full battery green from birth,
RouteSlot promoted to the kernel (third consumer). M1: THE ROTOR
shipped — mass/sag/poles/wheel params, laws pin the one-pole
spin-up (330 Hz at 1.1 s for τ=800 ms, the exact prediction), the
self-load sag landing at the declared −0.6 st, commutation
sidebands at 7f0/9f0, and the wheel's duty bite. FIELD LESSON: a
param-id collision (poles/wheel vs slot 2) passed every debug test
and killed the release factory (`table.validate()` → refused
instance, "createInstance failed 0x80000008") — the battery now
runs the factory's validation itself (param_table_valid).

A physical-modeling / evolving synth: Synplant's genetics ×
Atoms' living physics, with Stave's own spin — the simulation is
not an abstract mass-spring lattice but a MACHINE with nameable
parts, every part real physics, the assembly impossible. Product
bar: fantastic textures across harmonics, dynamics, and noise, in
both analog character (sag, drift, hysteresis, arcing) and digital
character (encoder patterns, quantized motion) — beautiful first,
clever second.

## Why a new project (the fork decision)

Opal stays the honest FM instrument — its laws are pinned to
Chowning physics and a keeper's identity doesn't mutate. DYNAMO is
`plugins/dynamo`, forked from opal's skeleton: VoiceRack voices,
the params/editor/presets layout convention, the patch bay, the
route-slot matrix. It is the THIRD consumer of the matrix/shaping
semantics chime and opal carry byte-identically — the promotion
trigger. First act of M0: promote the slot shaping (Slot /
SrcValues / shape() / evaluate scaffolding) into a shared home the
three synths instantiate (per the earning criterion, now met).

## The machine (signal path, five subsystems)

```
        ┌────────────── THE SUPPLY (one boiler, shared) ─────────┐
        │                                                        │
KEY →  THE ROTOR  →  commutation →  THE WIRES  →  THE BRIDGE  → out
        (pitch w/     (exciter)      (body w/      (shared,
         inertia,                     memory)       nonlinear)
         load sag)                       ↑              │
             ↑        THE ESCAPEMENT ────┘              │
             └────────────── load feedback ─────────────┘
```

1. **THE ROTOR** — the oscillator with character. Rotation rate is
   pitch; rate changes have MASS (asymmetric `Lag`: heavy spin-up,
   freewheel down). The body's energy LOADS the rotor — chords bog
   the machine down and pitch sags (displace-from-base; the forge
   sag physics as pitch behavior). Pole count sets the raw timbre
   (commutation pulse train through existing shaper cells);
   brush ARCING is wear- and velocity-gated noise bursts
   (`RandomWalk`-timed). The DIGITAL side lives here too: an **encoder
   wheel** — a programmable slot pattern on the rotor (8/16-bit
   patterns) that gates the commutation, from pure mechanical
   pulses to gnarly PCM-like patterns. One wheel param sweeps
   analog→digital timbre inside the metaphor.
2. **THE WIRES** — the body with memory. Per voice, a small bank
   of strings: waveguide (delay cell + damping filter) or modal
   (SVF bank) — prototype both in M2, keep the one that sings.
   Each wire passes a **hysteretic head** (forge's tape cell in
   the loop): the wire REMEMBERS what it played — timbre depends
   on history, demagnetizes slowly. Tuning morphs harmonic ↔
   stretched/inharmonic (the ORDER↔CHAOS axis).
3. **THE BRIDGE** — all sixteen voices terminate on ONE nonlinear
   bridge (impossible geometry, piano physics): loud notes sharpen
   slightly and bleed into their neighbors. SYMP graduated from
   control-rate fake to audio-rate coupling. Passivity is a law.
4. **THE SUPPLY** — one boiler behind everything. Pressure is a
   slow integrator (the HEAT shape); every sounding voice drains
   it; the machine goes breathy and flat under big chords and
   recovers in the rests. Polyphony as a shared resource.
5. **THE ESCAPEMENT** — the exciter's clock: from free (struck
   notes ring) through entrained (the escapement ticks at the
   wire's own period — self-organizing tremolo) to chaotic
   (mis-escapement, stochastic re-excitation). This is the motion
   heart of the CHAOS macro.

Every subsystem is composition over shipped cells (`Lag`,
`RandomWalk`, delay/SVF/allpass, tape hysteresis, triode shapers, the
Coupler, Adsr, the route matrix). Kernel additions only if a true
primitive emerges and earns it — expected: none.

## The face (the Atoms lesson, our spin)

Atoms' UI teaches three things: FEW macros; every macro can be SET
IN MOTION from a slot directly beneath it; and the center of the
panel is a LIVING PICTURE of the physics. DYNAMO's face:

- **Six macros in a ring**: FORCE (how hard the machine is
  driven), MASS (rotor inertia — pitch dynamics), MEMORY (head
  magnetism — how much the wires remember), COUPLE (bridge
  strength), STEAM (boiler size/recovery), CHAOS (escapement +
  tuning stretch, the order↔chaos axis).
- **A MOTION row under each macro** — source (OFF/LFO 1/LFO 2/
  DRIFT/ENV/…), rate, depth. Implemented as PRE-WIRED ROUTE SLOTS
  (routes-are-params; the matrix grows to 10 slots: 4 free + 6
  macro-motion). Zero new automation/state machinery; the cable
  bay and the motion rows are two views of the same params.
- **THE MACHINE DIAGRAM in the center** — a live Scene (probes:
  rotor rate, per-wire energy, boiler pressure, bridge load,
  escapement phase): the rotor visibly spins, wires glow with
  energy, the boiler needle droops under chords. Numbered-diagram
  precedent + `Op::Text` + probe-driven scenes all exist.
- **Bottom utility row** (Atoms' pattern): ROOT/PROFILE (genome
  slot), ATTACK, RELEASE, SPACE (a simple width/verb send later),
  OUTPUT.
- The patch bay stays (below the fold or behind a MOD view tab):
  macros are ALSO route targets, so the bay can modulate the
  macros that modulate the machine.

## The genome (the Synplant lesson)

A preset IS a genome (routes-are-params made it so). The genetics
layer is pure param math:
- **DICE**: seeded mutation — a walk in normalized param space
  with per-gene mutation RATES (build-sheet genes like pole count
  and wire tuning mutate eagerly; utility genes like output never).
- **BREED**: interpolate/cross two genomes (preset A × preset B).
- **LOCK**: per-gene locks (hold the rotor, mutate the body).
- **JITTER**: per-note gene noise riding SPREAD — every note a
  slightly different individual.
- Laws: same seed → same offspring, bit-compared; locks are
  absolute; jitter is reproducible per note sequence.
- The radial GARDEN view (seed center, mutant branches, drag to
  grow) is M7 polish — the genetics work headless first.

## Texture coverage (the product bar, stated)

- **Harmonics**: pole count + encoder patterns (exciter spectrum),
  wire tunings harmonic↔stretched, FM-style strike brightness.
- **Dynamics**: rotor inertia (pitch attack), load sag, boiler
  droop, bridge compression — the machine PLAYS BACK at you.
- **Noise**: brush arcing, steam breath, escapement chatter — all
  gated by physics (wear, pressure, chaos), never a static layer.
- **Analog domain**: drift, hysteresis memory, sag, warm-up (HEAT).
- **Digital domain**: encoder wheels, quantized motion routes,
  S&H escapement modes.

## Milestones

- **M0 — Skeleton + promotion.** Fork opal's layout; canon suite
  green on a sine-through voice; matrix/shaping semantics promoted
  to the shared home (third consumer, earned).
- **M1 — THE ROTOR.** Inertia, load-sag input, poles, encoder
  wheel, arcing. Laws: spin-up τ measured; sag under N voices;
  pole count = harmonic count; encoder pattern spectra pinned.
- **M2 — THE WIRES.** Waveguide vs modal prototype shoot-out;
  hysteretic head; order↔chaos tuning morph. Laws: per-mode T60;
  memory = history-dependent timbre, bit-reproducible.
- **M3 — BRIDGE + SUPPLY.** Audio-rate coupling; boiler. Laws:
  passivity (coupled network never gains energy); droop under
  chord load with recovery τ.
- **M4 — THE ESCAPEMENT.** Free/entrained/chaotic; CHAOS macro.
  Law: entrainment locks to the wire period within a dialed range.
- **M5 — MACROS + MOTION.** Six macros as params targeting the
  build sheet; motion rows as pre-wired slots; bay integration.
- **M6 — THE FACE.** Center diagram scene + probes; macro ring;
  bottom utility row; storybook story same-commit.
- **M7 — GENETICS.** Dice/breed/lock/jitter + laws; garden view
  last.

Ship rhythm as established: laws in the same commit as the thing
they measure; storybook story per UI component; gates + both
remotes per milestone; docs updated in-commit.
