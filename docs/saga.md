# Saga — a Lore-class reverse soundscape generator

`plugins/saga` (bundle **StaveSaga**) is modelled on the **Walrus Audio Lore
MKII** stereo reverse soundscape generator (manual v.1, 10/13/26). This note records
what the pedal is, how each control maps to a parameter, how the engine models it,
and where the plugin deliberately departs from the hardware.

## The pedal (research summary)

* Two DSP chips in series, each with its **own analog feedback path**. FEEDBACK
  sets the first path's gain and REGEN the second's. The wet path runs at **32 kHz**.
* Five modes:
  I reverse delay → reverse reverb · II reverse delay → octave-up reverb ·
  III reverse delay → octave-down reverb (heavy filtering + harmonic distortion) ·
  IV reverse reverb → forward reverb · V pitch delay → pitch delay.
* Face knobs: Feedback, Regen, Mod, Mix, X, Time, Tone, Tweak (+ Tweak button).
* Tweak menu: Width; Mod Rate 0.1–5 Hz; Mod Shape sine/square/random; Shift
  0.5×–2× (Dive/Rise target sample rate); Ramp 50–5000 ms.
* Tap/Shift footswitch: tap sets Time; **hold** shifts the *clock rate* to Shift
  over Ramp and holds it. In mode IV, Time is the *same* sample-rate stretch,
  so Time = Shift makes Shift inaudible.
* MIDI CC: mode 9, feedback 14, regen 15, mod 16, mix 17, X 18, time 19, tone 20,
  width 21, mod rate 22, shape 23, shift 24, ramp 25, exp 27, bypass 31, tap 85.
* Factory presets RED (mode I), GREEN (II), BLUE (IV). Knob positions were read
  off the manual's diagrams and live in `presets.rs`.

## Control map

| Pedal | Param (id) | Notes |
|---|---|---|
| Mode button | `mode` (3) | Tabs I–V. A switch fades the wet, wipes the core, fades back |
| Feedback | `feedback` (4) | path-1 gain, 0..1 (soft-clipped loop, so unity sustains) |
| Regen | `regen` (5) | path-2 gain |
| Mod | `mod` (6) | clock wobble depth, up to ±0.05 oct (square law) |
| Mix | `mix` (7) | equal power; 0 = bit-exact dry (property test) |
| X | `x` (8) | I: forward→reversed · II–IV: decay 0.6–25 s · V: 6 interval orders |
| Time | `time` (9) | 50–1200 ms; IV: clock stretch 2×…0.5× |
| Tone | `tone` (10) | resonant LP 120 Hz–18 kHz on the wet |
| Tweak→Width | `width` (11) | wet side gain 0..2× |
| Tweak→Mod Rate | `mod rate` (12) | 0.1–5 Hz |
| Tweak→Mod Shape | `mod shape` (13) | sine / square / smoothed random |
| Tweak→Shift | `shift` (14) | 0.5–2×, absolute clock target |
| Tweak→Ramp | `ramp` (15) | 50–5000 ms |
| Tap/Shift hold | `dive` (16) | hold = on; map it to a momentary MIDI button |
| Tap tempo | `sync` (17) | FREE or host-tempo divisions |
| Bypass | `bypass` (2) | framework click-free gate |

## Engine model

The defining move is literal: the wet core runs on its own **clock**
(`engine/clock.rs`). It ticks at 32 kHz × clock in real time and is resampled
to and from the host through 4th-order converter filters. DIVE/RISE, the MOD
LFO and mode IV's stretch all move that one clock, so audio already in the
buffers changes pitch and every time scales, as on the hardware. Programs are
five wirings (`engine/programs.rs`) of four building blocks: `ReverseLine` (two backward
heads under sin² windows), `Tank` (8-line Hadamard FDN, energy-normalized),
`Transposer` (two-head delay-line shifter) and `AnalogPath` (HP, LP and
soft-clip feedback leg).

## Deliberate departures

* **Trails**: bypass is the framework crossfade, so it behaves like No-Trails.
* **Tap tempo** is replaced by host sync. **Expression** and **MIDI CC** come
  from host automation and MIDI mapping.
* Voicings (tank sizes, path corners, wet blends) are by ear and measurement,
  not by teardown. Wet-only level sits within −2.5…+4 dB of dry across the
  factory presets (`tests/listen.rs`).
