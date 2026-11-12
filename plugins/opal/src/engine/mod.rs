//! The opal ENGINE — sixteen [`OpalVoice`]s under a quietest-steal
//! [`VoiceRack`]; each voice = an [`FmCore`] (four operators, wiring
//! as data) beside FOUR per-voice [`Adsr`] instances — the tier-2
//! shape the mini-FM conformance voice proved before this project
//! existed. The spine is the same three phases as
//! every stave engine: apply changed knobs to the voices, bridge
//! host events to the rack at their sample offsets, tick.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{ContinuousControl, Watch};
use stave_core::{Environment, EventCursor, EventKind, ProcessCtx, Tail};
use stave_dsp::{
    hz_of_key, standard_sources, sympathy, Adsr, AdsrConfig, Ensemble, NoteSources, Steal, Voice,
    VoiceEvent, VoiceRack,
};

use crate::params::OpalParams;

mod matrix;
pub(crate) mod operators;
use matrix::{with_lfo2, Displacements, Routes};
use operators::{FmCore, OpSettings};
use stave_core::debug::Tap;
use stave_core::DebugBus;
use stave_dsp::{db_to_amp, mod_lfo_hz, rise, ModLfo, ModLfoShape};

const MAX_VOICES: usize = 16;

/// −12 dB voice-sum headroom (the chime lesson: an unscaled pool
/// reads as a param_sweep offender the day someone maxes the trim).
const VOICE_GAIN: f32 = 0.25;

/// One note: the FM core + its four envelope instances.
pub struct OpalVoice {
    core: FmCore,
    envs: [Adsr; 4],
    /// The knobs as last applied, and the voice's settings gliding to
    /// them: levels, feedback and vel mod move a sounding note, where a
    /// step clicks.
    knobs: OpSettings,
    set: OpSettings,
    /// False until the first voicing after prepare, which snaps.
    voiced: bool,
    /// Envelope knob snapshot (a, d, s, r per op) — reapplied to the
    /// per-voice instances whenever the table moves.
    inc: [f64; 4],
    hz: f32,
    /// What this note offers the matrix; its two LFO phases are LFO1
    /// and LFO2 under NOTE trig.
    sources: NoteSources<2>,
    /// Matrix displacement targets (block rate) and their ~5 ms
    /// glides — displace-from-base without zipper (the chime shape).
    disp_target: Displacements,
    disp: Displacements,
    glide: f32,
    sr: f32,
    lags: [f32; 4],
}

impl OpalVoice {
    fn new() -> Self {
        Self {
            core: FmCore::new(),
            envs: core::array::from_fn(|_| Adsr::new()),
            knobs: OpSettings::new(),
            set: OpSettings::new(),
            voiced: false,
            inc: [0.0; 4],
            hz: 0.0,
            sources: NoteSources::new(0x9E37_79B9),
            disp_target: Displacements::default(),
            disp: Displacements::default(),
            glide: 1.0,
            sr: 48_000.0,
            lags: [0.0; 4],
        }
    }

    fn with_seed(seed: u32) -> Self {
        let mut v = Self::new();
        v.sources = NoteSources::new(seed);
        v
    }

    /// The ENV source: the loudest carrier envelope (unscaled).
    fn env_level(&self) -> f32 {
        self.carrier_env_stat(|e| e.level(), f32::max)
    }

    /// Block rate: evaluate the routes for THIS voice.
    fn apply_routes(&mut self, routes: &Routes, sv: &matrix::SrcValues) {
        let mut lags = self.lags;
        self.disp_target = matrix::evaluate(routes, sv, &mut lags);
        self.lags = lags;
    }

    /// Block-rate voicing from the table (via the rack's
    /// `voices_mut`). Envelope times retune live; ratios re-derive
    /// the increments so a ratio knob bends a SOUNDING note.
    fn apply(&mut self, set: OpSettings, adsr: [AdsrConfig; 4]) {
        self.knobs = set;
        let gliding = (self.set.levels, self.set.feedback, self.set.vel_mod);
        self.set = set;
        if self.voiced {
            (self.set.levels, self.set.feedback, self.set.vel_mod) = gliding;
        }
        self.voiced = true;
        for (env, config) in self.envs.iter_mut().zip(adsr) {
            env.set(&config, self.sr);
        }
        self.retune();
    }

    fn retune(&mut self) {
        for i in 0..4 {
            self.inc[i] = f64::from(self.hz * self.set.ratios[i]) / f64::from(self.sr);
        }
    }

    /// A carrier op reaches the ear; a modulator only shapes one.
    fn carrier_env_stat(&self, f: impl Fn(&Adsr) -> f32, fold: impl Fn(f32, f32) -> f32) -> f32 {
        let carriers = operators::WIRINGS[self.set.algo.min(7)].carriers;
        let mut acc = 0.0f32;
        for (i, env) in self.envs.iter().enumerate() {
            if carriers & (1 << i) != 0 {
                acc = fold(acc, f(env));
            }
        }
        acc
    }
}

impl Voice for OpalVoice {
    fn prepare(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        self.glide = 1.0 - (-1.0 / (0.005 * sample_rate)).exp();
        self.voiced = false;
        self.reset();
    }
    fn reset(&mut self) {
        self.core.reset();
        for e in self.envs.iter_mut() {
            e.reset();
        }
        self.sources.velocity = 0.0;
        self.disp = Displacements::default();
        self.disp_target = Displacements::default();
    }
    fn note_on(&mut self, hz: f32, velocity: f32) {
        self.hz = hz;
        self.sources.note_on(hz, velocity);
        self.retune();
        self.core.reset(); // same attack every time — reproducible
        for e in self.envs.iter_mut() {
            e.gate_on();
        }
    }
    fn note_off(&mut self) {
        for e in self.envs.iter_mut() {
            e.gate_off();
        }
    }
    fn choke(&mut self) {
        for e in self.envs.iter_mut() {
            e.choke();
        }
    }
    /// Alive while any CARRIER still sounds — a ringing modulator
    /// with dead carriers is silence.
    fn active(&self) -> bool {
        self.carrier_env_stat(|e| if e.active() { 1.0 } else { 0.0 }, f32::max) > 0.0
    }
    fn level(&self) -> f32 {
        self.carrier_env_stat(|e| e.level(), f32::max) * self.sources.velocity
    }
    fn tick(&mut self) -> (f32, f32) {
        self.sources.tick();
        // Glide the matrix displacements (anti-zipper), then apply
        // displace-from-base: knobs are the base, routes displace.
        let g = self.glide;
        for i in 0..4 {
            self.set.levels[i] += g * (self.knobs.levels[i] - self.set.levels[i]);
        }
        self.set.feedback += g * (self.knobs.feedback - self.set.feedback);
        self.set.vel_mod += g * (self.knobs.vel_mod - self.set.vel_mod);
        self.disp.pitch_st += g * (self.disp_target.pitch_st - self.disp.pitch_st);
        self.disp.feedback += g * (self.disp_target.feedback - self.disp.feedback);
        self.disp.level1 += g * (self.disp_target.level1 - self.disp.level1);
        self.disp.ratio2_st += g * (self.disp_target.ratio2_st - self.disp.ratio2_st);
        for i in 0..3 {
            self.disp.levels[i] += g * (self.disp_target.levels[i] - self.disp.levels[i]);
        }
        let mut set = self.set;
        set.levels[0] = (set.levels[0] + self.disp.level1).clamp(0.0, 1.0);
        for i in 0..3 {
            set.levels[i + 1] = (set.levels[i + 1] + self.disp.levels[i]).clamp(0.0, 1.0);
        }
        set.feedback = (set.feedback + self.disp.feedback).clamp(0.0, 1.0);
        let mut inc = if self.disp.pitch_st.abs() > 1e-4 {
            let m = f64::from((self.disp.pitch_st / 12.0).exp2());
            [
                self.inc[0] * m,
                self.inc[1] * m,
                self.inc[2] * m,
                self.inc[3] * m,
            ]
        } else {
            self.inc
        };
        // RATIO 2 bends op 2 alone — inharmonicity, the FM chorus.
        if self.disp.ratio2_st.abs() > 1e-4 {
            inc[1] *= f64::from((self.disp.ratio2_st / 12.0).exp2());
        }
        // Velocity → loudness on carriers, → brightness on
        // modulators (depth = lerp(1, vel, vel_mod)).
        let vel_index = 1.0 + self.set.vel_mod * (self.sources.velocity - 1.0);
        let y = self
            .core
            .tick(&set, &mut self.envs, inc, self.sources.velocity, vel_index);
        (y, y)
    }
}

pub struct OpalEngine {
    params: Arc<OpalParams>,
    output_control: ContinuousControl,
    sr: f32,
    rack: VoiceRack<OpalVoice, MAX_VOICES>,
    /// The whole voicing surface (27 knobs). Reset in activate — the
    /// ACTIVATION LAW.
    voicing: Watch<27>,
    /// The matrix clock.
    lfo: ModLfo,
    /// The second LFO (the Absynth lesson); its previous block's value
    /// is what self-referencing rate routes read.
    lfo2: ModLfo,
    /// LFO2 RATE is a GLOBAL-scope target: its routes evaluate once
    /// per block with ensemble source values; the lag states for
    /// that pass live here, not in any voice.
    global_lags: [f32; 4],
    ensemble: Ensemble,
    bus: Option<Arc<DebugBus>>,
    mod_taps: [Tap; 4],
}

impl stave_core::Processor for OpalEngine {
    type Params = OpalParams;

    fn new(params: Arc<OpalParams>) -> Self {
        Self {
            output_control: ContinuousControl::new(&params.output),
            params,
            sr: 48_000.0,
            rack: VoiceRack::new(Steal::Quietest, |i| {
                OpalVoice::with_seed(0x0BA1_0BA1 ^ (i as u32).wrapping_mul(0x9E37_79B1))
            }),
            voicing: Watch::new(),
            lfo: ModLfo::new(833.0, 0x0BA1_0001),
            lfo2: ModLfo::new(3_333.0, 0x0BA1_0002),
            global_lags: [0.0; 4],
            ensemble: Ensemble::default(),
            bus: None,
            mod_taps: [
                Tap::new("mod 1"),
                Tap::new("mod 2"),
                Tap::new("mod 3"),
                Tap::new("mod 4"),
            ],
        }
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    fn activate(&mut self, env: Environment) {
        let rate = f64::from(env.sample_rate);
        self.output_control.prepare(rate, &self.params.output);
        self.sr = env.sample_rate;
        self.rack.prepare(env.sample_rate);
        self.lfo
            .prepare(env.sample_rate, self.params.lfo_rate.value());
        self.lfo2
            .prepare(env.sample_rate, self.params.lfo2_rate.value());
        self.global_lags = [0.0; 4];
        self.ensemble.reset();
        self.voicing.reset();
    }

    fn reset(&mut self) {
        self.rack.reset();
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Param changes become structure.
        #[rustfmt::skip]
        let moved = self.voicing.changed([
            &p.algo, &p.feedback, &p.vel_mod,
            &p.ratio1, &p.level1, &p.attack1, &p.decay1, &p.sustain1, &p.release1,
            &p.ratio2, &p.level2, &p.attack2, &p.decay2, &p.sustain2, &p.release2,
            &p.ratio3, &p.level3, &p.attack3, &p.decay3, &p.sustain3, &p.release3,
            &p.ratio4, &p.level4, &p.attack4, &p.decay4, &p.sustain4, &p.release4,
        ]);
        if moved {
            let set = OpSettings {
                ratios: [
                    p.ratio1.value(),
                    p.ratio2.value(),
                    p.ratio3.value(),
                    p.ratio4.value(),
                ],
                levels: [
                    p.level1.value(),
                    p.level2.value(),
                    p.level3.value(),
                    p.level4.value(),
                ],
                algo: p.algo.index(),
                feedback: p.feedback.value(),
                vel_mod: p.vel_mod.value(),
            };
            let adsr = [
                AdsrConfig {
                    attack_ms: p.attack1.value(),
                    decay_ms: p.decay1.value(),
                    sustain: p.sustain1.value(),
                    release_ms: p.release1.value(),
                },
                AdsrConfig {
                    attack_ms: p.attack2.value(),
                    decay_ms: p.decay2.value(),
                    sustain: p.sustain2.value(),
                    release_ms: p.release2.value(),
                },
                AdsrConfig {
                    attack_ms: p.attack3.value(),
                    decay_ms: p.decay3.value(),
                    sustain: p.sustain3.value(),
                    release_ms: p.release3.value(),
                },
                AdsrConfig {
                    attack_ms: p.attack4.value(),
                    decay_ms: p.decay4.value(),
                    sustain: p.sustain4.value(),
                    release_ms: p.release4.value(),
                },
            ];
            for v in self.rack.voices_mut() {
                v.apply(set, adsr);
            }
        }

        // 1b. The matrix (the chime pattern, per-op destinations,
        //     M-mod sources and shaping).
        let block_s = ctx.frames as f32 / self.sr;
        let tempo = ctx
            .transport
            .as_ref()
            .and_then(|t| t.tempo_bpm)
            .unwrap_or(120.0) as f32;
        let hz = mod_lfo_hz(p.lfo_rate.value(), p.lfo_sync.index(), tempo);
        self.lfo
            .set(hz, ModLfoShape::from_index(p.lfo_shape.index()), self.sr);
        let global_lfo = self.lfo.advance(ctx.frames);
        self.ensemble.leak(block_s);
        let routes = Routes {
            slots: p.routes.each_ref().map(|r| r.slot(block_s)),
        };
        let mut total_level = 0.0f32;
        for v in self.rack.voices_mut() {
            total_level += v.level();
        }
        // LFO 2 — its rate is itself a TARGET, evaluated once per
        // block at GLOBAL scope (ensemble source values; a global
        // clock cannot follow sixteen voices; self-reference reads
        // the PREVIOUS block, the one-block lag every feedback
        // observes).
        let hz2_base = mod_lfo_hz(p.lfo2_rate.value(), p.lfo2_sync.index(), tempo);
        let global_sv = with_lfo2(
            self.ensemble.sources(global_lfo, total_level),
            self.lfo2.value(),
        );
        let mut glags = self.global_lags;
        let gdisp = matrix::evaluate(&routes, &global_sv, &mut glags);
        self.global_lags = glags;
        let hz2 = (hz2_base * gdisp.lfo2_oct.exp2()).clamp(0.01, 40.0);
        self.lfo2
            .set(hz2, ModLfoShape::from_index(p.lfo2_shape.index()), self.sr);
        let global_lfo2 = self.lfo2.advance(ctx.frames);
        if routes.any() {
            let rise_ms = p.lfo_rise.value();
            let rise2_ms = p.lfo2_rise.value();
            let note_trig = p.lfo_trig.index() == 1;
            let note_trig2 = p.lfo2_trig.index() == 1;
            let ensemble = &self.ensemble;
            let sr = self.sr;
            let (lfo, lfo2) = (&self.lfo, &self.lfo2);
            for v in self.rack.voices_mut() {
                let raw_lfo = if note_trig {
                    lfo.at_phase(v.sources.lfo_phases[0])
                } else {
                    global_lfo
                };
                let raw_lfo2 = if note_trig2 {
                    lfo2.at_phase(v.sources.lfo_phases[1])
                } else {
                    global_lfo2
                };
                let phases = &mut v.sources.lfo_phases;
                phases[0] = lfo.advance_phase(phases[0], block_s);
                phases[1] = lfo2.advance_phase(phases[1], block_s);
                let age = v.sources.age;
                let sv = with_lfo2(
                    standard_sources(
                        raw_lfo * rise(age, rise_ms, sr),
                        &v.sources,
                        v.env_level(),
                        ensemble,
                        sympathy(total_level, v.level()),
                    ),
                    raw_lfo2 * rise(age, rise2_ms, sr),
                );
                v.apply_routes(&routes, &sv);
            }
        }
        if let Some(bus) = &self.bus {
            let shown = with_lfo2(self.ensemble.sources(global_lfo, total_level), global_lfo2);
            for (i, slot) in routes.slots.iter().enumerate() {
                let value = slot.source(&shown).unwrap_or(0.0);
                self.mod_taps[i].set(bus, f64::from(value));
            }
        }

        // 2. The audio block: the host-event → VoiceEvent bridge at
        //    sample offsets, around the rack's tick.
        let mut out_ramp = self.output_control.begin_block(ctx.params, &p.output);
        let rack = &mut self.rack;
        let mut notes = EventCursor::new(ctx.events);
        let ensemble = &mut self.ensemble;
        let lanes = ctx.lanes::<2>();
        lanes.run(|fr| {
            for event in notes.due(fr.index()) {
                let EventKind::Note(note) = event.kind;
                let voice = note.voice_event(hz_of_key);
                if let VoiceEvent::On { hz, velocity, .. } = voice {
                    ensemble.note_on(hz, velocity);
                }
                rack.dispatch(voice);
            }
            let g = db_to_amp(out_ramp.next()) * VOICE_GAIN;
            let (l, r) = rack.tick();
            fr.write(0, l * g);
            fr.write(1, r * g);
        });
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    /// The longest a released note can ring: the slowest carrier
    /// release, to the −80 dB park (≈ 9.3 τ).
    fn tail(&self) -> Tail {
        let r = self
            .params
            .release1
            .value()
            .max(self.params.release2.value())
            .max(self.params.release3.value())
            .max(self.params.release4.value());
        Tail::Samples((r * 0.001 * 9.3 * self.sr) as u32)
    }
}
