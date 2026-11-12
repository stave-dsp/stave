//! The tolex ENGINE — a cabinet in three components per lane:
//!
//! ```text
//! mod.rs      ← you are here: the orchestration (bind → render)
//! motor.rs    Motor — the lumped electro-mechanical driver core
//!               (leaf: 3 f64 states, trapezoidal semi-implicit,
//!                Bl(x)/Cms(x) large-signal terms, k_box air spring)
//! cabinet.rs  RearPath — the open back's wrap-around radiator
//!               (delay + diffraction lowpass + mic-distance mix)
//! voice.rs    Voice — the cone's modal fingerprint: the fixed
//!               mode set (T(f) ring law Qs), the position system
//!               (weights + comb + macro walk), drive softening,
//!               and the piston cliff
//! ```
//!
//! Bind: the physical build sheet (size/fs/Qts/damping/back/cab)
//! rebuilds on a Watch, block rate. Render: per sample, per lane —
//! program swings the mesh at `drive` volts, the motor answers with
//! far-field pressure, the back door combs it, the voice rolls it.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::{ContinuousControl, Watch};
use stave_core::{DebugBus, Environment, ProcessCtx, Tail, Tap};
use stave_dsp::{db_to_amp, PerChannel};

pub(crate) mod cabinet;
pub(crate) mod drivers;
pub(crate) mod interior;
pub(crate) mod microphone;
pub(crate) mod motor;
pub(crate) mod voice;

use cabinet::RearPath;
use drivers::Drivers;
use interior::Interior;
use microphone::Microphone;
use motor::Motor;
use voice::Voice;

use crate::params::TolexParams;

/// The drive knob's 0 dB reference swing, volts at the mesh.
const V_REF: f32 = 8.0;

/// How often the voice reads the motor's excursion (samples). The
/// envelope itself lags ~10 ms; this just paces the coefficient
/// refresh.
const SOFTEN_INTERVAL: u32 = 64;

/// One audio lane: motor → back door → the voice.
struct Lane {
    motor: Motor,
    chorus: Drivers,
    back: RearPath,
    interior: Interior,
    voice: Voice,
    mic: Microphone,
}

pub struct TolexEngine {
    params: Arc<TolexParams>,
    drive_control: ContinuousControl,
    mic_control: ContinuousControl,
    output_control: ContinuousControl,
    sr: f32,
    lanes: PerChannel<Lane, 2>,
    /// Rebuild-on-change for the physical build sheet. Reset at
    /// activate: rate-derived coefficients are stale state the
    /// params can't see.
    build: Watch<15>,
    soften_phase: u32,
    /// Telemetry (the attach_telemetry seam): the motor's live
    /// operating point, out to the response scene and the travel
    /// strip. Zero-cost with no bus attached.
    bus: Option<Arc<DebugBus>>,
    tap_excursion: Tap,
    tap_travel: Tap,
    /// True between activate and the first bind — structure snaps
    /// instead of gliding (no history to protect yet).
    fresh: bool,
}

impl TolexEngine {
    /// Bind — params become structure, block rate, only on change.
    fn bind(&mut self) {
        let p = &self.params;
        let size = p.size.index();
        let fs = p.resonance.value();
        let qts = p.q.value();
        let d = p.damping.value();
        let rsrc = motor::rsrc_ohms(d);
        let open = p.back.index() == 1;
        let vb_l = p.cab.value();
        let vb_box = if open { None } else { Some(vb_l * 1e-3) };
        let mass = p.mass.value();
        let variant = p.cone.value().round().max(0.0) as usize;
        let material = p.material.index();
        let model = p.model.index();
        let mic_r = p.mic.value() * 0.01;
        let layout = p.speakers.index();
        let (breakup, doping, position) = (p.breakup.value(), p.doping.value(), p.position.value());
        let sr = self.sr;
        for lane in self.lanes.iter_mut() {
            lane.motor.set(size, fs, qts, rsrc, vb_box, mass);
            lane.chorus.set(layout);
            lane.back.set(open, vb_l, sr);
            lane.interior.set(vb_l, material, open, layout, sr);
            lane.voice
                .set(size, variant, mass, breakup, doping, position);
            lane.mic.set(model, mic_r);
            if self.fresh {
                lane.back.snap();
            }
        }
        self.fresh = false;
    }
}

impl stave_core::Processor for TolexEngine {
    type Params = TolexParams;

    fn new(params: Arc<TolexParams>) -> Self {
        Self {
            drive_control: ContinuousControl::new(&params.drive),
            mic_control: ContinuousControl::new(&params.mic),
            output_control: ContinuousControl::new(&params.output),
            params,
            sr: 48_000.0,
            lanes: PerChannel::new(|_| Lane {
                motor: Motor::new(),
                chorus: Drivers::new(),
                back: RearPath::new(),
                interior: Interior::new(),
                voice: Voice::new(),
                mic: Microphone::new(),
            }),
            build: Watch::new(),
            soften_phase: 0,
            fresh: true,
            bus: None,
            tap_excursion: Tap::new("excursion"),
            tap_travel: Tap::new("travel"),
        }
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        // Register BEFORE the editor mounts (the dynamo lesson: a
        // Scene binds probes at mount; first-process is too late).
        self.tap_excursion.set(&bus, 0.0);
        self.tap_travel.set(&bus, 0.0);
        self.bus = Some(bus);
    }

    fn activate(&mut self, env: Environment) {
        let rate = f64::from(env.sample_rate);
        self.drive_control.prepare(rate, &self.params.drive);
        self.mic_control.prepare(rate, &self.params.mic);
        self.output_control.prepare(rate, &self.params.output);
        self.sr = env.sample_rate;
        for lane in self.lanes.iter_mut() {
            lane.motor.prepare(self.sr);
            lane.chorus.prepare(self.sr);
            lane.back.prepare(self.sr);
            lane.interior.prepare(self.sr);
            lane.voice.prepare(self.sr);
            lane.mic.prepare(self.sr);
        }
        self.build.reset();
        self.fresh = true;
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        if self.build.changed([
            &p.size,
            &p.resonance,
            &p.q,
            &p.damping,
            &p.back,
            &p.cab,
            &p.material,
            &p.model,
            &p.speakers,
            &p.mass,
            &p.breakup,
            &p.doping,
            &p.position,
            &p.mic,
            &p.cone,
        ]) {
            self.bind();
        }

        let mut drive = self.drive_control.begin_block(ctx.params, &p.drive);
        let mut mic = self.mic_control.begin_block(ctx.params, &p.mic);
        let mut out_ramp = self.output_control.begin_block(ctx.params, &p.output);
        let lanes_state = &mut self.lanes;
        let lanes = ctx.lanes::<2>();
        let active = lanes.active();
        let mut soften_phase = self.soften_phase;
        lanes.run(|fr| {
            let vd = V_REF * db_to_amp(drive.next());
            let mic_r = mic.next() * 0.01; // cm → m
            let g = db_to_amp(out_ramp.next());
            let soften_now = soften_phase == 0;
            soften_phase = (soften_phase + 1) % SOFTEN_INTERVAL;
            for lane in 0..active.min(2) {
                let s = &mut lanes_state[lane];
                let pressure = s.chorus.tick(s.motor.tick(fr.input(lane), vd), mic_r);
                if soften_now {
                    s.voice.soften(s.motor.work());
                }
                let at_mic = s.interior.tick(s.back.tick(pressure, mic_r));
                fr.write(lane, s.mic.tick(s.voice.tick(at_mic)) * g);
            }
        });
        self.soften_phase = soften_phase;

        // Observe: the block's operating point (max across lanes).
        if let Some(bus) = &self.bus {
            let mut work = 0.0f32;
            let mut travel = 0.0f32;
            for lane in 0..active.min(2) {
                work = work.max(lanes_state[lane].motor.work());
                travel = travel.max(lanes_state[lane].motor.excursion());
            }
            self.tap_excursion.set(bus, f64::from(work));
            // Travel in its PHYSICAL unit: |x| as a fraction of the
            // Bl-droop scale (env is the squared, smoothed fraction —
            // sqrt puts normal playing at a visible few percent
            // instead of an invisible squared sliver).
            self.tap_travel
                .set(bus, f64::from(travel.max(0.0).sqrt().clamp(0.0, 1.0)));
        }
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        // The motor rings at its resonance (Q up to ~6 under tube
        // damping) and the wrap path adds ~2 ms — a quarter second
        // covers the worst alignment with margin.
        Tail::Samples((self.sr * 0.25) as u32)
    }
}
