//! The chime ENGINE — sixteen [`ChimeVoice`]s under a
//! quietest-steal [`VoiceRack`] (the Voice form's first shipping
//! consumer). The engine's own job is exactly the
//! BRIDGE the form's layering law prescribes: host note events →
//! `VoiceEvent` (hz resolved here — tuning is engine business, not
//! kernel business), dispatched at their sample offsets so a note
//! landing mid-block sounds mid-block (a law test).
//!
//! THE MOD MATRIX: routes
//! are ordinary PARAMS (7 per slot × 4), evaluated per block per
//! voice and applied as DISPLACE-FROM-BASE: knobs set the base,
//! routes displace, they never fight. Per-voice targets glide over
//! ~5 ms inside the voice (block-rate application without
//! block-rate zipper).
//!
//! SOURCES (all composition over dsp cells, per the kernel rule):
//! * LFO — shaped (dsp basis; S&H/DRIFT are RandomWalk recipes), tempo-
//!   syncable, NOTE-retriggerable (per-voice phase), with an onset
//!   RISE ramp per note (a player's vibrato fades in);
//! * VEL / KEY / ENV / SPREAD — per-voice: the note's velocity and
//!   key, the voice's own envelope, and a stable per-note random
//!   (unison detune generalized to any target);
//! * HEAT — a fatigue integrator of playing intensity (components
//!   warming up: hit it harder and longer, it drifts);
//! * SYMP — the OTHER voices' energy bleeding in (sympathetic
//!   strings; a lone voice hears none).

extern crate alloc;
use alloc::sync::Arc;

use stave_core::debug::Tap;
use stave_core::params::{ContinuousControl, Watch};
use stave_core::{DebugBus, Environment, EventCursor, EventKind, ProcessCtx, Tail};
use stave_dsp::{
    db_to_amp, hz_of_key, mod_lfo_hz, rise, standard_sources, sympathy, Adsr, AdsrConfig, Ensemble,
    ModLfo, ModLfoShape, NoteSources, Osc, OscShape, Steal, Voice, VoiceEvent, VoiceRack,
};

use crate::params::ChimeParams;

const MAX_VOICES: usize = 16;

/// Voice-sum headroom: −12 dB per voice, so a four-voice fortissimo
/// chord sums to unity and a full 16-voice storm stays inside +12
/// dBFS BEFORE the output trim (the validation suite's param_sweep
/// caught the unscaled sum at +37 dBFS with the trim maxed).
const VOICE_GAIN: f32 = 0.25;

mod matrix;
use matrix::Routes;

/// One note: an oscillator through its envelope, velocity-scaled,
/// displaced by the matrix.
pub struct ChimeVoice {
    osc: Osc,
    env: Adsr,
    /// What this note offers the matrix (velocity, key, spread, age,
    /// its LFO phase).
    sources: NoteSources<1>,
    hz: f32,
    /// Route displacement TARGETS (block rate) and their ~5 ms
    /// glides (frame rate) — displace-from-base without zipper.
    pitch_st_target: f32,
    level_db_target: f32,
    pitch_st: f32,
    level_db: f32,
    glide: f32,
    /// Base envelope knobs (the mod scales DECAY from these).
    base_env: AdsrConfig,
    decay_scale: f32,
    sr: f32,
    /// Per-slot lag states (the shaping chain's memory).
    lags: [f32; 4],
}

impl ChimeVoice {
    fn new() -> Self {
        Self {
            osc: Osc::new(OscShape::Sine),
            env: Adsr::new(),
            sources: NoteSources::new(0x9E37_79B9),
            hz: 0.0,
            pitch_st_target: 0.0,
            level_db_target: 0.0,
            pitch_st: 0.0,
            level_db: 0.0,
            glide: 1.0,
            base_env: AdsrConfig {
                attack_ms: 5.0,
                decay_ms: 800.0,
                sustain: 0.0,
                release_ms: 1_200.0,
            },
            decay_scale: 1.0,
            sr: 48_000.0,
            lags: [0.0; 4],
        }
    }

    fn with_seed(seed: u32) -> Self {
        let mut v = Self::new();
        v.sources = NoteSources::new(seed);
        v
    }

    /// The voice's own envelope level (the ENV source).
    fn env_level(&self) -> f32 {
        self.env.level()
    }

    /// Block-rate voicing: the envelope knobs and the shape reach
    /// every voice through the rack's `voices_mut`.
    /// The envelope with the matrix's decay displacement applied.
    fn scaled_env(&self) -> AdsrConfig {
        AdsrConfig {
            decay_ms: self.base_env.decay_ms * self.decay_scale,
            ..self.base_env
        }
    }

    fn set(&mut self, env: AdsrConfig, shape: OscShape, sr: f32) {
        self.base_env = env;
        self.env.set(&self.scaled_env(), sr);
        self.osc.set_shape(shape);
    }

    /// Evaluate the routes for THIS voice (per-voice sources read
    /// the voice's own state) and set the displacement targets.
    fn apply_routes(&mut self, routes: &Routes, sv: &matrix::SrcValues) {
        let mut lags = self.lags;
        let (pitch, level_db, decay_oct) = matrix::evaluate(routes, sv, &mut lags);
        self.lags = lags;
        self.pitch_st_target = pitch;
        self.level_db_target = level_db;
        let scale = decay_oct.exp2();
        if (scale - self.decay_scale).abs() > 1e-3 {
            self.decay_scale = scale;
            self.env.set(&self.scaled_env(), self.sr);
        }
    }
}

impl Voice for ChimeVoice {
    fn prepare(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        // ~5 ms displacement glide (block-rate targets, no zipper).
        self.glide = 1.0 - (-1.0 / (0.005 * sample_rate)).exp();
        self.reset();
    }
    fn reset(&mut self) {
        self.env.reset();
        self.osc.reset();
        self.sources.velocity = 0.0;
        self.pitch_st = 0.0;
        self.level_db = 0.0;
        self.pitch_st_target = 0.0;
        self.level_db_target = 0.0;
    }
    fn note_on(&mut self, hz: f32, velocity: f32) {
        self.hz = hz;
        self.osc.set_hz(hz, self.sr);
        self.osc.reset(); // same attack every time: reproducible by construction
        self.env.gate_on();
        self.sources.note_on(hz, velocity);
    }
    fn note_off(&mut self) {
        self.env.gate_off();
    }
    fn choke(&mut self) {
        self.env.choke();
    }
    fn active(&self) -> bool {
        self.env.active()
    }
    fn level(&self) -> f32 {
        self.env.level() * self.sources.velocity
    }
    fn tick(&mut self) -> (f32, f32) {
        self.sources.tick();
        // Displacements glide toward their block-rate targets; the
        // glide is the anti-zipper, not the modulation (routes are
        // the modulation).
        self.pitch_st += self.glide * (self.pitch_st_target - self.pitch_st);
        self.level_db += self.glide * (self.level_db_target - self.level_db);
        if self.pitch_st.abs() > 1e-4 {
            self.osc
                .set_hz(self.hz * (self.pitch_st / 12.0).exp2(), self.sr);
        }
        let g = if self.level_db.abs() > 1e-4 {
            db_to_amp(self.level_db)
        } else {
            1.0
        };
        let y = self.osc.tick() * self.env.tick() * self.sources.velocity * g;
        (y, y)
    }
}

pub struct ChimeEngine {
    params: Arc<ChimeParams>,
    output_control: ContinuousControl,
    sr: f32,
    rack: VoiceRack<ChimeVoice, MAX_VOICES>,
    /// Envelope + shape re-voice only when a knob moved. Reset in
    /// activate — the ACTIVATION LAW (a re-prepare wipes what the
    /// params can't see).
    voicing: Watch<5>,
    /// The matrix clock (global source; per-voice sources live in
    /// the notes themselves).
    lfo: ModLfo,
    /// HEAT and the last note played, for the rings.
    ensemble: Ensemble,
    /// The rings' feed: per-slot live SOURCE values, as
    /// DECLARED taps. Per-voice sources publish the most
    /// recent note's value — the ring dances to what you just
    /// played, the Vital feel.
    bus: Option<Arc<DebugBus>>,
    mod_taps: [Tap; 4],
}

impl stave_core::Processor for ChimeEngine {
    type Params = ChimeParams;

    fn new(params: Arc<ChimeParams>) -> Self {
        Self {
            output_control: ContinuousControl::new(&params.output),
            params,
            sr: 48_000.0,
            rack: VoiceRack::new(Steal::Quietest, |i| {
                ChimeVoice::with_seed(0x9E37_79B9 ^ (i as u32).wrapping_mul(0x9E37_79B1))
            }),
            voicing: Watch::new(),
            lfo: ModLfo::new(833.0, 0xC0FF_EE01),
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
        self.ensemble.reset();
        self.voicing.reset();
    }

    fn reset(&mut self) {
        self.rack.reset();
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Param changes become structure.
        if self
            .voicing
            .changed([&p.attack, &p.decay, &p.sustain, &p.release, &p.shape])
        {
            let env = AdsrConfig {
                attack_ms: p.attack.value(),
                decay_ms: p.decay.value(),
                sustain: p.sustain.value(),
                release_ms: p.release.value(),
            };
            let shape = if p.shape.index() == 1 {
                OscShape::Saw
            } else {
                OscShape::Sine
            };
            let sr = self.sr;
            for v in self.rack.voices_mut() {
                v.set(env, shape, sr);
            }
        }

        // 1b. The matrix: snapshot the route params (7 per slot),
        //     advance the global clocks, then evaluate per voice.
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
        if routes.any() {
            // SYMP needs the ensemble's energy before any voice moves.
            let mut total_level = 0.0f32;
            for v in self.rack.voices_mut() {
                total_level += v.level();
            }
            let rise_ms = p.lfo_rise.value();
            let note_trig = p.lfo_trig.index() == 1;
            let ensemble = &self.ensemble;
            let sr = self.sr;
            let lfo = &self.lfo;
            for v in self.rack.voices_mut() {
                // The voice's own LFO value: NOTE trig reads its own
                // phase, and the RISE ramp fades it in from note-on.
                let raw_lfo = if note_trig {
                    lfo.at_phase(v.sources.lfo_phases[0])
                } else {
                    global_lfo
                };
                v.sources.lfo_phases[0] = lfo.advance_phase(v.sources.lfo_phases[0], block_s);
                let ramp = rise(v.sources.age, rise_ms, sr);
                let sv: matrix::SrcValues = standard_sources(
                    raw_lfo * ramp,
                    &v.sources,
                    v.env_level(),
                    ensemble,
                    sympathy(total_level, v.level()),
                );
                v.apply_routes(&routes, &sv);
            }
        }
        if let Some(bus) = &self.bus {
            // The rings' feed: each slot's live SOURCE value (±1).
            // Per-voice sources publish the most recent note's or an
            // ensemble view — the ring is an indicator, not a meter.
            let mut total_level = 0.0f32;
            for v in self.rack.voices_mut() {
                total_level += v.level();
            }
            let shown = self.ensemble.sources(global_lfo, total_level);
            for (i, slot) in routes.slots.iter().enumerate() {
                let value = slot.source(&shown).unwrap_or(0.0);
                self.mod_taps[i].set(bus, f64::from(value));
            }
        }

        // 2. The audio block: the BRIDGE (host events → VoiceEvent,
        //    at their frame offsets) around the rack's tick.
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

    /// Honest from the knobs: the longest a released note can ring
    /// (release parks at −80 dB ≈ 9.3 τ).
    fn tail(&self) -> Tail {
        let r_s = self.params.release.value() * 0.001 * 9.3;
        Tail::Samples((r_s * self.sr) as u32)
    }
}
