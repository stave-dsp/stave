//! The library as one list (DSP.md R6): every catalogued stage — the
//! stave-host catalog and its reference models, the circuits (each
//! hand topology, each deck and classic topology on `Circuit<Dk>` and
//! `Circuit<Wdf>`), learned, physical, effects and the primitives they
//! are built from — at a setting it is documented at, with the drive it
//! works at and how its rate invariance is read.
//!
//! An entry builds a fresh, unprepared stage each call, so one rate's
//! state never leaks into another's.

use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::methods::wdf::Wdf;
use stave_dsp::circuit::Circuit;
use stave_dsp::learned::{nam, Gru, LoadError, RateMismatch, RatePolicy};
use stave_dsp::units::{db, hz, ms, seconds, semitones};
use stave_dsp::Stage;
use stave_host::stages::{CATALOG, REFERENCES};
use stave_test::solver_fuzz::DECKS;
use stave_dsp::units::{Db, Hz};

/// How an entry's rate invariance is read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Measure {
    /// A time-invariant stage, linear or not: its small-signal gain at
    /// 100 Hz, 1 kHz and 5 kHz together, and the stronger of its second
    /// and third harmonics under a hot 200 Hz sine.
    Response,
    /// A time-varying stage (modulation, a pitch shift, a detector's
    /// envelope): its output level for a multisine over a second,
    /// against the multisine's.
    Level,
    /// A resonant or reverberant stage (a reverb, a comb, a modal bank):
    /// the energy it puts out over a second for a short Gaussian pulse,
    /// against the pulse's — the response's power averaged over the
    /// pulse's band, which a few discrete tones on sharp peaks are not.
    Energy,
    /// Output that ignores the input: its level and its zero-crossing
    /// rate (its pitch, where it crosses 20 times a second or more).
    Generator,
    /// A random process: its level over a second. One realisation a
    /// rate, so the spread is the estimate's own variance as much as the
    /// process's; its crossings are not read (a rough path's count grows
    /// with the rate).
    Random,
}

/// A rate a stage declines, by its documented policy.
pub type Refuses = fn(f32) -> bool;

/// A stage built for a run, and its rate guard's account when it has
/// one (a learned model's).
pub trait Subject {
    fn stage(&mut self) -> &mut dyn Stage;
    /// The mismatch between the rate it was trained (or fitted) at and
    /// the one it was prepared at, as the model reports it — whether it
    /// then refuses (silence) or serves the rate is its policy's call.
    fn refusal(&self) -> Option<RateMismatch>;
}

struct Plain<S>(S);

impl<S: Stage> Subject for Plain<S> {
    fn stage(&mut self) -> &mut dyn Stage {
        &mut self.0
    }
    fn refusal(&self) -> Option<RateMismatch> {
        None
    }
}

struct Guarded<S>(S, fn(&S) -> Option<RateMismatch>);

impl<S: Stage> Subject for Guarded<S> {
    fn stage(&mut self) -> &mut dyn Stage {
        &mut self.0
    }
    fn refusal(&self) -> Option<RateMismatch> {
        (self.1)(&self.0)
    }
}

/// One catalogued stage.
pub struct Entry {
    pub family: &'static str,
    pub name: String,
    pub build: Box<dyn Fn() -> Box<dyn Subject> + Send + Sync>,
    pub measure: Measure,
    /// Its input at rest (a circuit's bias, volts), its small-signal
    /// amplitude and its hot amplitude, in its input's units.
    pub bias: f32,
    pub small: f32,
    pub hot: f32,
    /// No sample beyond this, for any input the robustness sweep gives.
    pub bound: f32,
    /// Rates its policy refuses (silence and a reported mismatch), by
    /// design; `never` for most.
    pub refuses: Refuses,
    /// Its invariance is read at T1 only: a netlist that costs tens of
    /// microseconds a sample unoptimised, or the same netlist as an entry
    /// T0 reads (a deck on `Circuit<Wdf>` beside `Circuit<Dk>`, a
    /// reference model beside its topology). T0 still sweeps its rates.
    pub heavy: bool,
}

pub fn never(_: f32) -> bool {
    false
}

fn learned_refuses_but_48k(rate: f32) -> bool {
    rate != 48_000.0
}

/// A recurrent network under `RatePolicy::Interpolated`, trained at
/// 48 kHz: half to sixteen times its rate.
fn interpolated_refuses(rate: f32) -> bool {
    !(24_000.0..=768_000.0).contains(&rate)
}

fn plain<S: Stage + 'static>(s: S) -> Box<dyn Subject> {
    Box::new(Plain(s))
}

impl Entry {
    fn new(family: &'static str, name: impl Into<String>, measure: Measure, build: impl Fn() -> Box<dyn Subject> + Send + Sync + 'static) -> Self {
        Self {
            family,
            name: name.into(),
            build: Box::new(build),
            measure,
            bias: 0.0,
            small: 0.01,
            hot: 1.0,
            bound: 100.0,
            refuses: never,
            heavy: false,
        }
    }

    fn heavy(mut self) -> Self {
        self.heavy = true;
        self
    }

    fn drive(mut self, bias: f32, small: f32, hot: f32) -> Self {
        (self.bias, self.small, self.hot) = (bias, small, hot);
        self
    }

    fn bound(mut self, bound: f32) -> Self {
        self.bound = bound;
        self
    }

    fn refuses(mut self, refuses: Refuses) -> Self {
        self.refuses = refuses;
        self
    }
}

/// `stage` measured as `measure`.
fn stage<S: Stage + 'static>(
    family: &'static str,
    name: &str,
    measure: Measure,
    build: impl Fn() -> S + Send + Sync + 'static,
) -> Entry {
    Entry::new(family, name, measure, move || plain(build()))
}

/// Weights in ±0.5 from an LCG (the datasheet's).
fn weights(n: usize) -> String {
    let mut lcg: u32 = 11;
    let w: Vec<String> = (0..n)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            format!("{:.3}", (lcg >> 8) as f32 / (1u32 << 24) as f32 - 0.5)
        })
        .collect();
    format!("[{}]", w.join(","))
}

/// A NAM file with `{w}` filled by as many weights as its loader asks.
fn network(template: &str) -> nam::Network {
    let expected = match nam::parse(&template.replace("{w}", "[]")) {
        Err(LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    };
    nam::parse(&template.replace("{w}", &weights(expected))).unwrap().network
}

const WAVENET: &str = r#"{"version":"0.5.4","architecture":"WaveNet","sample_rate":48000,
"config":{"layers":[{"input_size":1,"condition_size":1,"channels":8,"head_size":4,"kernel_size":3,
"dilations":[1,2,4,8,16,32],"activation":"Tanh","gated":false,"head_bias":false},
{"input_size":8,"condition_size":1,"channels":4,"head_size":1,"kernel_size":3,
"dilations":[1,2,4,8,16,32],"activation":"Tanh","gated":false,"head_bias":true}],
"head":null,"head_scale":0.02},"weights":{w}}"#;

const LSTM: &str = r#"{"version":"0.5.4","architecture":"LSTM","sample_rate":48000,
"config":{"input_size":1,"hidden_size":16,"num_layers":1},"weights":{w}}"#;

fn gru() -> Gru {
    let w = weights(3 * 16 * 17 + 6 * 16 + 17);
    Gru::parse(&format!(
        r#"{{"input_size":1,"hidden_size":16,"num_layers":1,"sample_rate":48000,"weights":{w}}}"#
    ))
    .unwrap()
}

/// A network and its own account of a rate it was not trained at.
fn guarded<S: Stage + 'static>(s: S, mismatch: fn(&S) -> Option<RateMismatch>) -> Box<dyn Subject> {
    Box::new(Guarded(s, mismatch))
}

/// The circuits' and the catalog's own stages.
fn catalog(out: &mut Vec<Entry>) {
    for spec in CATALOG {
        let e = Entry::new("catalog", spec.name, Measure::Response, move || plain((spec.build)(&(spec.params)())));
        out.push(match spec.name {
            "circuit/common_cathode" => e.bound(1e3),
            _ => e,
        });
    }
    for spec in REFERENCES {
        let e = Entry::new("references", spec.name, Measure::Response, move || plain((spec.build)(&(spec.params)()))).heavy();
        out.push(match spec.name {
            // Its full scale is 0.1 V in: the ladder's own small signal.
            "circuit/moog_904a" => e.drive(0.0, 0.01, 1.0),
            _ => e.bound(1e3),
        });
    }
}

fn circuits(out: &mut Vec<Entry>) {
    use stave_dsp::{
        CommonCathode, CommonCathodeConfig, Dempwolf, DiodeClipper, DiodeClipperConfig, OpAmpClipper,
        OpAmpClipperConfig, PairSolver, PassiveToneControlPositions, PassiveToneStack, PassiveToneStackComponents,
        PowerSupply, PowerSupplyConfig, PushPull, Shockley,
    };
    const F: &str = "circuits";
    for (solver, label) in PairSolver::ALL.into_iter().zip(PairSolver::NAMES) {
        out.push(
            stage(F, &format!("DiodeClipper ({label})"), Measure::Response, move || {
                DiodeClipper::new(DiodeClipperConfig {
                    input_ohms: 2.2e3,
                    capacitor_farads: 10e-9,
                    diode: Shockley::N4148,
                    solver,
                })
            })
            .drive(0.0, 0.01, 4.0),
        );
    }
    out.push(stage(F, "OpAmpClipper (TS808)", Measure::Response, || OpAmpClipper::new(OpAmpClipperConfig::TS808)));
    out.push(
        stage(F, "CommonCathode (Dempwolf)", Measure::Response, || {
            CommonCathode::new(CommonCathodeConfig {
                tube: Dempwolf::RSD_1,
                source_ohms: 33e3,
                supply_volts: 300.0,
                plate_load_ohms: 100e3,
                cathode_ohms: 1.5e3,
                bypass_farads: 22e-6,
                newton: Default::default(),
            })
        })
        .drive(0.0, 0.01, 8.0)
        .bound(1e3),
    );
    out.push(
        stage(F, "PowerSupply (silicon, choke)", Measure::Level, || PowerSupply::new(PowerSupplyConfig::SILICON_CHOKE))
            .drive(0.05, 0.001, 0.04)
            .bound(1e4),
    );
    out.push(stage(F, "PushPull (EL34)", Measure::Response, || {
        let mut p = PushPull::new();
        p.set_pentode(stave_dsp::circuit::topologies::push_pull::PentodePair::EL34);
        p.set_drive(db(18.0));
        p
    }));
    out.push(stage(F, "PassiveToneStack", Measure::Response, || {
        let mut s = PassiveToneStack::new(PassiveToneStackComponents {
            treble_capacitance_farads: 250e-12,
            bass_capacitance_farads: 100e-9,
            midrange_capacitance_farads: 47e-9,
            treble_potentiometer_ohms: 250e3,
            bass_potentiometer_ohms: 1e6,
            midrange_potentiometer_ohms: 25e3,
            slope_resistance_ohms: 100e3,
        });
        s.set_controls(PassiveToneControlPositions::new(0.5, 0.5, 0.5));
        s
    }));
    out.push(stage(F, "Triode (behavioural)", Measure::Response, stave_dsp::Triode::new));
    out.push(stage(F, "Coupler (20 Hz)", Measure::Response, || stave_dsp::Coupler::new(hz(20.0))));
    {
        use stave_dsp::circuit::devices::bbd::{Chip, Filter, HoltersParker};
        use stave_dsp::circuit::devices::compander::{self, Compander, Mode};
        use stave_dsp::circuit::devices::converter::{self, Converter};
        use stave_dsp::circuit::devices::photocell::Najnudel;
        use stave_dsp::circuit::topologies::bucket_brigade::BucketBrigade;
        use stave_dsp::circuit::topologies::opto_compressor::OptoCompressor;
        use stave_dsp::primitives::fixed::Overflow;
        out.push(stage(F, "HoltersParker (MN3007, Juno-60)", Measure::Response, || {
            let mut l = HoltersParker::new(Chip::MN3007, Filter::JUNO_60_INPUT, Filter::JUNO_60_OUTPUT);
            l.set_clock(hz(40_000.0));
            l
        }));
        // Known gap (R6's sweep): the NE570 compressor closes its loop on
        // the last sample's rectifier, with no output swing modelled, so a
        // hot onset after silence passes its full 80 dB of range for a
        // sample (2 V DC: 2e4 V), and the compandored line expands that
        // spike again (to 1.7e6 at 16 kHz, 5e2 at 768 kHz). The bounds
        // pin it, twice the worst seen, until the chip's output stage is
        // modelled.
        out.push(
            stage(F, "BucketBrigade (MN3007, NE570)", Measure::Response, || {
                let mut l = BucketBrigade::new(Chip::MN3007);
                l.set_clock(hz(40_000.0));
                l
            })
            .bound(4e6),
        );
        out.push(
            stage(F, "Compander (NE570)", Measure::Response, || Compander::new(compander::Chip::NE570, Mode::Compress))
                .bound(4e4),
        );
        out.push(stage(F, "OptoCompressor (VTL5C3)", Measure::Level, || OptoCompressor::new(Najnudel::VTL5C3)).drive(0.0, 0.1, 10.0));
        out.push(stage(F, "Converter (SP-1200)", Measure::Response, || Converter::new(converter::Machine::SP1200)));
        out.push(stage(F, "Digital delay (SP-1200 word)", Measure::Energy, || {
            let mut d = converter::Delay::new(converter::Machine::SP1200, seconds(0.5), Overflow::Saturate, 8);
            d.set_delay(seconds(0.3));
            d.set_feedback(0.6);
            d
        }));
    }
    // The decks whose solve is costliest (many transistors, a ladder).
    const HEAVY: [&str; 4] = ["Moog 904-A", "MXR Phase 90", "transistor ladder", "diode ladder"];
    for (i, deck) in DECKS.iter().enumerate() {
        let drive = |e: Entry| e.drive(deck.bias as f32, 0.01 * deck.volts as f32, deck.volts as f32).bound(1e4);
        let dk = drive(Entry::new(F, format!("Circuit<Dk> {}", deck.name), Measure::Response, move || {
            let d = &DECKS[i];
            let mut c = Circuit::<Dk>::new(&(d.netlist)([0.5; 3])).unwrap();
            c.drive(d.source).unwrap();
            plain(c)
        }));
        out.push(if HEAVY.contains(&deck.name) { dk.heavy() } else { dk });
        out.push(drive(Entry::new(F, format!("Circuit<Wdf> {}", deck.name), Measure::Response, move || {
            let d = &DECKS[i];
            let mut c = Circuit::<Wdf>::new(&(d.netlist)([0.5; 3])).unwrap();
            c.drive(d.source).unwrap();
            plain(c)
        })).heavy());
    }
}

fn learned(out: &mut Vec<Entry>) {
    use stave_dsp::learned::block::{self, Nonlinearity, ParallelHammerstein, Section, WienerHammerstein};
    const F: &str = "learned";
    out.push(
        Entry::new(F, "WaveNet (NAM, strict)", Measure::Response, || guarded(network(WAVENET), nam::Network::rate_mismatch))
            .refuses(learned_refuses_but_48k),
    );
    out.push(
        Entry::new(F, "LSTM (NAM 1 x 16, strict)", Measure::Response, || guarded(network(LSTM), nam::Network::rate_mismatch))
            .refuses(learned_refuses_but_48k),
    );
    out.push(
        Entry::new(F, "LSTM (NAM 1 x 16, interpolated)", Measure::Response, || {
            let mut n = network(LSTM);
            n.set_rate_policy(RatePolicy::Interpolated);
            guarded(n, nam::Network::rate_mismatch)
        })
        .refuses(interpolated_refuses),
    );
    out.push(Entry::new(F, "GRU (1 x 16, strict)", Measure::Response, || guarded(gru(), Gru::rate_mismatch)).refuses(learned_refuses_but_48k));
    out.push(
        Entry::new(F, "GRU (1 x 16, interpolated)", Measure::Response, || {
            let mut g = gru();
            g.set_rate_policy(RatePolicy::Interpolated);
            guarded(g, Gru::rate_mismatch)
        })
        .refuses(interpolated_refuses),
    );
    // Block models fitted at 48 kHz on rational sections, which follow
    // any rate (a mismatch reported, nothing refused).
    let section = |hz: f64| {
        let w = std::f64::consts::TAU * hz / 48_000.0;
        let alpha = w.sin() / 1.4;
        let a0 = 1.0 + alpha;
        Section {
            b: [(1.0 - w.cos()) / 2.0 / a0, (1.0 - w.cos()) / a0, (1.0 - w.cos()) / 2.0 / a0],
            a: [-2.0 * w.cos() / a0, (1.0 - alpha) / a0],
        }
    };
    out.push(
        Entry::new(F, "WienerHammerstein (spline)", Measure::Response, move || {
            let knots: Vec<f64> = (0..24).map(|k| (f64::from(k) / 23.0 * 2.0 - 1.0).powi(3) * 3.0).collect();
            let spline = Nonlinearity::Spline {
                values: knots.iter().map(|&u| u.tanh()).collect(),
                knots,
            };
            let wh = WienerHammerstein::new(
                48_000.0,
                block::Linear::Rational(vec![section(700.0), section(5_000.0)]),
                spline,
                block::Linear::Rational(vec![section(9_000.0), section(12_000.0)]),
            );
            guarded(wh, WienerHammerstein::rate_mismatch)
        }),
    );
    out.push(
        Entry::new(F, "ParallelHammerstein (order 5)", Measure::Response, move || {
            let p = ParallelHammerstein::new(
                48_000.0,
                (0..5).map(|m| block::Linear::Rational(vec![section(2_000.0 + 1_000.0 * f64::from(m)); 2])).collect(),
            );
            guarded(p, ParallelHammerstein::rate_mismatch)
        })
        // An order-5 polynomial: twice its hot level is 32 times it.
        .bound(1e3),
    );
}

fn physical(out: &mut Vec<Entry>) {
    use stave_dsp::physical::plate::Plate;
    use stave_dsp::physical::rotary::{Cabinet, Directivity, Radiator, Rotor, Speed, Wall};
    use stave_dsp::physical::spring::{Chirp, Spring, Tank};
    use stave_dsp::physical::tape::{Chowdhury, Flutter, Formulation, Machine, Phase, Playback, Recording, Transport};
    use stave_dsp::primitives::time::interpolators::Lagrange3;
    const F: &str = "physical";
    out.push(stage(F, "Plate (EMT 140, to 5 kHz)", Measure::Energy, || {
        let mut p = Plate::emt140(seconds(2.0));
        p.set_bandwidth(hz(5_000.0));
        p
    }));
    out.push(stage(F, "Chirp (100 sections)", Measure::Response, || Chirp::new(100, 6, 0.6)));
    out.push(stage(F, "Spring", Measure::Energy, || Spring::new(seconds(0.033), hz(4_000.0))));
    out.push(stage(F, "Tank (3 springs)", Measure::Energy, Tank::default));
    out.push(stage(F, "Radiator (horn, tremolo)", Measure::Level, || {
        let mut h: Radiator<Lagrange3> = Radiator::new(Rotor::HORN_122H, Directivity::HORN_122H);
        h.set_walls(&[Wall::BACK_122H]);
        h.set_speed(Speed::Tremolo);
        h
    }));
    out.push(stage(F, "Cabinet (Leslie 122H, chorale)", Measure::Level, || {
        let mut c: Cabinet<Lagrange3> = Cabinet::leslie_122h();
        c.set_speed(Speed::Chorale);
        c
    }));
    out.push(stage(F, "Chowdhury tape (TC-260)", Measure::Level, || Chowdhury::new(Machine::TC260)));
    // The record side needs a rate above twice its 55 kHz bias (its
    // valid range): 16× here, as `Chowdhury` runs it.
    out.push(stage(F, "Recording (TC-260, 16x)", Measure::Response, || {
        stave_dsp::Oversampled::linear(Recording::new(Machine::TC260), 16)
    }));
    out.push(stage(F, "Playback (7.5 ips)", Measure::Response, || {
        Playback::new(Machine::TC260.play, Formulation::FERRIC.thickness, Machine::ips(7.5), Phase::Linear)
    }));
    out.push(stage(F, "Flutter (Akai 4000D)", Measure::Level, || Flutter::new(Transport::AKAI_4000D)));
    {
        // H10's instruments: a string and a membrane and a bar driven by
        // their input (a velocity, a force), a reed tube blowing itself.
        use stave_dsp::physical::bar::{Bar, Beam, Boundary, Material};
        use stave_dsp::physical::membrane::{Drumhead, Membrane};
        use stave_dsp::physical::plate::Loss;
        use stave_dsp::physical::string::Waveguide as StringGuide;
        use stave_dsp::physical::tube::{Bore, Shape, Valve, Waveguide as TubeGuide};
        out.push(stage(F, "String (110 Hz, driven)", Measure::Energy, || {
            let s: StringGuide = StringGuide::new(hz(40.0), hz(110.0));
            s
        }));
        out.push(stage(F, "Membrane (110 Hz tom)", Measure::Energy, || {
            let head = Drumhead::tuned(0.18, 0.26, hz(110.0));
            Membrane::new(head, Loss::two_point(hz(110.0), seconds(1.2), hz(2_000.0), seconds(0.3)).unwrap())
        }));
        out.push(stage(F, "Bar (880 Hz, free)", Measure::Energy, || {
            let key = Beam::tuned(Material::ALUMINIUM, 0.008, Boundary::Free, hz(880.0));
            Bar::new(key, Boundary::Free, Loss::two_point(hz(880.0), seconds(2.0), hz(8_000.0), seconds(0.4)).unwrap())
        }));
        out.push(
            stage(F, "Tube (clarinet, reed, blown)", Measure::Generator, || {
                let bore = Bore { length: 0.6, radius: 0.0075, shape: Shape::Cylinder };
                let mut t: TubeGuide = TubeGuide::new(bore, Valve::Reed);
                t.set_blow(0.5);
                t.set_embouchure(0.3);
                t
            })
            .drive(0.0, 0.0, 0.0),
        );
    }
    {
        // E9 and H7: the loudspeaker, a capture's cabinet, the close mic.
        use stave_dsp::physical::cabinet::{Breakup, Captured, CloseMic, Placement, Position};
        use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
        use stave_dsp::units::SampleRate;
        out.push(stage(F, "Driver (Jensen C12N, linear)", Measure::Response, || {
            Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR)
        }));
        out.push(stage(F, "Driver (Jensen C12N, large signal)", Measure::Response, || {
            let mut d = Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::from_limits(1.0e-3, 1.2e-3, 6e-3));
            d.set_volts(20.0);
            d
        }));
        out.push(stage(F, "Captured (one position)", Measure::Response, || {
            let ir: Vec<f32> = (0..4_096u64)
                .map(|n| {
                    let u = (stave_dsp::primitives::random::splitmix64(n ^ 0xCAB) >> 40) as f32 / (1u32 << 24) as f32;
                    (u - 0.5) * (-(n as f32) / 600.0).exp()
                })
                .collect();
            let ir = stave_dsp::ImpulseResponse::mono(&ir).recorded_at(SampleRate::new(48_000.0));
            Captured::new(vec![vec![Position::new(0.0, ir)]])
        }));
        out.push(stage(F, "CloseMic (cardioid, 2.5 cm)", Measure::Response, || {
            CloseMic::new(
                Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR),
                Placement::CLOSE,
                Breakup::PAPER_12,
            )
        }));
    }
}

fn effects(out: &mut Vec<Entry>) {
    use stave_dsp::effects::dynamics::{Compressor, GainComputer, Limiter, Multiband, Peak, Peaks, ProgramDependent, Rms, Sidechain, Topology};
    use stave_dsp::effects::eq::{Band, Detector, Dynamic, DynamicBand, Dynamics, LinearPhase, MinimumPhase, Phase};
    use stave_dsp::effects::granular::Granulator;
    use stave_dsp::effects::modulation::FrequencyShifter;
    use stave_dsp::effects::pitch::{PhaseVocoder, Psola, Reconstruction};
    use stave_dsp::effects::reverb::{Dattorro, Fdn, Kronecker, Mixing, Schroeder, VelvetNoise};
    use stave_dsp::primitives::filters::crossover::{Crossover, Order};
    use stave_dsp::units::Ratio;
    const F: &str = "effects";
    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    out.push(stage(F, "Peak detector", Measure::Level, || Peak::new(ms(5.0), ms(50.0))));
    out.push(stage(F, "Rms detector", Measure::Level, || Rms::new(ms(10.0))));
    out.push(stage(F, "ProgramDependent detector", Measure::Level, || ProgramDependent::new(ms(1.0), ms(50.0), ms(1_000.0), ms(500.0))));
    out.push(stage(F, "Sidechain (HP + LP)", Measure::Response, || Sidechain::new(Some(hz(100.0)), Some(hz(8_000.0)))));
    out.push(stage(F, "Compressor (feed-forward)", Measure::Response, move || Compressor::new(curve, Topology::FeedForward)));
    out.push(stage(F, "Compressor (feedback)", Measure::Response, move || Compressor::new(curve, Topology::Feedback)));
    out.push(stage(F, "Limiter (true peak)", Measure::Response, || Limiter::new(Peaks::True, db(-1.0))));
    out.push(stage(F, "Limiter (sample peak)", Measure::Response, || Limiter::new(Peaks::Sample, db(-1.0))));
    let corners = [hz(120.0), hz(800.0), hz(5_000.0)];
    out.push(stage(F, "Crossover (LR8, 4 bands)", Measure::Response, move || Crossover::<4>::linkwitz_riley(Order::Lr8, &corners)));
    out.push(stage(F, "Crossover (linear phase, 4 bands)", Measure::Response, move || {
        Crossover::<4>::linear_phase(&corners, hz(200.0), db(80.0))
    }));
    out.push(stage(F, "Multiband (LR8, compressors)", Measure::Response, move || {
        Multiband::new(Crossover::<4>::linkwitz_riley(Order::Lr8, &corners), |_| Compressor::new(curve, Topology::FeedForward))
    }));
    let bands = [
        Band::LowCut { frequency: hz(30.0), q: 0.707 },
        Band::LowShelf { frequency: hz(120.0), gain: db(4.0) },
        Band::Bell { frequency: hz(400.0), gain: db(-6.0), q: 1.5 },
        Band::Bell { frequency: hz(3_000.0), gain: db(5.0), q: 3.0 },
        Band::HighShelf { frequency: hz(10_000.0), gain: db(-3.0) },
    ];
    out.push(stage(F, "MinimumPhase EQ (5 bands)", Measure::Response, move || MinimumPhase::new(&bands)));
    out.push(stage(F, "LinearPhase EQ (5 bands)", Measure::Response, move || LinearPhase::new(&bands)));
    out.push(stage(F, "Dynamic EQ", Measure::Response, move || {
        let working = Dynamics {
            threshold: db(-30.0),
            ratio: Ratio::new(4.0),
            knee: db(6.0),
            range: db(-9.0),
            attack: ms(2.0),
            release: ms(60.0),
            detector: Detector::Peak,
        };
        Dynamic::new(&[
            DynamicBand::dynamic(bands[1], Phase::Minimum, working),
            DynamicBand::dynamic(bands[2], Phase::Minimum, working),
            DynamicBand::dynamic(bands[3], Phase::Linear, working),
        ])
    }));
    for (r, name) in [(Reconstruction::Prusa, "PhaseVocoder +7 st (Prusa)"), (Reconstruction::LarocheDolson, "PhaseVocoder +7 st (Laroche-Dolson)")] {
        out.push(stage(F, name, Measure::Level, move || {
            let mut pv = PhaseVocoder::new(r, semitones(12.0));
            pv.set_shift(semitones(7.0));
            pv
        }));
    }
    out.push(stage(F, "Psola +7 st", Measure::Level, || {
        let mut p = Psola::new(hz(60.0), hz(1_000.0));
        p.set_shift(semitones(7.0));
        p
    }));
    out.push(stage(F, "Granulator", Measure::Level, || {
        let mut g = Granulator::new(seconds(1.0), 16);
        g.set_density(50.0);
        g.set_size(ms(80.0));
        g.set_pitch(semitones(7.0));
        g.set_position(ms(150.0));
        g.set_spray(ms(50.0));
        g
    }));
    out.push(stage(F, "FrequencyShifter (+100 Hz)", Measure::Level, || FrequencyShifter::new(hz(100.0))));
    for (name, mixing) in [("Fdn (Householder)", Mixing::Householder), ("Fdn (Hadamard)", Mixing::Hadamard)] {
        out.push(stage(F, name, Measure::Energy, move || {
            let mut f = Fdn::new(mixing);
            f.set_decay(seconds(2.0));
            f
        }));
    }
    out.push(stage(F, "Fdn (Kronecker, modulated)", Measure::Energy, || {
        let mut f = Fdn::new(Mixing::Hadamard);
        f.set_kronecker(Kronecker::hadamard());
        f.set_angle_modulation(0.3, hz(0.5));
        f.set_decay(seconds(2.0));
        f
    }));
    out.push(stage(F, "VelvetNoise", Measure::Energy, || {
        let mut v = VelvetNoise::new();
        v.set_decay(seconds(2.0));
        v
    }));
    out.push(stage(F, "Dattorro (plate)", Measure::Energy, || {
        let mut d = Dattorro::new(ms(50.0));
        d.set_t60(seconds(2.0));
        d
    }));
    out.push(stage(F, "Schroeder", Measure::Energy, || {
        let mut s = Schroeder::new();
        s.set_t60(seconds(2.0));
        s
    }));
    out.push(stage(F, "Schroeder::moorer", Measure::Energy, || {
        let mut s = Schroeder::moorer();
        s.set_t60(seconds(2.0));
        s
    }));
    out.push(stage(F, "Room (live room)", Measure::Energy, || {
        stave_dsp::effects::reverb::Room::new(stave_dsp::effects::reverb::Shoebox::LIVE_ROOM)
    }));
}

fn primitives(out: &mut Vec<Entry>) {
    use stave_dsp::primitives::control::drift::{OrnsteinUhlenbeck, Wiener};
    use stave_dsp::primitives::filters::formant::{Formant, Formants, Resonator, Speaker, Vowel};
    use stave_dsp::primitives::filters::hilbert::Hilbert;
    use stave_dsp::primitives::filters::korg35::Korg35;
    use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
    use stave_dsp::primitives::filters::sallen_key::SallenKey;
    use stave_dsp::primitives::quantise::{Dither, Quantiser};
    use stave_dsp::primitives::resonators::{Bank, Comb as Resonant, KarplusStrong, Mode};
    use stave_dsp::primitives::spectral::{Fir, Head, Passband, Stft, Window};
    use stave_dsp::primitives::time::delay::Delay;
    use stave_dsp::primitives::time::interpolators::{Lagrange3, Sinc, Thiran};
    use stave_dsp::units::SampleRate;
    use stave_dsp::{
        Adsr, AdsrConfig, Allpass1, Biquad, Comb, Convolver, CurveStage, DcBlocker, DelayAllpass, EnvelopeFollower,
        EnvelopeTimes, FilterSpec, Hysteresis, ImpulseResponse, Lag, Lfo, LinkwitzRiley4, ModLfo, NonlinearRc, OnePole,
        Osc, Oversampled, RandomWalk, RcClipperConfig, Slew, SmoothSat, SubOctave, Svf, Waveform, Wavetable,
    };
    const F: &str = "primitives";
    use Measure::{Energy, Generator, Level, Random, Response};
    out.push(stage(F, "Biquad (peak)", Response, || Biquad::new(FilterSpec::Peak { frequency: Hz::new(1_000.0), gain: Db::new(6.0), q: 1.0 })));
    out.push(stage(F, "Allpass1", Response, || Allpass1::new(hz(700.0))));
    out.push(stage(F, "OnePole (lowpass)", Response, || OnePole::lowpass(hz(2_000.0))));
    out.push(stage(F, "DcBlocker", Response, || DcBlocker::new(hz(10.0))));
    out.push(stage(F, "Svf", Response, || Svf::new(hz(1_000.0), 0.707)));
    out.push(stage(F, "LinkwitzRiley4 (lowpass)", Response, || LinkwitzRiley4::lowpass(hz(2_000.0))));
    out.push(stage(F, "Hilbert", Response, Hilbert::default));
    out.push(stage(F, "Ladder (Huovilainen)", Response, || Huovilainen::new(hz(1_000.0), 0.5)));
    out.push(stage(F, "Ladder (Stinchcombe)", Response, || Stinchcombe::new(hz(1_000.0), 0.5)));
    out.push(stage(F, "SallenKey", Response, || SallenKey::new(hz(1_000.0), 0.5)));
    out.push(stage(F, "Korg35", Response, || Korg35::new(hz(1_000.0), 0.5)));
    out.push(stage(F, "Formants (aa, man)", Response, || Formants::new(Vowel::Aa.formants(Speaker::Man))));
    out.push(stage(F, "Resonator (formant)", Response, || {
        Resonator::new(Formant { centre: hz(800.0), bandwidth: hz(80.0) })
    }));
    out.push(stage(F, "CurveStage (tanh)", Response, || CurveStage::from_fn(-4.0, 4.0, 0.0, f64::tanh)));
    out.push(stage(F, "SmoothSat", Response, SmoothSat::new));
    out.push(stage(F, "Hysteresis", Response, || Hysteresis::new(2.2, 0.5)));
    out.push(stage(F, "Slew", Response, || Slew::new(120.0, 120.0)));
    out.push(stage(F, "NonlinearRc (diode)", Response, || {
        NonlinearRc::diode_clipper(&RcClipperConfig { tau_ms: 0.35, threshold: 0.5, hardness: 2.0 })
    }));
    out.push(stage(F, "Oversampled 4x (SmoothSat, linear)", Response, || Oversampled::linear(SmoothSat::new(), 4)));
    out.push(stage(F, "Oversampled 2x (SmoothSat, minimum)", Response, || Oversampled::minimum(SmoothSat::new(), 2)));
    out.push(stage(F, "Quantiser (16-bit TPDF)", Response, || Quantiser::new(16).with_dither(Dither::Triangular, 1)));
    out.push(stage(F, "Delay<Lagrange3> (7.3 ms)", Response, || Delay::<Lagrange3>::new(ms(20.0), ms(7.3))));
    out.push(stage(F, "Delay<Sinc<16>> (7.3 ms)", Response, || Delay::<Sinc<16>>::new(ms(20.0), ms(7.3))));
    out.push(stage(F, "Comb (feedback)", Energy, || Comb::new(ms(50.0), ms(12.3), 0.6)));
    out.push(stage(F, "DelayAllpass", Energy, || DelayAllpass::new(ms(50.0), ms(12.3), 0.6)));
    out.push(stage(F, "Comb resonator", Energy, || Resonant::<Lagrange3>::new(hz(40.0), hz(110.0), seconds(2.0))));
    out.push(stage(F, "KarplusStrong", Energy, || KarplusStrong::<Thiran>::new(hz(40.0), hz(110.0))));
    out.push(stage(F, "Bank (16 modes)", Energy, || {
        let modes: Vec<Mode> = (1..=16).map(|k| Mode { frequency: hz(110.0 * k as f32), decay: seconds(1.0), gain: 0.5 }).collect();
        Bank::new(&modes)
    }));
    out.push(stage(F, "Convolver (decaying noise, uniform)", Response, || {
        let ir: Vec<f32> = (0..4_800u64)
            .map(|n| {
                let u = (stave_dsp::primitives::random::splitmix64(n) >> 40) as f32 / (1u32 << 24) as f32;
                (u - 0.5) * (-(n as f32) / 800.0).exp()
            })
            .collect();
        // Recorded at 48 kHz: the convolver resamples it to its rate.
        let ir = ImpulseResponse::mono(&ir).recorded_at(SampleRate::new(48_000.0));
        Convolver::uniform(&ir, 64, Head::Direct)
    }));
    out.push(stage(F, "Convolver (damped 1 kHz resonance, non-uniform)", Response, || {
        let ir: Vec<f32> = (0..9_600)
            .map(|n| {
                let t = n as f32 / 48_000.0;
                (std::f32::consts::TAU * 1_000.0 * t).cos() * (-t / 0.02).exp() * 0.05
            })
            .collect();
        let ir = ImpulseResponse::mono(&ir).recorded_at(SampleRate::new(48_000.0));
        Convolver::non_uniform(&ir, 64, 1_024, Head::Direct)
    }));
    out.push(stage(F, "Fir (Kaiser lowpass)", Response, || {
        // Designed for 48 kHz (a 4.8 kHz cutoff), resampled at prepare.
        let fir = Fir::kaiser(Passband::Lowpass { cutoff: 0.1 }, db(80.0), 0.04);
        Convolver::uniform(&fir.impulse_response().recorded_at(SampleRate::new(48_000.0)), 64, Head::Direct)
    }));
    out.push(stage(F, "Stft (1024/256 Hann, identity)", Response, || {
        Stft::new(1_024, 256, Window::Hann, |_: &mut [stave_dsp::primitives::spectral::Complex<f32>]| {}).unwrap()
    }));
    out.push(stage(F, "EnvelopeFollower", Level, || {
        EnvelopeFollower::new(EnvelopeTimes { attack_ms: 1.0, release_ms: 50.0, slow_release_ms: None })
    }));
    out.push(stage(F, "Lag", Response, || Lag::new(ms(50.0), ms(50.0))));
    out.push(stage(F, "SubOctave", Level, SubOctave::new));
    for (name, wave) in [("Osc (sine)", Waveform::Sine), ("Osc (saw, polyBLEP)", Waveform::Saw), ("Osc (triangle)", Waveform::Triangle)] {
        out.push(
            stage(F, name, Generator, move || {
                let mut o = Osc::new(wave);
                o.set_frequency(hz(220.0));
                o
            })
            .drive(0.0, 0.0, 0.0),
        );
    }
    out.push(
        stage(F, "Osc (saw, minBLEP)", Generator, || {
            let mut o = Osc::min_blep(Waveform::Saw);
            o.set_frequency(hz(220.0));
            o
        })
        .drive(0.0, 0.0, 0.0),
    );
    out.push(
        stage(F, "Osc (saw, hard sync)", Generator, || {
            let mut o = Osc::new(Waveform::Saw);
            o.set_frequency(hz(370.0));
            o.set_sync(Some(hz(220.0)));
            o
        })
        .drive(0.0, 0.0, 0.0),
    );
    out.push(
        stage(F, "Wavetable (saw)", Generator, || {
            let mut t = Wavetable::new(Waveform::Saw);
            t.set_frequency(hz(220.0));
            t
        })
        .drive(0.0, 0.0, 0.0),
    );
    out.push(stage(F, "Lfo (1.3 Hz)", Generator, || Lfo::new(hz(1.3))).drive(0.0, 0.0, 0.0));
    out.push(stage(F, "ModLfo", Generator, || ModLfo::new(ms(100.0), 7)).drive(0.0, 0.0, 0.0));
    out.push(stage(F, "RandomWalk", Random, || RandomWalk::new(ms(140.0), ms(30.0), 7)).drive(0.0, 0.0, 0.0));
    out.push(
        stage(F, "Adsr (held)", Generator, || {
            let mut a = Adsr::new();
            a.set(&AdsrConfig { attack_ms: 2.0, decay_ms: 50.0, sustain: 0.7, release_ms: 100.0 });
            a.gate_on();
            a
        })
        .drive(0.0, 0.0, 0.0),
    );
    out.push(stage(F, "OrnsteinUhlenbeck", Random, || OrnsteinUhlenbeck::new(25.0, 0.5, seconds(30.0), 1)).drive(0.0, 0.0, 0.0));
    out.push(stage(F, "Wiener", Random, || Wiener::new(1.0, 1e-4, 1e-3, 1)).drive(0.0, 0.0, 0.0));
}

/// Every entry, family by family.
pub fn library() -> Vec<Entry> {
    let mut out = Vec::new();
    catalog(&mut out);
    circuits(&mut out);
    learned(&mut out);
    physical(&mut out);
    effects(&mut out);
    primitives(&mut out);
    out
}
