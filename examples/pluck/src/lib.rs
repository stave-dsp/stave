//! Stave Pluck — a plucked-string instrument, and the shape of every
//! stave instrument: notes in, a rack of voices, stereo out.
//!
//! Each note plucks a Karplus–Strong string (`KarplusStrong`, in tune to
//! 0.1 cent and decaying as set: see its rustdoc) tuned by the note's key.
//! A note-off damps the string; eight voices, the quietest stolen.

use stave::dsp::primitives::resonators::KarplusStrong;
use stave::dsp::{hz_of_key, Steal, Voice, VoiceRack};
use stave::prelude::*;

#[derive(Params)]
pub struct PluckParams {
    /// How long a held string rings (its fundamental's T60).
    pub decay: FloatParam,
    /// How much longer the treble rings, as a share of the decay.
    pub brightness: FloatParam,
    pub output: FloatParam,
}

impl Default for PluckParams {
    fn default() -> Self {
        PluckParams {
            decay: FloatParam::new(ParamId(1), "Decay", Range::log(200.0, 8000.0))
                .unit(Unit::Ms)
                .default(2500.0),
            brightness: FloatParam::new(ParamId(2), "Brightness", Range::linear(0.05, 1.0))
                .unit(Unit::Percent)
                .default(0.35),
            output: FloatParam::new(ParamId(3), "Output", Range::linear(-24.0, 6.0))
                .unit(Unit::Db)
                .default(-6.0)
                .smooth(Smooth::ms(5.0)),
        }
    }
}

/// How long a released string takes to fall 60 dB.
const DAMPED: Seconds = seconds(0.12);

/// One string, as a voice.
struct Plucked {
    string: KarplusStrong,
    /// The decay the next pluck rings with (fundamental, treble).
    decay: (Seconds, Seconds),
    /// A follower of the output, for stealing and for `active`.
    level: f32,
    fall: f32,
}

impl Plucked {
    fn new() -> Self {
        Plucked {
            string: KarplusStrong::new(hz(20.0), hz(220.0)),
            decay: (seconds(2.5), seconds(0.9)),
            level: 0.0,
            fall: 0.999,
        }
    }
}

impl Voice for Plucked {
    fn prepare(&mut self, sample_rate: f32) {
        self.string.prepare(sample_rate);
        // The follower falls 60 dB in 50 ms.
        self.fall = (-6.9 / (0.05 * sample_rate)).exp();
    }
    fn reset(&mut self) {
        self.string.reset();
        self.level = 0.0;
    }
    fn note_on(&mut self, frequency: f32, velocity: f32) {
        self.string.reset();
        self.string.set_pitch(hz(frequency));
        self.string.set_decay(self.decay.0, self.decay.1);
        self.string.pluck(0.6 * velocity);
        self.level = velocity;
    }
    fn note_off(&mut self) {
        self.string.set_decay(DAMPED, DAMPED);
    }
    fn choke(&mut self) {
        self.reset();
    }
    fn active(&self) -> bool {
        self.level > 1e-5
    }
    fn level(&self) -> f32 {
        self.level
    }
    fn tick(&mut self) -> (f32, f32) {
        let y = self.string.tick(0.0);
        self.level = (self.level * self.fall).max(y.abs());
        (y, y)
    }
}

pub struct Pluck {
    params: Arc<PluckParams>,
    output: Smoother,
    voices: VoiceRack<Plucked, 8>,
}

impl Plugin for Pluck {
    const INFO: Info = Info {
        id: "com.ryanbahan.stave.pluck",
        name: "Stave Pluck",
        vendor: "Ryan Bahan",
        url: "https://github.com/stave-dsp/stave",
        features: &[Feature::Instrument, Feature::Synthesizer, Feature::Stereo],
        ..Info::INSTRUMENT
    };
    const LAYOUTS: &'static [Layout] = &[Layout::INSTRUMENT];
    /// Eight strings at most, each a delay line and a one-pole: well
    /// under 100 ns a sample on any build host.
    const CPU_BUDGET: Budget = Budget::per_channel(400.0);
    type Params = PluckParams;

    fn new(params: &Arc<PluckParams>, _telemetry: &Telemetry) -> Self {
        Pluck {
            params: params.clone(),
            output: Smoother::new(&params.output),
            voices: VoiceRack::new(Steal::Quietest, |_| Plucked::new()),
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.output.prepare(env);
        self.voices.prepare(env.sample_rate);
    }

    fn reset(&mut self) {
        self.voices.reset();
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let ring = f64::from(self.params.decay.value()) / 1000.0;
        let decay = seconds(ring);
        let treble = seconds(ring * f64::from(self.params.brightness.value()));
        for v in self.voices.voices_mut() {
            v.decay = (decay, treble);
        }
        let mut output = self.output.ramp(block, &self.params.output);
        let events = block.events();
        let mut due = EventCursor::new(events);
        let frames = block.frames();
        let out = block.output();
        let channels = out.channel_count();
        for f in 0..frames {
            for e in due.due(f) {
                let EventKind::Note(n) = e.kind;
                self.voices.dispatch(n.voice_event(hz_of_key));
            }
            let (l, r) = self.voices.tick();
            let gain: Gain = output.next_as();
            out.channel_mut(0)[f] = l * gain;
            if channels > 1 {
                out.channel_mut(1)[f] = r * gain;
            }
        }
    }

    fn tail(&self) -> Tail {
        Tail::Samples(48_000 * 8)
    }
}

stave::export!(Pluck);
