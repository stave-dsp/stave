//! The dynamo ENGINE — sixteen [`DynamoVoice`]s under a
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
use core::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use stave_dsp::Xorshift32;

use stave_core::debug::Tap;
use stave_core::params::{ContinuousControl, Watch};
use stave_core::{DebugBus, Environment, EventCursor, EventKind, ProcessCtx, Tail};
use stave_dsp::{
    db_to_amp, hz_of_key, mod_lfo_hz, rise, standard_sources, sympathy, Adsr, AdsrConfig, Ensemble,
    ModLfo, ModLfoShape, NoteSources, Steal, Voice, VoiceEvent, VoiceRack,
};

mod rotor;
mod wire;
use rotor::Rotor;
use wire::Wire;

use crate::params::DynamoParams;

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
pub struct DynamoVoice {
    rotor: Rotor,
    /// The body: a detuned wire pair the rotor drives. The machine
    /// is ONE unit — its only voice is this body (no parallel dry
    /// path; the field read a blend knob as two circuits).
    wires: [Wire; 2],
    /// Stereo width: the pair splits mid/side. Glides to its target.
    width: f32,
    width_target: f32,
    /// Contact mode (0 GRIND, 1 STRIKE, 2 BOW) + its gate state.
    contact: usize,
    strike_gate: f32,
    strike_decay: f32,
    /// Machine noise into the wires (arcing + bearing).
    grit: f32,
    noise_rng: Xorshift32,
    /// FORCE: how hard the machine is driven (macro; 0.5 = unity).
    force: f32,
    /// Key held? The escapement engages only while held (an EBow
    /// you lift off — without this, chaotic voices never died and
    /// transport stop/start layered old drones under new notes).
    held: bool,
    /// THE LIFE SYSTEM: hum, bearing, clank, crackle, tension
    /// dials plus their state — every involuntary sound is a
    /// function of machine state, voiced through the wires.
    life: (f32, f32, f32, f32, f32),
    hum_phase: f32,
    hum_inc: f32,
    heat_now: f32,
    bearing_lp: f32,
    clank_amp: f32,
    clank_ph: [f32; 3],
    clank_inc: [f32; 3],
    crackle_wait: u32,
    tension_st: f32,
    /// The last body sample (the BOW's friction sense).
    last_body: f32,
    /// THE BRIDGE: the shared rail every voice reads (previous
    /// sample's ensemble body — the one-sample lag every feedback
    /// observes) and feeds its own body back into.
    bridge: Arc<AtomicU32>,
    couple: f32,
    /// The voice's own share of the rail (subtracted from what it
    /// receives — sympathy is for OTHERS; the passivity law caught
    /// self-regeneration droning at full couple, tail > body).
    own_rail: f32,
    /// THE SUPPLY: boiler pressure (engine-set, block rate).
    supply: f32,
    /// THE ESCAPEMENT: engagement (the CHAOS axis), the pending
    /// tick, and the crossing detector's previous sample.
    chaos: f32,
    /// A tooth in flight: samples remaining, its length, its push.
    esc_run: u32,
    esc_len: u32,
    esc_amp: f32,
    /// Pallet refractory: one tooth per swing (without it the
    /// pallet fired on every crossing and locked partial 2).
    esc_refract: u32,
    /// The pallet's INERTIA: it senses the low-passed swing, not
    /// the jangle (raw-body crossings locked partial 3 — the
    /// winner-take-all of whichever mode dominates the sum).
    esc_sense: f32,
    esc_sense_k: f32,
    esc_prev: f32,
    /// (ring_s, stiff) the wires are tuned for, gliding to the knobs'
    /// target; note-ons tune to it.
    wire_voicing: (f32, f32),
    wire_target: (f32, f32),
    /// False until the first voicing after prepare, which snaps.
    voiced: bool,
    /// Load-sag pitch multiplier target (the ensemble bogs the
    /// machine down; glided like every displacement).
    sag_mult_target: f32,
    sag_mult: f32,
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
    lags: [f32; matrix::SLOTS],
}

impl DynamoVoice {
    fn new(bridge: Arc<AtomicU32>) -> Self {
        Self {
            rotor: Rotor::new(),
            wires: [Wire::new(), Wire::new()],
            width: 0.8,
            width_target: 0.8,
            contact: 0,
            strike_gate: 0.0,
            strike_decay: 0.999,
            grit: 0.15,
            noise_rng: Xorshift32::new(0x51DE_CA57),
            force: 0.5,
            held: false,
            life: (0.0, 0.0, 0.0, 0.0, 0.0),
            hum_phase: 0.0,
            hum_inc: 0.0,
            heat_now: 0.0,
            bearing_lp: 0.0,
            clank_amp: 0.0,
            clank_ph: [0.0; 3],
            clank_inc: [0.0; 3],
            crackle_wait: 4_800,
            tension_st: 0.0,
            last_body: 0.0,
            bridge,
            couple: 0.15,
            own_rail: 0.0,
            supply: 1.0,
            chaos: 0.0,
            esc_run: 0,
            esc_len: 1,
            esc_amp: 0.0,
            esc_refract: 0,
            esc_sense: 0.0,
            esc_sense_k: 0.05,
            esc_prev: 0.0,
            wire_voicing: (0.8, 0.15),
            wire_target: (0.8, 0.15),
            voiced: false,
            sag_mult_target: 1.0,
            sag_mult: 1.0,
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
            lags: [0.0; matrix::SLOTS],
        }
    }

    fn with_seed(seed: u32, bridge: Arc<AtomicU32>) -> Self {
        let mut v = Self::new(bridge);
        v.sources = NoteSources::new(seed);
        v
    }

    /// The voice's own envelope level (the ENV source).
    fn env_level(&self) -> f32 {
        self.env.level()
    }

    /// Envelope voicing — WATCH-gated (env.set restates the ADSR).
    /// The envelope with the matrix's decay displacement applied.
    fn scaled_env(&self) -> AdsrConfig {
        AdsrConfig {
            decay_ms: self.base_env.decay_ms * self.decay_scale,
            ..self.base_env
        }
    }

    fn set_env(&mut self, env: AdsrConfig, sr: f32) {
        self.base_env = env;
        self.env.set(&self.scaled_env(), sr);
    }

    /// The MACHINE's build sheet — applied EVERY BLOCK (macros
    /// are modulated globally; setters are cheap and idempotent).
    #[allow(clippy::too_many_arguments)]
    fn set_machine(
        &mut self,
        mass_ms: f32,
        poles: f32,
        wheel: u16,
        wires: (f32, f32, f32, f32), // (ring_s, stiff, memory, width)
        touch: (usize, f32, f32, f32, f32), // (contact, grit, couple, chaos, force)
        sr: f32,
    ) {
        self.rotor.set(mass_ms, poles, wheel, sr);
        let (ring_s, stiff, memory, width) = wires;
        self.width_target = width;
        self.wire_target = (ring_s, stiff);
        if !self.voiced {
            self.width = width;
            self.voiced = true;
        }
        self.contact = touch.0;
        self.grit = touch.1;
        self.couple = touch.2;
        self.chaos = touch.3;
        self.force = touch.4;
        // STRIKE contact: ~20 ms exponential release.
        self.strike_decay = (-1.0 / (0.020 * sr)).exp();
        self.wires[0].set_memory(memory);
        self.wires[1].set_memory(memory);
    }

    /// Glide ring and stiffness toward the knobs, retuning the sounding
    /// wires as they move (knobs bend a live note). Snaps when unvoiced.
    fn glide_wires(&mut self) {
        let (ring, stiff) = self.wire_voicing;
        let (ring_to, stiff_to) = self.wire_target;
        let next = if ring.is_nan() {
            (ring_to, stiff_to)
        } else {
            (
                ring + self.glide * (ring_to - ring),
                stiff + self.glide * (stiff_to - stiff),
            )
        };
        if next != self.wire_voicing {
            self.wire_voicing = next;
            if self.hz > 0.0 {
                self.wires[0].tune(self.hz, -wire::PAIR_CENTS, next.0, next.1, self.sr);
                self.wires[1].tune(self.hz, wire::PAIR_CENTS, next.0, next.1, self.sr);
            }
        }
    }

    /// The LIFE dials + shared-supply hum clock, per block. The
    /// hum PHASE is engine-mastered so all voices hum coherently
    /// (one supply); heat rides along for the crackle rate.
    fn set_life(
        &mut self,
        life: (f32, f32, f32, f32, f32),
        hum_inc: f32,
        hum_phase: f32,
        heat: f32,
    ) {
        self.life = life;
        self.hum_inc = hum_inc;
        self.hum_phase = hum_phase;
        self.heat_now = heat;
    }

    /// The ensemble's load, applied as a pitch-flattening multiplier
    /// (the supply sags — forge physics as pitch behavior). A
    /// drooping boiler flattens further AND is handed to the drive.
    fn set_load(&mut self, load: f32, sag: f32, pressure: f32) {
        let starved = (1.0 - pressure) * 0.6;
        self.sag_mult_target = (-(sag * (load + starved) * rotor::SAG_RANGE_ST) / 12.0).exp2();
        self.supply = pressure;
    }

    /// Evaluate the routes for THIS voice (per-voice sources read
    /// the voice's own state) and set the displacement targets.
    fn apply_routes(&mut self, routes: &Routes, sv: &matrix::SrcValues) {
        let mut lags = self.lags;
        let d = matrix::evaluate(routes, sv, &mut lags);
        self.lags = lags;
        self.pitch_st_target = d.pitch_st;
        self.level_db_target = d.level_db;
        let scale = d.decay_oct.exp2();
        if (scale - self.decay_scale).abs() > 1e-3 {
            self.decay_scale = scale;
            self.env.set(&self.scaled_env(), self.sr);
        }
    }
}

impl Voice for DynamoVoice {
    fn prepare(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        // ~5 ms displacement glide (block-rate targets, no zipper).
        self.glide = 1.0 - (-1.0 / (0.005 * sample_rate)).exp();
        self.voiced = false;
        self.wire_voicing = (f32::NAN, f32::NAN);
        self.wires[0].prepare(sample_rate);
        self.wires[1].prepare(sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.env.reset();
        self.rotor.reset();
        self.wires[0].reset();
        self.wires[1].reset();
        self.strike_gate = 0.0;
        self.last_body = 0.0;
        self.own_rail = 0.0;
        self.esc_run = 0;
        self.esc_amp = 0.0;
        self.esc_refract = 0;
        self.esc_sense = 0.0;
        self.esc_prev = 0.0;
        self.held = false;
        self.hum_phase = 0.0;
        self.bearing_lp = 0.0;
        self.clank_amp = 0.0;
        self.clank_ph = [0.0; 3];
        self.crackle_wait = 4_800;
        self.tension_st = 0.0;
        self.sag_mult_target = 1.0;
        self.sag_mult = 1.0;
        self.sources.velocity = 0.0;
        self.pitch_st = 0.0;
        self.level_db = 0.0;
        self.pitch_st_target = 0.0;
        self.level_db_target = 0.0;
    }
    fn note_on(&mut self, hz: f32, velocity: f32) {
        self.hz = hz;
        // The rotor is COMMANDED, not set: it spins up from
        // wherever the shaft currently sits (0 from silence, the
        // previous note's speed on a reused voice — the machine
        // keeps spinning; reproducible because reset() stops it).
        self.rotor.command(hz);
        // The wires retune to the note; their STATE persists — a
        // struck body keeps ringing through a retune (reset() is
        // the only silence).
        let (ring_s, stiff) = if self.wire_voicing.0.is_nan() {
            self.wire_target
        } else {
            self.wire_voicing
        };
        self.wires[0].tune(hz, -wire::PAIR_CENTS, ring_s, stiff, self.sr);
        self.wires[1].tune(hz, wire::PAIR_CENTS, ring_s, stiff, self.sr);
        self.strike_gate = 1.0;
        self.held = true;
        // THE CLANK: contact collision modes — the joint's own
        // inharmonic partials, mostly independent of the note (a
        // hammer sounds like a hammer), lightly keyed.
        self.clank_amp = velocity;
        let base = 620.0 + hz * 0.35;
        for (i, r) in [1.0f32, 1.83, 3.19].iter().enumerate() {
            self.clank_inc[i] = core::f32::consts::TAU * base * r / self.sr;
            self.clank_ph[i] = 0.0;
        }
        // The pallet's inertia tracks THIS note's swing.
        self.esc_sense_k = 1.0 - (-core::f32::consts::TAU * hz * 1.2 / self.sr).exp();
        self.env.gate_on();
        self.sources.note_on(hz, velocity);
    }
    fn note_off(&mut self) {
        self.env.gate_off();
        self.held = false;
    }
    fn choke(&mut self) {
        self.env.choke();
    }
    fn active(&self) -> bool {
        // A struck body: alive while the envelope gates the exciter
        // OR any wire still holds energy.
        self.env.active() || self.wires[0].alive() || self.wires[1].alive()
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
        self.sag_mult += self.glide * (self.sag_mult_target - self.sag_mult);
        self.width += self.glide * (self.width_target - self.width);
        self.glide_wires();
        {
            // TENSION: a loud body bends its own pitch sharp and
            // settles as it decays (the real-string nonlinearity —
            // glided like every displacement).
            let (.., tension_dial) = self.life;
            let energy = (self.wires[0].energy() + self.wires[1].energy()).min(1.2);
            let target_st = tension_dial * energy * 0.9;
            self.tension_st += self.glide * (target_st - self.tension_st);
            // Commanded speed = note × matrix displacement × sag ×
            // tension; the rotor's own inertia does the rest.
            let mut hz = self.hz * self.sag_mult;
            let bend = self.pitch_st + self.tension_st;
            if bend.abs() > 1e-4 {
                hz *= (bend / 12.0).exp2();
            }
            // The MOTOR is powered while the envelope runs; when it
            // closes the rotor freewheels DOWN (command 0). Without
            // this the shaft spun at the stale note's speed forever
            // and the speed-scaled bearing noise kept the wires
            // above the alive floor — the field's everlasting drone.
            if self.env.active() {
                self.rotor.command(hz / self.sr);
            } else {
                self.rotor.command(0.0);
            }
        }
        let g = if self.level_db.abs() > 1e-4 {
            db_to_amp(self.level_db)
        } else {
            1.0
        };
        // The envelope gates the EXCITER, not the output: the body
        // rings on after the note ends. HOW the exciter touches the
        // wire is the contact mode — a machine that never lets go
        // is a supersaw; one that strikes and listens is not.
        let env = self.env.tick();
        let gate = match self.contact {
            1 => {
                // STRIKE: ~20 ms of contact, then the body sings.
                self.strike_gate *= self.strike_decay;
                env * self.strike_gate
            }
            2 => {
                // BOW: stick-slip — the faster the wire already
                // moves with the bow, the less energy transfers.
                env * (1.0 - (2.5 * self.last_body).abs().min(0.9))
            }
            _ => env, // GRIND
        };
        // A drooping boiler starves the exciter (breathy under
        // chords); at pressure 1 the factor is exactly 1.0.
        // THE ESCAPEMENT's tooth re-opens the machine's OWN
        // exciter for a quarter period (a raw push into the wires
        // sounded like a second oscillator — field report: the
        // sustained tone must wear the commutation's timbre).
        let mut tooth = 0.0;
        if self.esc_run > 0 {
            self.esc_run -= 1;
            // Half-cosine: smooth in, smooth out (square edges
            // read as broadband buzz, measured).
            let progress = 1.0 - self.esc_run as f32 / self.esc_len as f32;
            tooth = self.esc_amp * (core::f32::consts::PI * progress).sin();
        }
        let mut drive = self.rotor.tick()
            * (gate + tooth)
            * self.sources.velocity
            * g
            * (0.4 + 0.6 * self.supply)
            * (2.0 * self.force);
        // One white draw feeds every noise mouth this sample.
        let (hum, bearing, clank, crackle, _) = self.life;
        let hammering = self.contact == 1 && self.strike_gate > 1e-4;
        let noisy = self.grit > 1e-3 || hammering || bearing > 1e-3 || crackle > 1e-3;
        let white = if noisy {
            self.noise_rng.next_bipolar()
        } else {
            0.0
        };
        if self.grit > 1e-3 {
            drive += white * self.grit * 0.9 * env * self.sources.velocity;
        }
        // The HAMMER is mechanically separate from the motor: a
        // strike on a stopped rotor still speaks.
        if hammering {
            drive += white * self.strike_gate * env * self.sources.velocity * 1.6;
        }
        // THE LIFE SYSTEM — involuntary sounds, every one a
        // function of machine STATE, all through the wires:
        // HUM: the shared supply's ripple — AM on the drive plus an
        // electrical leak, both deepening as the boiler strains.
        if hum > 1e-3 {
            let h = self.hum_phase.sin();
            self.hum_phase += self.hum_inc;
            let strain = 0.3 + 0.7 * (1.0 - self.supply);
            drive *= 1.0 + hum * 0.3 * strain * h;
            drive += hum * 0.12 * strain * h * env * self.sources.velocity;
        }
        // BEARING: rumble + brush hiss that follow the ACTUAL
        // rotor speed; the hiss bursts land at brush transitions.
        if bearing > 1e-3 {
            let speed = (self.rotor.rate() * self.sr / 400.0).min(1.4);
            if speed > 1e-4 {
                self.bearing_lp += 0.03 * (white - self.bearing_lp);
                let hiss = white * self.rotor.brush_gate() * 0.35;
                drive += bearing * speed * (self.bearing_lp * 1.8 + hiss) * 0.5;
            }
        }
        // CLANK: the contact's own collision modes, ~12 ms.
        if clank > 1e-3 && self.clank_amp > 1e-3 {
            let mut burst = 0.0;
            for i in 0..3 {
                burst += self.clank_ph[i].sin();
                self.clank_ph[i] += self.clank_inc[i];
            }
            drive += clank * self.clank_amp * burst * 0.25;
            self.clank_amp *= 1.0 - 1.0 / (0.012 * self.sr);
        }
        // CRACKLE: aging pops whose RATE rides heat + magnetism —
        // on a POWERED voice only (a dead machine doesn't pop; the
        // ungated pops re-struck the ringing wires forever).
        if crackle > 1e-3 && self.env.active() {
            if self.crackle_wait == 0 {
                let age = 0.15 + self.heat_now + self.wires[0].magnetization();
                drive += white.signum() * crackle * 0.8 * age.min(1.5);
                // Next pop: 30–400 ms, sooner on an aged machine.
                let u = (white.abs()).min(1.0);
                self.crackle_wait = (self.sr * (0.03 + 0.37 * u / age.min(1.5))) as u32;
            } else {
                self.crackle_wait -= 1;
            }
        }
        // THE BRIDGE: last sample's ensemble — minus this voice's
        // own share — bleeds into its wires, scaled by the wire's
        // own termination LOSS (bridge admittance: loop gain stays
        // put when the ring knob moves; a flat constant droned).
        if self.couple > 1e-3 {
            let rail = f32::from_bits(self.bridge.load(AtomicOrdering::Relaxed));
            drive += self.couple * 0.6 * self.wires[0].loss() * (rail - self.own_rail);
        }
        let a = self.wires[0].tick(drive);
        let b = self.wires[1].tick(drive);
        self.last_body = 0.5 * (a + b);
        // The share this voice will have contributed to the rail
        // (same soft-clip domain), read back out next sample.
        self.own_rail = self.last_body / (1.0 + self.last_body.abs());
        // THE ESCAPEMENT: on the wire's own positive crossing the
        // pallet releases one tooth of supply energy — phase-locked
        // by construction (entrainment is free: the wire IS the
        // pendulum). Tooth energy scales with the wire's loss, so
        // the self-sustained amplitude is ring-independent (the
        // bridge-admittance principle). Past the entrained regime
        // the clock BREAKS: teeth skip and double stochastically.
        if self.chaos > 1e-3 && self.held {
            self.esc_sense += self.esc_sense_k * (self.last_body - self.esc_sense);
            let crossed = self.esc_prev <= 0.0 && self.esc_sense > 0.0;
            self.esc_prev = self.esc_sense;
            if self.esc_refract > 0 {
                self.esc_refract -= 1;
            } else if crossed {
                let r = self.noise_rng.next_unit();
                let broken = (self.chaos - 0.55).max(0.0) * 1.6;
                let strength = if r < broken {
                    0.0 // skipped tooth
                } else if r > 1.0 - broken * 0.5 {
                    2.0 // double tooth
                } else {
                    1.0
                };
                let loss = self.wires[0].loss();
                let period = (self.sr / self.hz.max(20.0)).max(2.0);
                // Push starts at the crossing — maximum velocity,
                // maximum power transfer (the swing-pushing law) —
                // and the pallet rearms only after 0.6 of the
                // note's period: ONE tooth per swing.
                self.esc_run = (period * 0.25) as u32;
                self.esc_len = self.esc_run.max(1);
                self.esc_refract = (period * 0.6) as u32;
                // The rotor (~±0.7) carries the tooth now: rescale
                // so the sustained equilibrium matches the direct-
                // push calibration.
                self.esc_amp = strength * self.chaos * 8.0 * self.supply * loss;
            }
        }
        // The PAIR is the stereo field: mid/side on the two wires —
        // the body IS the machine's voice, whole (one unit, no
        // parallel dry circuit).
        let mid = 0.5 * (a + b);
        let side = 0.5 * (a - b) * self.width;
        (mid + side, mid - side)
    }
}

pub struct DynamoEngine {
    params: Arc<DynamoParams>,
    output_control: ContinuousControl,
    sr: f32,
    rack: VoiceRack<DynamoVoice, MAX_VOICES>,
    /// Envelope + shape re-voice only when a knob moved. Reset in
    /// activate — the ACTIVATION LAW (a re-prepare wipes what the
    /// params can't see).
    voicing: Watch<4>,
    /// The matrix clock (global source; per-voice sources live in
    /// the notes themselves).
    lfo: ModLfo,
    /// HEAT and the last note played.
    ensemble: Ensemble,
    /// THE SUPPLY: boiler pressure, drained by the ensemble.
    pressure: f32,
    /// The supply's ripple clock (one hum for the whole machine).
    hum_phase: f32,
    /// The GLOBAL macro pass's lag states (motion rows).
    global_lags: [f32; matrix::SLOTS],
    /// THE BRIDGE rail (shared with every voice).
    bridge_rail: Arc<AtomicU32>,
    /// The rings' feed: per-slot live SOURCE values, as
    /// DECLARED taps. Per-voice sources publish the most
    /// recent note's value — the ring dances to what you just
    /// played, the Vital feel.
    bus: Option<Arc<DebugBus>>,
    mod_taps: [Tap; 10],
    /// The DIAGRAM's telemetry: spin, shaft, glow, boiler,
    /// bridge — the machine seen.
    machine_taps: [Tap; 5],
}

impl stave_core::Processor for DynamoEngine {
    type Params = DynamoParams;

    fn new(params: Arc<DynamoParams>) -> Self {
        let bridge_rail: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        Self {
            output_control: ContinuousControl::new(&params.output),
            params,
            sr: 48_000.0,
            rack: {
                let bridge = bridge_rail.clone();
                VoiceRack::new(Steal::Quietest, move |i| {
                    DynamoVoice::with_seed(
                        0x9E37_79B9 ^ (i as u32).wrapping_mul(0x9E37_79B1),
                        bridge.clone(),
                    )
                })
            },
            voicing: Watch::new(),
            lfo: ModLfo::new(833.0, 0xC0FF_EE01),
            ensemble: Ensemble::default(),
            pressure: 1.0,
            hum_phase: 0.0,
            global_lags: [0.0; matrix::SLOTS],
            bridge_rail,
            bus: None,
            mod_taps: [
                Tap::new("mod 1"),
                Tap::new("mod 2"),
                Tap::new("mod 3"),
                Tap::new("mod 4"),
                Tap::new("mod 5"),
                Tap::new("mod 6"),
                Tap::new("mod 7"),
                Tap::new("mod 8"),
                Tap::new("mod 9"),
                Tap::new("mod 10"),
            ],
            machine_taps: [
                Tap::new("spin"),
                Tap::new("shaft"),
                Tap::new("glow"),
                Tap::new("boiler"),
                Tap::new("rail"),
            ],
        }
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        // Register the diagram's probes BEFORE the editor mounts:
        // a Scene binds its probes at mount time, and the first
        // process() block may come after (the rings learned this
        // lazily; a picture needs them at birth).
        self.machine_taps[0].set(&bus, 1.0); // spin: idle = at speed
        self.machine_taps[1].set(&bus, 0.0); // shaft
        self.machine_taps[2].set(&bus, 0.0); // glow
        self.machine_taps[3].set(&bus, 1.0); // boiler: full
        self.machine_taps[4].set(&bus, 0.0); // rail
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
        self.pressure = 1.0;
        self.global_lags = [0.0; matrix::SLOTS];
        self.bridge_rail.store(0, AtomicOrdering::Relaxed);
        self.voicing.reset();
    }

    fn reset(&mut self) {
        // The WHOLE machine powers down (a transport stop must be
        // a known place — accumulated spin/heat/pressure across
        // stops read as creeping nondeterminism in the field).
        self.rack.reset();
        self.ensemble.reset();
        self.pressure = 1.0;
        self.hum_phase = 0.0;
        self.global_lags = [0.0; matrix::SLOTS];
        self.bridge_rail.store(0, AtomicOrdering::Relaxed);
        self.lfo.prepare(self.sr, self.params.lfo_rate.value());
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        // 1. Param changes become structure.
        if self
            .voicing
            .changed([&p.attack, &p.decay, &p.sustain, &p.release])
        {
            let env = AdsrConfig {
                attack_ms: p.attack.value(),
                decay_ms: p.decay.value(),
                sustain: p.sustain.value(),
                release_ms: p.release.value(),
            };
            let sr = self.sr;
            for v in self.rack.voices_mut() {
                v.set_env(env, sr);
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
        // The ensemble's energy feeds SYMP, the boiler, and the
        // global macro pass.
        let mut total_level = 0.0f32;
        for v in self.rack.voices_mut() {
            total_level += v.level();
        }
        // THE GLOBAL MACRO PASS: motion rows evaluate ONCE per
        // block with ensemble source values — one motion animates
        // the machine, not sixteen private copies. (Voice targets
        // in the same table evaluate per voice below; each scope
        // owns its own lag states.)
        let global_sv = self.ensemble.sources(global_lfo, total_level);
        let mut glags = self.global_lags;
        let gd = matrix::evaluate(&routes, &global_sv, &mut glags);
        self.global_lags = glags;
        let force = (p.force.value() + gd.force).clamp(0.0, 1.0);
        let mass = (p.mass.value() * gd.mass_oct.exp2()).clamp(1.0, 8_000.0);
        let memory = (p.memory.value() + gd.memory).clamp(0.0, 1.0);
        let couple = (p.couple.value() + gd.couple).clamp(0.0, 1.0);
        let steam = (p.steam.value() + gd.steam).clamp(0.0, 1.0);
        let chaos = (p.chaos.value() + gd.chaos).clamp(0.0, 1.0);
        // THE LOAD + THE SUPPLY, with the EFFECTIVE steam.
        let drain = (1.0 - steam) * 0.35 * total_level * block_s;
        let recover = (1.0 - self.pressure) * (block_s / 1.2);
        self.pressure = (self.pressure - drain + recover).clamp(0.05, 1.0);
        let sag = p.sag.value();
        let load = (total_level / 4.0).clamp(0.0, 1.0);
        let pressure = self.pressure;
        // The machine's build sheet, applied EVERY block (macros
        // are live; setters are cheap and idempotent).
        {
            let poles = rotor::POLE_COUNTS[p.poles.index().min(6)];
            let wheel = rotor::WHEEL_BITS[p.wheel.index().min(7)];
            let wires = (p.ring.value(), p.stiff.value(), memory, p.width.value());
            let touch = (p.contact.index(), p.grit.value(), couple, chaos, force);
            let sr = self.sr;
            // One supply, one hum: the master phase advances per
            // block; voices integrate the same increment locally,
            // so the whole machine hums coherently at ~53 Hz.
            let hum_inc = core::f32::consts::TAU * 53.0 / sr;
            let hum_phase = self.hum_phase;
            self.hum_phase =
                (self.hum_phase + hum_inc * ctx.frames as f32) % core::f32::consts::TAU;
            let life = (
                p.hum.value(),
                p.bearing.value(),
                p.clank.value(),
                p.crackle.value(),
                p.tension.value(),
            );
            let heat = self.ensemble.heat();
            for v in self.rack.voices_mut() {
                v.set_machine(mass, poles, wheel, wires, touch, sr);
                v.set_load(load, sag, pressure);
                v.set_life(life, hum_inc, hum_phase, heat);
            }
        }
        if routes.any() {
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
            // THE DIAGRAM's telemetry: the machine, seen. The
            // showcase voice is the loudest living one.
            let mut spin = 1.0f32;
            let mut shaft = 0.0f32;
            let mut glow = 0.0f32;
            let mut best = -1.0f32;
            for v in self.rack.voices_mut() {
                let e = v.wires[0].energy() + v.wires[1].energy();
                glow += e;
                let score = v.level() + e;
                if score > best && score > 1e-4 {
                    best = score;
                    spin = v.rotor.spin();
                    shaft = v.rotor.shaft();
                }
            }
            self.machine_taps[0].set(bus, f64::from(spin));
            self.machine_taps[1].set(bus, f64::from(shaft));
            self.machine_taps[2].set(bus, f64::from((glow * 0.6).clamp(0.0, 1.0)));
            self.machine_taps[3].set(bus, f64::from(self.pressure));
            let rail = f32::from_bits(self.bridge_rail.load(AtomicOrdering::Relaxed)).abs();
            self.machine_taps[4].set(bus, f64::from(rail.min(1.0)));
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
        let bridge_rail = self.bridge_rail.clone();
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
            // Write THE BRIDGE rail: the ensemble body, softly
            // clipped (the nonlinear junction), read next sample.
            let sum = 0.5 * (l + r);
            let rail = sum / (1.0 + sum.abs());
            bridge_rail.store(rail.to_bits(), core::sync::atomic::Ordering::Relaxed);
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
