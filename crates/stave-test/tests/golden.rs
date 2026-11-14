//! Golden digests (DSP.md R9) of every catalogued effects, physical and
//! learned stage that no fingerprint pins (the nonlinear,
//! level-dependent ones are pinned by `fingerprints.rs`), and of the
//! tuned resonators, the multiband frame and the photocell (S7, D4, E7):
//! each run at 48 kHz over the one fixed stimulus
//! (`stave_test::golden::stimulus`) at the settings its datasheet line
//! uses, its output held to the committed time–frequency digest in
//! `crates/stave-test/golden/`. A stage that changes fails here, naming
//! what moved, until it is re-blessed on purpose:
//!
//! ```text
//! STAVE_BLESS=1 cargo test -p stave-test --test golden
//! ```

use stave_dsp::circuit::devices::photocell::{Najnudel, Vactrol};
use stave_dsp::circuit::topologies::opto_compressor::OptoCompressor;
use stave_dsp::effects::dynamics::{
    Compressor, GainComputer, Limiter, Multiband, Peak, Peaks, ProgramDependent, Rms, Sidechain, Topology,
};
use stave_dsp::effects::modulation::{FrequencyShifter, Sideband, SingleSideband};
use stave_dsp::effects::reverb::{Dattorro, Fdn, Kronecker, Mixing, Room, Schroeder, Shoebox, VelvetNoise, T60};
use stave_dsp::learned::nam;
use stave_dsp::physical::cabinet::{Breakup, Captured, CloseMic, Placement, Position};
use stave_dsp::physical::plate::Plate;
use stave_dsp::physical::rotary::{Cabinet, Directivity, Radiator, Rotor, Speed, Wall};
use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
use stave_dsp::physical::spring::{Chirp, Spring, Tank};
use stave_dsp::physical::tape::{
    Capstan, Chowdhury, Condition, Drift, Flutter, Formulation, Machine, Motor, Phase, Playback, Spacing, Splices,
    Transport,
};
use stave_dsp::primitives::filters::crossover::{Crossover, Order};
use stave_dsp::primitives::filters::formant::{Formants, Speaker, Vowel};
use stave_dsp::primitives::resonators::{Bank, Comb, KarplusStrong, Mode};
use stave_dsp::primitives::time::interpolators::Lagrange3;
use stave_dsp::units::{self, db, hz, ms, seconds, semitones};
use stave_dsp::{ImpulseResponse, Osc, Stage, Waveform};
use stave_test::golden::{pin, pin_stage, RATE};

// ---- S3, S3b: reverbs, T60 2 s -------------------------------------------

fn fdn(mixing: Mixing) -> Fdn {
    let mut f = Fdn::new(mixing);
    f.set_decay(seconds(2.0));
    f
}

/// The hall profile the datasheet's per-band lines use.
fn hall() -> T60 {
    T60::octaves([3.0, 2.8, 2.68, 2.55, 2.47, 2.5, 2.3, 1.89, 1.4, 1.2].map(seconds))
}

#[test]
fn fdn_householder() {
    pin_stage("effects/reverb/fdn_householder", "Fdn, 8 lines, Householder, T60 2 s", fdn(Mixing::Householder));
}

#[test]
fn fdn_hadamard() {
    pin_stage("effects/reverb/fdn_hadamard", "Fdn, 8 lines, Hadamard, T60 2 s", fdn(Mixing::Hadamard));
}

#[test]
fn fdn_graphic() {
    let mut f = Fdn::new(Mixing::Hadamard);
    f.set_t60(hall());
    pin_stage("effects/reverb/fdn_graphic", "Fdn, 8 lines, Hadamard, per-octave T60 (hall profile)", f);
}

#[test]
fn fdn_kronecker() {
    let mut f = Fdn::new(Mixing::Hadamard);
    f.set_kronecker(Kronecker::hadamard());
    f.set_angle_modulation(0.3, hz(0.5));
    f.set_decay(seconds(2.0));
    pin_stage("effects/reverb/fdn_kronecker", "Fdn, 8 lines, Kronecker (Hadamard), angles modulated 0.3 at 0.5 Hz, T60 2 s", f);
}

#[test]
fn velvet_noise() {
    let mut v = VelvetNoise::new();
    v.set_decay(seconds(2.0));
    pin_stage("effects/reverb/velvet_noise", "VelvetNoise, 4 branches, T60 2 s", v);
}

#[test]
fn velvet_noise_graphic() {
    let mut v = VelvetNoise::new();
    v.set_t60(hall());
    pin_stage("effects/reverb/velvet_noise_graphic", "VelvetNoise, per-octave T60 (hall profile)", v);
}

#[test]
fn dattorro() {
    let mut d = Dattorro::new(ms(50.0));
    d.set_t60(seconds(2.0));
    pin_stage("effects/reverb/dattorro", "Dattorro plate, 50, T60 2 s", d);
}

#[test]
fn schroeder() {
    let mut s = Schroeder::new();
    s.set_t60(seconds(2.0));
    pin_stage("effects/reverb/schroeder", "Schroeder, 4 combs and 2 allpasses, T60 2 s", s);
}

#[test]
fn moorer() {
    let mut s = Schroeder::moorer();
    s.set_t60(seconds(2.0));
    pin_stage("effects/reverb/moorer", "Schroeder::moorer, 6 combs, T60 2 s", s);
}

#[test]
fn room() {
    pin_stage("effects/reverb/room", "Room, Shoebox::LIVE_ROOM, early and late", Room::new(Shoebox::LIVE_ROOM));
}

// ---- D1–D4: dynamics --------------------------------------------------------

#[test]
fn peak_detector() {
    pin_stage("effects/dynamics/peak", "Peak detector, 5 ms attack, 50 ms release", Peak::new(ms(5.0), ms(50.0)));
}

#[test]
fn rms_detector() {
    pin_stage("effects/dynamics/rms", "Rms detector, 10 ms", Rms::new(ms(10.0)));
}

#[test]
fn program_dependent_detector() {
    pin_stage(
        "effects/dynamics/program_dependent",
        "ProgramDependent detector, 1/50/1000/500 ms",
        ProgramDependent::new(ms(1.0), ms(50.0), ms(1_000.0), ms(500.0)),
    );
}

#[test]
fn sidechain() {
    pin_stage("effects/dynamics/sidechain", "Sidechain, HP 100 Hz, LP 8 kHz", Sidechain::new(Some(hz(100.0)), Some(hz(8_000.0))));
}

const CURVE: GainComputer = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };

/// Driven 12 dB hot (the datasheet's ×4), so both bursts meet the curve.
fn hot(case: &str, model: &str, mut s: impl Stage) {
    s.prepare(RATE);
    pin(case, model, |x| s.tick(4.0 * x));
}

#[test]
fn compressor_feed_forward() {
    hot(
        "effects/dynamics/compressor_feed_forward",
        "Compressor, feed-forward, −20 dB, 4:1, 6 dB knee, input ×4",
        Compressor::new(CURVE, Topology::FeedForward),
    );
}

#[test]
fn compressor_feedback() {
    hot(
        "effects/dynamics/compressor_feedback",
        "Compressor, feedback, −20 dB, 4:1, 6 dB knee, input ×4",
        Compressor::new(CURVE, Topology::Feedback),
    );
}

#[test]
fn limiter_true_peak() {
    hot("effects/dynamics/limiter_true_peak", "Limiter, true peak, −1 dB, input ×4", Limiter::new(Peaks::True, db(-1.0)));
}

#[test]
fn limiter_sample_peak() {
    hot("effects/dynamics/limiter_sample_peak", "Limiter, sample peak, −1 dB, input ×4", Limiter::new(Peaks::Sample, db(-1.0)));
}

const CORNERS: [units::Hz; 3] = [hz(120.0), hz(800.0), hz(5_000.0)];

#[test]
fn multiband() {
    let mb = Multiband::new(Crossover::<4>::linkwitz_riley(Order::Lr4, &CORNERS), |_| {
        Compressor::new(CURVE, Topology::FeedForward)
    });
    hot("effects/dynamics/multiband", "Multiband, 4 bands LR4 at 120/800/5000 Hz, a compressor each, input ×4", mb);
}

#[test]
fn crossover_linear_phase() {
    pin_stage(
        "primitives/crossover_linear_phase",
        "Crossover<4>, linear phase, 120/800/5000 Hz, 200 Hz transitions at 80 dB, bands summed",
        Crossover::<4>::linear_phase(&CORNERS, hz(200.0), db(80.0)),
    );
}

// ---- S5: EQ -----------------------------------------------------------------

fn bands() -> [stave_dsp::effects::eq::Band; 5] {
    use stave_dsp::effects::eq::Band;
    [
        Band::LowCut { frequency: hz(30.0), q: 0.707 },
        Band::LowShelf { frequency: hz(120.0), gain: db(4.0) },
        Band::Bell { frequency: hz(400.0), gain: db(-6.0), q: 1.5 },
        Band::Bell { frequency: hz(3_000.0), gain: db(5.0), q: 3.0 },
        Band::HighShelf { frequency: hz(10_000.0), gain: db(-3.0) },
    ]
}

#[test]
fn eq_minimum_phase() {
    use stave_dsp::effects::eq::MinimumPhase;
    pin_stage("effects/eq/minimum_phase", "MinimumPhase, the datasheet's 5 bands", MinimumPhase::new(&bands()));
}

#[test]
fn eq_linear_phase() {
    use stave_dsp::effects::eq::LinearPhase;
    pin_stage("effects/eq/linear_phase", "LinearPhase, the datasheet's 5 bands", LinearPhase::new(&bands()));
}

fn dynamic_eq(phase: stave_dsp::effects::eq::Phase) -> stave_dsp::effects::eq::Dynamic {
    use stave_dsp::effects::eq::{Detector, Dynamic, DynamicBand, Dynamics, Phase};
    let b = bands();
    let working = Dynamics {
        threshold: db(-30.0),
        ratio: units::Ratio::new(4.0),
        knee: db(6.0),
        range: db(-9.0),
        attack: ms(2.0),
        release: ms(60.0),
        detector: Detector::Peak,
    };
    Dynamic::new(&[
        DynamicBand::dynamic(b[1], Phase::Minimum, working),
        DynamicBand::dynamic(b[2], Phase::Minimum, working),
        DynamicBand::dynamic(b[3], phase, working),
        DynamicBand::dynamic(b[4], Phase::Minimum, working),
    ])
}

#[test]
fn eq_dynamic_minimum() {
    use stave_dsp::effects::eq::Phase;
    pin_stage("effects/eq/dynamic_minimum", "Dynamic EQ, 4 minimum-phase bands, −30 dB, 4:1, range −9 dB", dynamic_eq(Phase::Minimum));
}

#[test]
fn eq_dynamic_mixed() {
    use stave_dsp::effects::eq::Phase;
    pin_stage("effects/eq/dynamic_mixed", "Dynamic EQ, 3 minimum-phase bands and 1 linear-phase, −30 dB, 4:1", dynamic_eq(Phase::Linear));
}

// ---- S4, S6: modulation, pitch, granular -----------------------------------

#[test]
fn frequency_shifter() {
    pin_stage("effects/modulation/frequency_shifter", "FrequencyShifter, +100 Hz", FrequencyShifter::new(hz(100.0)));
}

#[test]
fn single_sideband() {
    let mut ssb = SingleSideband::new(Sideband::Upper);
    ssb.prepare(RATE);
    let mut carrier = Osc::new(Waveform::Sine);
    carrier.set_frequency(hz(440.0));
    carrier.prepare(RATE);
    pin("effects/modulation/single_sideband", "SingleSideband, upper, a 440 Hz sine carrier", |x| {
        ssb.modulate(x, carrier.tick(0.0))
    });
}

fn vocoder(r: stave_dsp::effects::pitch::Reconstruction) -> stave_dsp::effects::pitch::PhaseVocoder {
    let mut pv = stave_dsp::effects::pitch::PhaseVocoder::new(r, semitones(12.0));
    pv.set_shift(semitones(7.0));
    pv
}

#[test]
fn phase_vocoder_prusa() {
    use stave_dsp::effects::pitch::Reconstruction;
    pin_stage("effects/pitch/phase_vocoder_prusa", "PhaseVocoder, Prusa, +7 st", vocoder(Reconstruction::Prusa));
}

#[test]
fn phase_vocoder_laroche_dolson() {
    use stave_dsp::effects::pitch::Reconstruction;
    pin_stage(
        "effects/pitch/phase_vocoder_laroche_dolson",
        "PhaseVocoder, Laroche–Dolson, +7 st",
        vocoder(Reconstruction::LarocheDolson),
    );
}

#[test]
fn psola() {
    let mut p = stave_dsp::effects::pitch::Psola::new(hz(60.0), hz(1_000.0));
    p.set_shift(semitones(7.0));
    pin_stage("effects/pitch/psola", "Psola, 60 Hz–1 kHz, +7 st", p);
}

#[test]
fn granulator() {
    let mut g = stave_dsp::effects::granular::Granulator::new(seconds(1.0), 16);
    g.set_density(50.0);
    g.set_size(ms(80.0));
    g.set_pitch(semitones(7.0));
    g.set_position(ms(150.0));
    g.set_spray(ms(50.0));
    pin_stage("effects/granular/granulator", "Granulator, 16 grains, 50/s, 80 ms, +7 st, 150 ms back, 50 ms spray", g);
}

// ---- H2–H4: springs, plate, rotary ------------------------------------------

#[test]
fn chirp() {
    pin_stage("physical/spring/chirp", "Chirp, 100 sections, k = 6, 0.6", Chirp::new(100, 6, 0.6));
}

#[test]
fn spring() {
    pin_stage("physical/spring/spring", "Spring, 33 ms, 4 kHz", Spring::new(seconds(0.033), hz(4_000.0)));
}

#[test]
fn tank() {
    pin_stage("physical/spring/tank", "Tank, 3 springs (default)", Tank::default());
}

#[test]
fn plate() {
    let mut p = Plate::emt140(seconds(3.0));
    p.set_bandwidth(hz(5_000.0));
    pin_stage("physical/plate/emt140", "Plate, EMT 140, 3 s, modes to 5 kHz, mono tick", p);
}

/// Both channels of a stereo stage, as `<case>/left` and `<case>/right`.
fn stereo(case: &str, model: &str, mut tick: impl FnMut(f32) -> (f32, f32)) {
    let mut right = Vec::new();
    pin(&format!("{case}/left"), &format!("{model}, left"), |x| {
        let (l, r) = tick(x);
        right.push(r);
        l
    });
    let mut it = right.into_iter();
    pin(&format!("{case}/right"), &format!("{model}, right"), |_| it.next().unwrap());
}

#[test]
fn radiator() {
    let mut horn: Radiator = Radiator::new(Rotor::HORN_122H, Directivity::HORN_122H);
    horn.set_walls(&[Wall::BACK_122H]);
    horn.set_speed(Speed::Tremolo);
    horn.prepare(RATE);
    stereo("physical/rotary/radiator", "Radiator, 122H horn, back wall, tremolo", |x| horn.tick_stereo(x));
}

#[test]
fn rotary_cabinet() {
    let mut leslie: Cabinet = Cabinet::leslie_122h();
    leslie.set_speed(Speed::Tremolo);
    leslie.prepare(RATE);
    stereo("physical/rotary/cabinet", "Cabinet, Leslie 122H, tremolo", |x| leslie.tick_stereo(x));
}

// ---- E9, H7: speaker, cabinet, room ------------------------------------------

#[test]
fn driver_linear() {
    let mut d = Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR);
    d.set_volts(20.0);
    pin_stage("physical/speaker/driver_linear", "Driver, Jensen C12N (LR-2 coil), linear, 20 V a unit", d);
}

/// Decaying noise of 8192 taps, signs from `seed` (the datasheet's
/// synthetic capture).
fn capture(seed: u32) -> ImpulseResponse {
    let mut lcg: u32 = 7;
    let mut ir: Vec<f32> = (0..8_192)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.3
        })
        .collect();
    let mut s = seed;
    for (n, v) in ir.iter_mut().enumerate() {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *v *= (-(n as f32) / 600.0).exp() * if s & 1 == 0 { 1.0 } else { -1.0 };
    }
    ImpulseResponse::mono(&ir)
}

#[test]
fn captured() {
    let mut cab = Captured::new(vec![
        vec![Position::new(0.0, capture(1)), Position::new(1.0, capture(2))],
        vec![Position::new(0.0, capture(3))],
    ]);
    cab.set_placement(0, 0.5);
    pin_stage("physical/cabinet/captured", "Captured, 2 mics of 8192-tap synthetic captures, the first halfway", cab);
}

#[test]
fn close_mic() {
    let mic = CloseMic::new(
        Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR),
        Placement::CLOSE,
        Breakup::PAPER_12,
    );
    pin_stage("physical/cabinet/close_mic", "CloseMic, C12N linear, close, paper 12\" breakup", mic);
}

// ---- H10: instruments ----------------------------------------------------------

#[test]
fn string_plucked() {
    use stave_dsp::physical::string;
    let mut s: string::Waveguide = string::Waveguide::new(hz(40.0), hz(110.0));
    s.prepare(RATE);
    s.pluck(0.5, seconds(0.001));
    pin("physical/string/plucked", "String, 110 Hz, Lagrange3, plucked at 0.5, input ×0.01", |x| s.tick(0.01 * x));
}

#[test]
fn string_stiff() {
    use stave_dsp::physical::string;
    let mut s: string::Waveguide = string::Waveguide::new(hz(40.0), hz(110.0));
    s.set_inharmonicity(4e-4);
    s.prepare(RATE);
    pin("physical/string/stiff", "String, 110 Hz, inharmonicity 4e-4 (8 sections), input ×0.01", |x| s.tick(0.01 * x));
}

#[test]
fn string_bowed() {
    use stave_dsp::physical::string::{self, Bow};
    let mut s: string::Waveguide = string::Waveguide::new(hz(40.0), hz(196.0));
    s.set_position(0.1);
    s.prepare(RATE);
    s.set_bow(Some(Bow { force: 0.5, velocity: 0.2 }));
    pin("physical/string/bowed", "String, 196 Hz, bowed at 0.1, force 0.5, velocity 0.2, no input", |_| s.tick(0.0));
}

fn blown(case: &str, model: &str, bore: stave_dsp::physical::tube::Bore, valve: stave_dsp::physical::tube::Valve) {
    use stave_dsp::physical::tube::{self, Valve};
    let mut t: tube::Waveguide = tube::Waveguide::new(bore, valve);
    t.set_embouchure(if matches!(valve, Valve::Reed) { 0.3 } else { 0.1 });
    t.set_blow(0.6);
    t.prepare(RATE);
    pin(case, model, |_| t.tick(0.0));
}

#[test]
fn tube_clarinet() {
    use stave_dsp::physical::tube::{Bore, Shape, Valve};
    blown(
        "physical/tube/clarinet",
        "Tube, cylinder 0.6 m × 7.5 mm, reed, embouchure 0.3, blow 0.6",
        Bore { length: 0.6, radius: 0.0075, shape: Shape::Cylinder },
        Valve::Reed,
    );
}

#[test]
fn tube_cone() {
    use stave_dsp::physical::tube::{Bore, Shape, Valve};
    blown(
        "physical/tube/cone",
        "Tube, cone 0.6 m × 20 mm, apex 0.05, reed, embouchure 0.3, blow 0.6",
        Bore { length: 0.6, radius: 0.02, shape: Shape::Cone { apex: 0.05 } },
        Valve::Reed,
    );
}

#[test]
fn tube_brass() {
    use stave_dsp::physical::tube::{Bore, Shape, Valve};
    blown(
        "physical/tube/brass",
        "Tube, cylinder 1.4 m × 6 mm, lips 275 Hz Q 20, embouchure 0.1, blow 0.6",
        Bore { length: 1.4, radius: 0.006, shape: Shape::Cylinder },
        Valve::Lip { frequency: hz(275.0), q: 20.0 },
    );
}

fn loss() -> stave_dsp::physical::membrane::Loss {
    stave_dsp::physical::membrane::Loss::two_point(hz(100.0), seconds(1.5), hz(2_000.0), seconds(0.4)).unwrap()
}

#[test]
fn membrane() {
    use stave_dsp::physical::membrane::{Drumhead, Membrane};
    let tom = Membrane::new(Drumhead::tuned(0.18, 0.26, hz(100.0)), loss());
    pin_stage("physical/membrane/tom", "Membrane, 0.18 m, 0.26, tuned 100 Hz, loss 1.5 s at 100 Hz / 0.4 s at 2 kHz", tom);
}

#[test]
fn bar() {
    use stave_dsp::physical::bar::{Bar, Beam, Boundary, Material};
    let key = Beam::tuned(Material::ALUMINIUM, 0.01, Boundary::Free, hz(100.0));
    pin_stage("physical/bar/free", "Bar, aluminium 10 mm, free, tuned 100 Hz, the membrane's loss", Bar::new(key, Boundary::Free, loss()));
}

// ---- E8, H5: tape --------------------------------------------------------------

#[test]
fn tape_machine() {
    pin_stage("physical/tape/chowdhury", "Chowdhury, TC-260 at 7.5 ips, its flutter", Chowdhury::new(Machine::TC260));
}

#[test]
fn tape_playback() {
    pin_stage(
        "physical/tape/playback",
        "Playback, TC-260 head, ferric, 7.5 ips, linear phase",
        Playback::new(Machine::TC260.play, Formulation::FERRIC.thickness, Machine::ips(7.5), Phase::Linear),
    );
}

#[test]
fn tape_flutter() {
    pin_stage("physical/tape/flutter", "Flutter, Akai 4000D", Flutter::new(Transport::AKAI_4000D));
}

#[test]
fn tape_capstan() {
    let c = Capstan::new(1).with_drift(Drift::STEADY).with_motor(Motor::STUDIO).with_splices(Splices::EDITED);
    pin_stage("physical/tape/capstan", "Capstan, seed 1, steady drift, studio motor, edited splices", c);
}

#[test]
fn tape_spacing_used() {
    pin_stage("physical/tape/spacing_used", "Spacing, used tape, seed 1", Spacing::new(Condition::USED, 1));
}

#[test]
fn tape_spacing_dropouts() {
    let busy = Condition { dropouts_per_minute: 6.0e4, depth_um: 2.0, duration_ms: 3.0, ..Condition::NONE };
    pin_stage("physical/tape/spacing_dropouts", "Spacing, 60 000 dropouts a minute, 2 µm, 3 ms, seed 1", Spacing::new(busy, 1));
}

// ---- E7: photocells ---------------------------------------------------------------

#[test]
fn vactrol() {
    let mut cell = Vactrol::new(Najnudel::VTL5C3);
    cell.prepare(RATE);
    // The LED driven 10 mA ± the stimulus; pinned as conductance in mS.
    pin("circuit/photocell/vactrol", "Vactrol VTL5C3, LED 10 mA × (1 + input), conductance in mS", |x| {
        (1e3 / cell.step(0.010 * (1.0 + f64::from(x)))) as f32
    });
}

#[test]
fn opto_compressor() {
    let mut opto = OptoCompressor::new(Najnudel::VTL5C3);
    opto.prepare(RATE);
    pin("circuit/opto_compressor", "OptoCompressor, VTL5C3, input ×10", |x| opto.tick(10.0 * x));
}

// ---- S7: tuned resonators, formants -------------------------------------------------

#[test]
fn comb_resonator() {
    pin_stage("primitives/resonators/comb", "Comb<Lagrange3>, 110 Hz, 2 s", Comb::<Lagrange3>::new(hz(40.0), hz(110.0), seconds(2.0)));
}

#[test]
fn karplus_strong() {
    let mut k = KarplusStrong::<Lagrange3>::new(hz(40.0), hz(110.0));
    k.set_decay(seconds(3.0), seconds(0.5));
    k.prepare(RATE);
    pin("primitives/resonators/karplus_strong", "KarplusStrong<Lagrange3>, 110 Hz, decay 3 s / 0.5 s, input ×0.01", |x| {
        k.tick(0.01 * x)
    });
}

#[test]
fn modal_bank() {
    let modes: Vec<Mode> = (1..=32)
        .map(|k| Mode {
            frequency: hz(110.0 * k as f32 * (1.0 + 0.001 * (k * k) as f32)),
            decay: seconds(2.0 / f64::from(k).sqrt()),
            gain: 1.0 / k as f32,
        })
        .collect();
    pin_stage("primitives/resonators/bank", "Bank, 32 stretched modes from 110 Hz", Bank::new(&modes));
}

#[test]
fn formants() {
    pin_stage("primitives/formants_aa", "Formants, /a/ (Peterson & Barney, man)", Formants::new(Vowel::Aa.formants(Speaker::Man)));
}

// ---- M7: a container of NAM networks ---------------------------------------------

#[test]
fn nam_container() {
    let text = include_str!("../../stave-dsp/tests/reference/nam/container.nam");
    let mut net = nam::parse(text).unwrap().network;
    net.prepare(RATE);
    pin(
        "learned/nam_container",
        "nam::Network::Container from the trainer's container.nam fixture, its default (last) submodel",
        |x| net.tick(x),
    );
}

// ---- the pins themselves ----------------------------------------------------------

/// A real change — the FDN's T60 2 s → 2.1 s — fails its digest, naming
/// where (as times and bands) and by how much; and every committed digest
/// parses, names the case its path says, and has a test above.
#[test]
fn a_moved_stage_is_named_and_every_digest_is_current() {
    use stave_test::golden::{path, root, stimulus, Digest};
    let committed = Digest::committed("effects/reverb/fdn_householder").unwrap().unwrap();
    let mut f = fdn(Mixing::Householder);
    f.set_decay(seconds(2.1));
    f.prepare(RATE);
    let out: Vec<f32> = stimulus().into_iter().map(|x| f.tick(x)).collect();
    let moved = committed.diff(&Digest::of("effects/reverb/fdn_householder", &committed.model, &out));
    assert!(!moved.is_empty() && moved[0].contains(" s, ") && moved[0].contains("tolerance"), "{moved:?}");

    let source = include_str!("golden.rs");
    let mut files = Vec::new();
    let mut dirs = vec![root()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "golden") {
                files.push(p);
            }
        }
    }
    assert!(files.len() >= 60, "{} digests", files.len());
    for file in files {
        let d = Digest::parse(&std::fs::read_to_string(&file).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        assert_eq!(path(&d.case), file, "{} names case {}", file.display(), d.case);
        let stem = d.case.trim_end_matches("/left").trim_end_matches("/right");
        assert!(source.contains(&format!("\"{stem}\"")), "{}: no test pins it (an orphan: delete it)", file.display());
    }
}
