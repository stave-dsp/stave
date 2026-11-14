//! The physical and converter models as reference stages: the EMT 140
//! plate, the Leslie 122H, Chowdhury's tape machine with its transport
//! and tape condition, the opto compressor, the samplers' converters,
//! the quantiser, a close-miked guitar speaker, and two instruments that
//! play themselves (stave-play has no note input: they are excited every
//! `interval`, and by the audio at their contact point).
//!
//! | stage | settings |
//! |---|---|
//! | `reverb/plate_emt140` | t60 (s at 500 Hz), pickup (left, right, both), output (velocity, acceleration), mix |
//! | `rotary/leslie_122h` | speed (chorale, tremolo, stop), toggle_s (flip chorale/tremolo every so often: hear the motors ramp), mic (left, right, both) |
//! | `tape/chowdhury` | transport (off, studio, consumer), condition (none, fresh, used, worn), speed (3.75, 7.5, 15 ips), bias, flutter, drive, stop_s (stop and restart the motor every so often) |
//! | `dynamics/opto` | cell (VTL5C3, NSL-32), full_scale_in, full_scale_out |
//! | `converter/sampler` | machine (SP-1200, S900, CD), drive |
//! | `quantise/dither` | bits, dither (none, rectangular, triangular, highpass triangular), shaping (none, first, second order) |
//! | `cabinet/close_mic` | speaker (Jensen C12N, P12N, C12K, P10R), distance, offset, volts, source_ohms |
//! | `instrument/string` | pitch, excitation (pluck, strike, bow), interval, position, t60, treble_t60, inharmonicity |
//! | `instrument/drum` | interval, t60, treble_t60, strike_at |
//!
//! Every variant a choice selects is built with the stage, so a setting
//! change only picks one or sets it in place: no allocation after
//! `prepare`.

use stave_core::params::Unit;
use stave_dsp::circuit::devices::converter::{Converter, Machine as Sampler};
use stave_dsp::circuit::devices::photocell::Najnudel;
use stave_dsp::circuit::topologies::opto_compressor::OptoCompressor;
use stave_dsp::physical::cabinet::{Breakup, CloseMic, Placement};
use stave_dsp::physical::membrane::{Drumhead, Membrane, Polar};
use stave_dsp::physical::plate::{Loss, Output, Plate};
use stave_dsp::physical::rotary::{Cabinet, Speed};
use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
use stave_dsp::physical::string::{Bow, Waveguide};
use stave_dsp::physical::tape::{Capstan, Chowdhury, Condition, Drift, Machine, Motor, Spacing, Splices};
use stave_dsp::primitives::quantise::{Dither, Quantiser, Shaping};
use stave_dsp::units::{hz, seconds, Hz};
use stave_dsp::{Stage, Tail};

use super::catalog::{choice, choice_at, float, stage};
use super::{BoxedStage, StageParams};

const PLAIN: Unit = Unit::Plain { decimals: 3 };
const SECONDS: Unit = Unit::Custom { label: "s", decimals: 2 };
const DB: Unit = Unit::Db;

fn db_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Every prebuilt variant of a stage, the selected one running.
#[derive(Clone)]
struct Pick<S> {
    all: Vec<S>,
    selected: usize,
}

impl<S: Stage> Pick<S> {
    fn new(all: Vec<S>) -> Self {
        Self { all, selected: 0 }
    }

    fn select(&mut self, i: usize) {
        let i = i.min(self.all.len() - 1);
        if i != self.selected {
            self.selected = i;
            self.all[i].reset();
        }
    }

    fn current(&mut self) -> &mut S {
        &mut self.all[self.selected]
    }
}

impl<S: Stage> Stage for Pick<S> {
    fn prepare(&mut self, sample_rate: f32) {
        self.all.iter_mut().for_each(|s| s.prepare(sample_rate));
    }
    fn reset(&mut self) {
        self.current().reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.current().tick(x)
    }
    fn latency(&self) -> u32 {
        self.all[self.selected].latency()
    }
    fn tail(&self) -> Tail {
        self.all[self.selected].tail()
    }
}

/// Which of two outputs: the first, the second, or their mean.
fn pair(which: usize, (l, r): (f32, f32)) -> f32 {
    match which {
        0 => l,
        1 => r,
        _ => 0.5 * (l + r),
    }
}

options_list!(
    /// Which of a stereo model's two outputs.
    Side = ["left", "right", "both"]
);

/// A countdown that fires every `period` seconds (never at 0).
#[derive(Copy, Clone, Default)]
struct Every {
    period: f64,
    left: f64,
    step: f64,
}

impl Every {
    fn prepare(&mut self, sample_rate: f32) {
        self.step = 1.0 / f64::from(sample_rate);
        self.left = 0.0;
    }

    /// One sample on: whether it fires now (at once when restarted).
    fn tick(&mut self) -> bool {
        if self.period <= 0.0 {
            return false;
        }
        self.left -= self.step;
        if self.left <= 0.0 {
            self.left += self.period;
            if self.left <= 0.0 {
                self.left = self.period;
            }
            return true;
        }
        false
    }
}

// --- EMT 140 ----------------------------------------------------------------

options_list!(
    /// What the plate's pickups read.
    PlateOutput = ["velocity", "acceleration"]
);

/// Modes kept: to 5 kHz (3 µs a sample, stereo, at 48 kHz).
const PLATE_TOP: f32 = 5_000.0;

struct PlateStage {
    plate: Plate,
    side: usize,
    mix: f32,
}

impl PlateStage {
    fn configure(&mut self, v: &StageParams) {
        if let Some(loss) = Loss::emt140(seconds(v.get("t60"))) {
            self.plate.set_loss(loss);
        }
        self.plate.set_output([Output::Velocity, Output::Acceleration][v.choice("output").min(1)]);
        self.side = v.choice("pickup");
        self.mix = v.f32("mix");
    }
}

impl Stage for PlateStage {
    fn prepare(&mut self, sample_rate: f32) {
        self.plate.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.plate.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        let wet = pair(self.side, self.plate.tick_stereo(x));
        x + self.mix * (wet - x)
    }
    fn process(&mut self, block: &mut [f32]) {
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        for chunk in block.chunks_mut(64) {
            let n = chunk.len();
            self.plate.process_stereo(chunk, &mut l[..n], &mut r[..n]);
            for (i, x) in chunk.iter_mut().enumerate() {
                let wet = pair(self.side, (l[i], r[i]));
                *x += self.mix * (wet - *x);
            }
        }
    }
    fn tail(&self) -> Tail {
        self.plate.tail()
    }
}

pub fn plate_params() -> StageParams {
    StageParams::new(vec![
        float(0, "t60", 0.8, 5.0, 2.5, SECONDS),
        choice::<Side>(1, "pickup"),
        choice::<PlateOutput>(2, "output"),
        float(3, "mix", 0.0, 1.0, 1.0, PLAIN),
    ])
}

pub fn plate(v: &StageParams) -> BoxedStage {
    let mut plate = Plate::emt140(seconds(v.get("t60")));
    plate.set_bandwidth(Hz::new(PLATE_TOP));
    let mut s = PlateStage { plate, side: 0, mix: 1.0 };
    s.configure(v);
    stage(s, PlateStage::configure)
}

// --- Leslie 122H ------------------------------------------------------------

options_list!(
    /// The rotors' speed switch.
    LeslieSpeed = ["chorale", "tremolo", "stop"]
);

struct Leslie {
    cabinet: Cabinet,
    speed: Speed,
    toggle: Every,
    mic: usize,
}

impl Leslie {
    fn configure(&mut self, v: &StageParams) {
        let speed = [Speed::Chorale, Speed::Tremolo, Speed::Stop][v.choice("speed").min(2)];
        if speed != self.speed {
            self.speed = speed;
            self.cabinet.set_speed(speed);
        }
        let period = v.get("toggle_s");
        if period != self.toggle.period {
            self.toggle.period = period;
            self.toggle.left = period;
        }
        self.mic = v.choice("mic");
    }
}

impl Stage for Leslie {
    fn prepare(&mut self, sample_rate: f32) {
        self.cabinet.prepare(sample_rate);
        self.cabinet.set_speed(self.speed);
        self.toggle.prepare(sample_rate);
        self.toggle.left = self.toggle.period;
    }
    fn reset(&mut self) {
        self.cabinet.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.toggle.tick() {
            self.speed = match self.speed {
                Speed::Tremolo => Speed::Chorale,
                _ => Speed::Tremolo,
            };
            self.cabinet.set_speed(self.speed);
        }
        pair(self.mic, self.cabinet.tick_stereo(x))
    }
    fn latency(&self) -> u32 {
        self.cabinet.latency()
    }
    fn tail(&self) -> Tail {
        self.cabinet.tail()
    }
}

pub fn leslie_params() -> StageParams {
    StageParams::new(vec![
        choice::<LeslieSpeed>(0, "speed"),
        float(1, "toggle_s", 0.0, 30.0, 0.0, SECONDS),
        choice::<Side>(2, "mic"),
    ])
}

pub fn leslie(v: &StageParams) -> BoxedStage {
    let mut s = Leslie {
        cabinet: Cabinet::leslie_122h(),
        speed: Speed::Chorale,
        toggle: Every::default(),
        mic: 0,
    };
    s.configure(v);
    stage(s, Leslie::configure)
}

// --- Tape ---------------------------------------------------------------------

options_list!(
    /// The transport preset (DSP.md H5).
    Transport = ["off", "studio", "consumer"]
);
options_list!(
    /// The tape's condition (DSP.md H5).
    TapeCondition = ["none", "fresh", "used", "worn"]
);
options_list!(
    /// The tape speed.
    TapeSpeed = ["3.75 ips", "7.5 ips", "15 ips"]
);

const TAPE_IPS: [f64; 3] = [3.75, 7.5, 15.0];
const CONDITIONS: [Condition; 4] = [Condition::NONE, Condition::FRESH, Condition::USED, Condition::WORN];

/// A TC-260 with transport `t` and condition `c`.
fn tape_machine(t: usize, c: usize) -> Chowdhury {
    let mut tape = Chowdhury::new(Machine::TC260);
    let seed = 0x7a9e;
    tape = match t {
        1 => tape.with_capstan(Capstan::new(seed).with_drift(Drift::STEADY).with_motor(Motor::STUDIO)),
        2 => tape.with_capstan(
            Capstan::new(seed)
                .with_drift(Drift::LOOSE)
                .with_motor(Motor::CONSUMER)
                .with_splices(Splices::EDITED),
        ),
        _ => tape,
    };
    if c > 0 {
        tape = tape.with_spacing(Spacing::new(CONDITIONS[c], seed));
    }
    tape
}

struct Tape {
    machines: Pick<Chowdhury>,
    drive: f32,
    stop: Every,
    stopped: bool,
}

impl Tape {
    fn configure(&mut self, v: &StageParams) {
        let i = v.choice("transport") * CONDITIONS.len() + v.choice("condition");
        self.machines.select(i);
        let speed = Machine::ips(TAPE_IPS[v.choice("speed").min(2)]);
        let (bias, flutter) = (v.get("bias"), v.get("flutter"));
        for m in &mut self.machines.all {
            m.set_speed(speed);
            m.set_bias(bias);
            m.set_flutter(flutter);
        }
        self.drive = db_gain(v.get("drive"));
        let period = v.get("stop_s");
        if period != self.stop.period {
            self.stop.period = period;
            self.stop.left = period;
            if period <= 0.0 && self.stopped {
                self.stopped = false;
                if let Some(c) = self.machines.current().capstan() {
                    c.start();
                }
            }
        }
    }
}

impl Stage for Tape {
    fn prepare(&mut self, sample_rate: f32) {
        self.machines.prepare(sample_rate);
        self.stop.prepare(sample_rate);
        self.stop.left = self.stop.period;
        self.stopped = false;
    }
    fn reset(&mut self) {
        self.machines.reset();
        self.stopped = false;
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.stop.tick() {
            self.stopped = !self.stopped;
            let stopped = self.stopped;
            if let Some(c) = self.machines.current().capstan() {
                if stopped {
                    c.stop();
                } else {
                    c.start();
                }
            }
        }
        self.machines.tick(x * self.drive) / self.drive
    }
    fn latency(&self) -> u32 {
        self.machines.latency()
    }
    fn tail(&self) -> Tail {
        self.machines.tail()
    }
}

pub fn tape_params() -> StageParams {
    StageParams::new(vec![
        choice::<Transport>(0, "transport"),
        choice::<TapeCondition>(1, "condition"),
        // 7.5 ips, the TC-260's own speed.
        choice_at::<TapeSpeed>(2, "speed", 1),
        float(3, "bias", 0.0, 10.0, 5.0, PLAIN),
        float(4, "flutter", 0.0, 2.0, 1.0, PLAIN),
        float(5, "drive", -24.0, 18.0, 0.0, DB),
        float(6, "stop_s", 0.0, 30.0, 0.0, SECONDS),
    ])
}

pub fn tape(v: &StageParams) -> BoxedStage {
    let all = (0..3).flat_map(|t| (0..CONDITIONS.len()).map(move |c| tape_machine(t, c))).collect();
    let mut s = Tape {
        machines: Pick::new(all),
        drive: 1.0,
        stop: Every::default(),
        stopped: false,
    };
    s.configure(v);
    stage(s, Tape::configure)
}

// --- Opto compressor --------------------------------------------------------

options_list!(
    /// The opto cell.
    OptoCell = ["VTL5C3", "NSL-32"]
);

struct Opto {
    cells: Pick<OptoCompressor>,
    full_scale_in: f64,
    full_scale_out: f64,
}

impl Opto {
    fn configure(&mut self, v: &StageParams) {
        self.cells.select(v.choice("cell"));
        self.full_scale_in = v.get("full_scale_in");
        self.full_scale_out = v.get("full_scale_out");
    }
}

impl Stage for Opto {
    fn prepare(&mut self, sample_rate: f32) {
        self.cells.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.cells.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        let fs = self.full_scale_in;
        (self.cells.current().tick_f64(f64::from(x) * fs) / self.full_scale_out) as f32
    }
}

pub fn opto_params() -> StageParams {
    let volts = Unit::Custom { label: "V/FS", decimals: 2 };
    StageParams::new(vec![
        choice::<OptoCell>(0, "cell"),
        float(1, "full_scale_in", 0.5, 12.0, 6.0, volts),
        float(2, "full_scale_out", 0.5, 12.0, 3.0, volts),
    ])
}

pub fn opto(v: &StageParams) -> BoxedStage {
    let mut s = Opto {
        cells: Pick::new(vec![OptoCompressor::new(Najnudel::VTL5C3), OptoCompressor::new(Najnudel::NSL32)]),
        full_scale_in: 1.0,
        full_scale_out: 1.0,
    };
    s.configure(v);
    stage(s, Opto::configure)
}

// --- Converters -------------------------------------------------------------

options_list!(
    /// The sampler whose converters are modelled.
    SamplerChoice = ["SP-1200", "S900", "CD"]
);

struct Sampling {
    converters: Pick<Converter>,
    drive: f32,
}

impl Stage for Sampling {
    fn prepare(&mut self, sample_rate: f32) {
        self.converters.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.converters.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.converters.tick(x * self.drive) / self.drive
    }
    fn latency(&self) -> u32 {
        self.converters.latency()
    }
}

pub fn sampler_params() -> StageParams {
    StageParams::new(vec![choice::<SamplerChoice>(0, "machine"), float(1, "drive", -24.0, 12.0, 0.0, DB)])
}

pub fn sampler(v: &StageParams) -> BoxedStage {
    let configure = |s: &mut Sampling, v: &StageParams| {
        s.converters.select(v.choice("machine"));
        s.drive = db_gain(v.get("drive"));
    };
    let mut s = Sampling {
        converters: Pick::new([Sampler::SP1200, Sampler::S900, Sampler::CD].map(Converter::new).to_vec()),
        drive: 1.0,
    };
    configure(&mut s, v);
    stage(s, configure)
}

// --- Quantiser --------------------------------------------------------------

const BITS: [u32; 7] = [4, 6, 8, 10, 12, 16, 24];
const DITHERS: [Dither; 4] = [Dither::None, Dither::Rectangular, Dither::Triangular, Dither::HighpassTriangular];
const SHAPINGS: [Shaping; 3] = [Shaping::NONE, Shaping::FIRST_ORDER, Shaping::SECOND_ORDER];

options_list!(
    /// The word length.
    Bits = ["4", "6", "8", "10", "12", "16", "24"]
);
options_list!(
    /// The dither.
    DitherChoice = ["none", "rectangular", "triangular", "highpass triangular"]
);
options_list!(
    /// The error feedback's shaping.
    ShapingChoice = ["none", "first order", "second order"]
);

pub fn quantiser_params() -> StageParams {
    // 8 bits, triangular: the dither audible.
    StageParams::new(vec![
        choice_at::<Bits>(0, "bits", 2),
        choice_at::<DitherChoice>(1, "dither", 2),
        choice::<ShapingChoice>(2, "shaping"),
    ])
}

pub fn quantiser(v: &StageParams) -> BoxedStage {
    let all = BITS
        .iter()
        .flat_map(|&b| {
            DITHERS.iter().flat_map(move |&d| {
                SHAPINGS.iter().map(move |&s| {
                    let q = Quantiser::new(b).with_dither(d, 0x5eed);
                    q.clone().with_shaping(s).unwrap_or(q)
                })
            })
        })
        .collect();
    let configure = |q: &mut Pick<Quantiser>, v: &StageParams| {
        q.select((v.choice("bits") * DITHERS.len() + v.choice("dither")) * SHAPINGS.len() + v.choice("shaping"));
    };
    let mut s = Pick::new(all);
    configure(&mut s, v);
    stage(s, configure)
}

// --- Close mic ----------------------------------------------------------------

options_list!(
    /// The guitar speaker (its impedance-curve fit).
    Speaker = ["Jensen C12N", "Jensen P12N", "Jensen C12K", "Jensen P10R"]
);

const SPEAKERS: [ThieleSmall; 4] = [
    ThieleSmall::JENSEN_C12N_LR2,
    ThieleSmall::JENSEN_P12N_LR2,
    ThieleSmall::JENSEN_C12K_LR2,
    ThieleSmall::JENSEN_P10R_LR2,
];

struct Miked {
    mics: Pick<CloseMic>,
}

impl Miked {
    fn configure(&mut self, v: &StageParams) {
        self.mics.select(v.choice("speaker"));
        let placement = Placement {
            distance: v.get("distance") * 1e-2,
            offset: v.get("offset") * 1e-2,
            ..Placement::CLOSE
        };
        let (volts, ohms) = (v.get("volts"), v.get("source_ohms"));
        for m in &mut self.mics.all {
            m.set_placement(placement);
            m.driver().set_volts(volts);
            m.driver().set_source_ohms(ohms);
        }
    }
}

impl Stage for Miked {
    fn prepare(&mut self, sample_rate: f32) {
        self.mics.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.mics.reset();
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.mics.tick(x)
    }
    fn latency(&self) -> u32 {
        self.mics.latency()
    }
    fn tail(&self) -> Tail {
        self.mics.tail()
    }
}

pub fn close_mic_params() -> StageParams {
    let cm = Unit::Custom { label: "cm", decimals: 1 };
    StageParams::new(vec![
        choice::<Speaker>(0, "speaker"),
        float(1, "distance", 1.0, 50.0, 2.5, cm),
        float(2, "offset", 0.0, 15.0, 0.0, cm),
        float(3, "volts", 0.1, 40.0, 10.0, Unit::Custom { label: "V/FS", decimals: 1 }),
        float(4, "source_ohms", 0.0, 64.0, 0.0, Unit::Custom { label: "Ω", decimals: 1 }),
    ])
}

pub fn close_mic(v: &StageParams) -> BoxedStage {
    let mics = SPEAKERS
        .iter()
        .map(|&ts| CloseMic::new(Driver::new(ts, LargeSignal::LINEAR), Placement::CLOSE, Breakup::PAPER_12))
        .collect();
    let mut s = Miked { mics: Pick::new(mics) };
    s.configure(v);
    stage(s, Miked::configure)
}

// --- Instruments ------------------------------------------------------------

options_list!(
    /// How the string is played.
    Excitation = ["pluck", "strike", "bow"]
);

struct StringStage {
    string: Waveguide,
    excitation: usize,
    every: Every,
}

impl StringStage {
    fn configure(&mut self, v: &StageParams) {
        let s = &mut self.string;
        s.set_pitch(hz(v.f32("pitch")));
        s.set_position(v.get("position"));
        let t60 = v.get("t60");
        s.set_decay(seconds(t60), seconds(v.get("treble_t60").min(t60)));
        s.set_inharmonicity(v.get("inharmonicity"));
        self.excitation = v.choice("excitation");
        s.set_bow((self.excitation == 2).then_some(Bow { force: 0.6, velocity: 0.2 }));
        self.every.period = v.get("interval");
    }
}

impl Stage for StringStage {
    fn prepare(&mut self, sample_rate: f32) {
        self.string.prepare(sample_rate);
        self.every.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.string.reset();
        self.every.left = 0.0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.every.tick() {
            match self.excitation {
                0 => self.string.pluck(0.5, seconds(0.001)),
                1 => self.string.strike(0.5, seconds(0.002)),
                _ => {}
            }
        }
        self.string.tick(x)
    }
    fn tail(&self) -> Tail {
        self.string.tail()
    }
}

pub fn string_params() -> StageParams {
    StageParams::new(vec![
        float(0, "pitch", 40.0, 1_000.0, 110.0, Unit::Hz),
        choice::<Excitation>(1, "excitation"),
        float(2, "interval", 0.0, 10.0, 1.5, SECONDS),
        float(3, "position", 0.02, 0.5, 0.13, PLAIN),
        float(4, "t60", 0.1, 10.0, 3.0, SECONDS),
        float(5, "treble_t60", 0.05, 10.0, 0.6, SECONDS),
        float(6, "inharmonicity", 0.0, 0.002, 0.0, Unit::Plain { decimals: 5 }),
    ])
}

pub fn string(v: &StageParams) -> BoxedStage {
    let mut s = StringStage {
        string: Waveguide::new(hz(20.0), hz(110.0)),
        excitation: 0,
        every: Every::default(),
    };
    s.configure(v);
    stage(s, StringStage::configure)
}

struct Drum {
    head: Membrane,
    every: Every,
}

impl Drum {
    fn configure(&mut self, v: &StageParams) {
        let t60 = v.get("t60");
        if let Some(loss) = Loss::two_point(hz(110.0), seconds(t60), hz(2_000.0), seconds(v.get("treble_t60").min(t60))) {
            self.head.set_loss(loss);
        }
        self.head.set_drive(Polar::new(v.get("strike_at"), 0.0));
        self.every.period = v.get("interval");
    }
}

impl Stage for Drum {
    fn prepare(&mut self, sample_rate: f32) {
        self.head.prepare(sample_rate);
        self.every.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.head.reset();
        self.every.left = 0.0;
    }
    fn tick(&mut self, x: f32) -> f32 {
        let hit = if self.every.tick() { 1.0 } else { 0.0 };
        self.head.tick(x + hit)
    }
    fn tail(&self) -> Tail {
        self.head.tail()
    }
}

pub fn drum_params() -> StageParams {
    StageParams::new(vec![
        float(0, "interval", 0.0, 10.0, 1.0, SECONDS),
        float(1, "t60", 0.1, 5.0, 1.2, SECONDS),
        float(2, "treble_t60", 0.05, 5.0, 0.3, SECONDS),
        float(3, "strike_at", 0.0, 0.95, 0.33, PLAIN),
    ])
}

/// An 18 cm tom head tuned to 110 Hz.
pub fn drum(v: &StageParams) -> BoxedStage {
    let loss = Loss::two_point(hz(110.0), seconds(1.2), hz(2_000.0), seconds(0.3)).unwrap_or(Loss::DAFX26);
    let mut s = Drum {
        head: Membrane::new(Drumhead::tuned(0.18, 0.26, hz(110.0)), loss),
        every: Every::default(),
    };
    s.configure(v);
    stage(s, Drum::configure)
}
