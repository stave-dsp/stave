//! Realtime safety of the delays, reverbs, dynamics, crossovers, formant
//! filters, resonators, the plate, the rotary speaker, the loudspeaker
//! driver, the cabinet stages and the instruments: after `prepare`,
//! `tick`, setters that are documented as cheap, and `reset` never
//! allocate. A counting global allocator, armed per thread, watches.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use stave_dsp::effects::dynamics::{
    Compressor, GainComputer, Limiter, Link, Multiband, Peak, Peaks, ProgramDependent, Rms,
    Sidechain, Topology,
};
use stave_dsp::primitives::filters::crossover::{Crossover, Order};
use stave_dsp::primitives::filters::formant::{Formants, Speaker, Vowel};
use stave_dsp::primitives::resonators::{Bank, Comb, KarplusStrong, Mode};
use stave_dsp::units::{db, hz, seconds};
use stave_dsp::effects::reverb::{Dattorro, Fdn, Kronecker, Mixing, Schroeder, VelvetNoise, T60};
use stave_dsp::primitives::time::delay::Delay;
use stave_dsp::primitives::time::interpolators::{Lagrange3, Lagrange5, Sinc, Thiran};
use stave_dsp::Stage;
use stave_dsp::units::Seconds;
use stave_dsp::units::ms;

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            COUNT.with(|c| c.set(c.get() + 1));
        }
    });
}

// SAFETY: every method forwards to `System` with the caller's arguments,
// unchanged; counting touches only thread-local cells.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc`, passed through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` via `alloc`/`realloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `realloc`, passed through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn allocations_in(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

/// Prepare `stage`, then run, reset and run again with counting armed.
fn no_allocation(name: &str, stage: &mut dyn Stage) {
    stage.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..4_096 {
            stage.tick(((i * 37) % 101) as f32 / 25.0 - 2.0);
        }
        stage.reset();
        for _ in 0..256 {
            stage.tick(1.5);
        }
    });
    assert_eq!(n, 0, "{name} allocated {n} times after prepare");
}

#[test]
#[cfg_attr(miri, ignore)]
fn effects_never_allocate_after_prepare() {
    no_allocation("Delay<Lagrange3>", &mut Delay::<Lagrange3>::new(ms(20.0), ms(7.3)));
    no_allocation("Delay<Lagrange5>", &mut Delay::<Lagrange5>::new(ms(20.0), ms(7.3)));
    no_allocation("Delay<Thiran>", &mut Delay::<Thiran>::new(ms(20.0), ms(7.3)));
    no_allocation("Delay<Sinc<16>>", &mut Delay::<Sinc<16>>::new(ms(20.0), ms(7.3)));
    no_allocation("Fdn", &mut Fdn::new(Mixing::Hadamard));
    let mut graphic = Fdn::new(Mixing::Hadamard);
    graphic.set_t60(T60::uniform(seconds(2.0)));
    graphic.set_kronecker(Kronecker::hadamard());
    graphic.set_angle_modulation(0.4, hz(0.7));
    no_allocation("Fdn (graphic EQ, Kronecker)", &mut graphic);
    no_allocation("VelvetNoise", &mut VelvetNoise::new());
    no_allocation("Dattorro", &mut Dattorro::new(ms(50.0)));
    no_allocation("Schroeder", &mut Schroeder::new());
    no_allocation("Schroeder::moorer", &mut Schroeder::moorer());
    no_allocation("Peak", &mut Peak::new(ms(1.0), ms(50.0)));
    no_allocation("Rms", &mut Rms::new(ms(10.0)));
    no_allocation("ProgramDependent", &mut ProgramDependent::new(ms(1.0), ms(50.0), ms(800.0), ms(400.0)));
    no_allocation("Sidechain", &mut Sidechain::new(Some(hz(80.0)), Some(hz(9_000.0))));
    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    no_allocation("Compressor", &mut Compressor::new(curve, Topology::FeedForward));
    no_allocation("Compressor (feedback)", &mut Compressor::new(curve, Topology::Feedback));
    no_allocation("Limiter (true peak)", &mut Limiter::new(Peaks::True, db(-1.0)));
    no_allocation("Limiter (sample peak)", &mut Limiter::new(Peaks::Sample, db(-1.0)));

    // Setters a modulation path calls per sample.
    let mut voice = Delay::<Lagrange3>::new(ms(20.0), ms(10.0));
    voice.prepare(48_000.0);
    let mut room = Fdn::new(Mixing::Householder);
    room.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..1_000 {
            voice.set_delay(ms(f64::from(10.0 + (i as f32 * 0.01).sin())));
            voice.tick(0.1);
            room.set_decay(Seconds::new(f64::from(1.0 + i as f32 * 1e-3)));
            room.tick_stereo(0.1);
        }
        // A per-band design, a matrix angle and a freeze, as a knob or
        // an automation lane moves them.
        for i in 0..20 {
            room.set_t60(T60::uniform(seconds(1.0 + f64::from(i) * 0.1)));
            room.set_kronecker(Kronecker::new(Default::default(), [0.1 * i as f32; 3]));
            room.set_freeze(i % 2 == 0);
            room.tick_stereo(0.1);
        }
    });
    assert_eq!(n, 0, "setters allocated {n} times");
}

/// The EQs, pitch shifters and granulator (DSP.md S5, S6): `tick`,
/// `reset` and their realtime setters never allocate after `prepare` —
/// a linear-phase band change redesigns and swaps its FIR in place.
#[test]
#[cfg_attr(miri, ignore)]
fn spectral_effects_never_allocate_after_prepare() {
    use stave_dsp::effects::eq::{Band, Detector, Dynamic, DynamicBand, Dynamics, LinearPhase, MinimumPhase, Phase};
    use stave_dsp::effects::granular::Granulator;
    use stave_dsp::effects::pitch::{PhaseVocoder, Psola, Reconstruction};
    use stave_dsp::units::{db, hz, ms, seconds, semitones, Ratio};

    let bell = |g: f32| Band::Bell { frequency: hz(1_000.0), gain: db(g), q: 2.0 };
    let bands = [Band::LowCut { frequency: hz(40.0), q: 0.707 }, bell(3.0)];
    let dynamics = Dynamics {
        threshold: db(-30.0),
        ratio: Ratio::new(4.0),
        knee: db(6.0),
        range: db(-9.0),
        attack: ms(1.0),
        release: ms(50.0),
        detector: Detector::Rms(ms(10.0)),
    };
    let dynamic_bands = [
        DynamicBand::fixed(bands[0], Phase::Linear),
        DynamicBand::dynamic(bell(0.0), Phase::Minimum, dynamics),
        DynamicBand::dynamic(bell(2.0), Phase::Linear, dynamics),
    ];
    no_allocation("MinimumPhase", &mut MinimumPhase::new(&bands));
    no_allocation("LinearPhase", &mut LinearPhase::new(&bands));
    no_allocation("Dynamic", &mut Dynamic::new(&dynamic_bands));
    for r in [Reconstruction::Prusa, Reconstruction::LarocheDolson] {
        let mut pv = PhaseVocoder::new(r, semitones(12.0));
        pv.set_shift(semitones(7.0));
        no_allocation("PhaseVocoder", &mut pv);
    }
    let mut psola = Psola::new(hz(60.0), hz(1_000.0));
    psola.set_shift(semitones(-5.0));
    no_allocation("Psola", &mut psola);
    let mut cloud = Granulator::new(seconds(1.0), 32);
    cloud.set_density(80.0);
    cloud.set_pitch(semitones(7.0));
    cloud.set_spray(ms(50.0));
    no_allocation("Granulator", &mut cloud);

    // Band changes and shifts while running.
    let mut linear = LinearPhase::new(&bands);
    linear.prepare(48_000.0);
    let mut dynamic = Dynamic::new(&dynamic_bands);
    dynamic.prepare(48_000.0);
    let mut pv = PhaseVocoder::new(Reconstruction::Prusa, semitones(12.0));
    pv.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..6_000 {
            let x = (i as f32 * 0.07).sin();
            if i % 700 == 0 {
                let g = (i / 700) as f32 - 4.0;
                linear.set_band(1, bell(g));
                dynamic.set_band(2, bell(g), Some(dynamics));
                pv.set_shift(semitones(g));
            }
            linear.tick(x);
            dynamic.tick(x);
            pv.tick(x);
        }
    });
    assert_eq!(n, 0, "band changes allocated {n} times");
}

#[test]
#[cfg_attr(miri, ignore)]
fn bands_and_resonators_never_allocate_after_prepare() {
    let corners = [hz(120.0), hz(800.0), hz(5_000.0)];
    let lr = Crossover::<4>::linkwitz_riley(Order::Lr8, &corners);
    let lin = Crossover::<4>::linear_phase(&corners, hz(200.0), db(80.0));
    no_allocation("Crossover (LR8)", &mut lr.clone());
    no_allocation("Crossover (linear phase)", &mut lin.clone());
    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    let comp = |_| Compressor::new(curve, Topology::FeedForward);
    no_allocation("Multiband (LR8)", &mut Multiband::new(lr.clone(), comp));
    no_allocation("Multiband (linear phase)", &mut Multiband::new(lin, comp));
    let vowel = Vowel::Aa.formants(Speaker::Man);
    no_allocation("Formants", &mut Formants::new(vowel));
    no_allocation("Comb resonator", &mut Comb::<Lagrange3>::new(hz(40.0), hz(110.0), seconds(2.0)));
    no_allocation("KarplusStrong", &mut KarplusStrong::<Sinc<16>>::new(hz(40.0), hz(110.0)));
    let modes: Vec<Mode> = (1..=16)
        .map(|k| Mode { frequency: hz(110.0 * k as f32), decay: seconds(1.0), gain: 0.5 })
        .collect();
    no_allocation("Bank", &mut Bank::new(&modes));

    // What moves per sample: corners, switches, link, keyed stereo, a
    // morphing vowel, a gliding string, a retuned mode.
    let mut mb = Multiband::new(lr, comp);
    mb.prepare(48_000.0);
    let mut f = Formants::new(vowel);
    f.prepare(48_000.0);
    let mut string = KarplusStrong::<Thiran>::new(hz(40.0), hz(110.0));
    string.prepare(48_000.0);
    let mut bank = Bank::new(&modes);
    bank.prepare(48_000.0);
    let ee = Vowel::Iy.formants(Speaker::Man);
    let n = allocations_in(|| {
        for i in 0..2_000 {
            let t = i as f32 / 2_000.0;
            mb.set_corner(1, hz(800.0 + 400.0 * t));
            mb.set_solo(i % 4, i % 3 == 0);
            mb.set_bypass(i % 4, i % 5 == 0);
            mb.set_link(if i % 2 == 0 { Link::Max } else { Link::Mean });
            mb.tick_stereo_linked(0.3, -0.2);
            mb.tick_stereo_keyed(0.3, -0.2, 0.9, 0.0);
            mb.tick_stereo(0.1, 0.1);
            f.morph(vowel, ee, t);
            f.tick(0.1);
            string.set_pitch(hz(110.0 + 20.0 * t));
            string.set_decay(seconds(2.0), seconds(0.3 + f64::from(t)));
            if i % 500 == 0 {
                string.pluck(0.8);
            }
            string.tick(0.0);
            bank.set(i % 16, Mode { frequency: hz(200.0 + t), decay: seconds(0.5), gain: 0.3 });
            bank.tick(0.1);
        }
    });
    assert_eq!(n, 0, "per-sample setters allocated {n} times");
}

/// The plate and the rotary speaker (DSP.md H3, H4): `tick`, the block
/// and stereo forms, `reset` and every realtime setter never allocate
/// after `prepare` — moving a pickup, changing the loss, switching the
/// rotors' speed, moving a microphone or the walls.
#[test]
#[cfg_attr(miri, ignore)]
fn plate_and_rotary_never_allocate_after_prepare() {
    use stave_dsp::physical::plate::{Loss, Output, Plate, Point};
    use stave_dsp::physical::rotary::{Cabinet, Directivity, Mic, Radiator, Rotor, Speed, Wall};

    let mut small = Plate::emt140(seconds(2.0));
    small.set_bandwidth(hz(1_000.0));
    no_allocation("Plate", &mut small);
    no_allocation("Radiator", &mut Radiator::<Lagrange3>::new(Rotor::HORN_122H, Directivity::HORN_122H));
    no_allocation("Cabinet", &mut Cabinet::<Lagrange3>::leslie_122h());

    let mut plate = Plate::emt140(seconds(2.0));
    plate.set_bandwidth(hz(1_000.0));
    plate.prepare(48_000.0);
    let mut cab = Cabinet::<Lagrange3>::leslie_122h();
    cab.prepare(48_000.0);
    let input = [0.25f32; 256];
    let (mut l, mut r) = ([0.0f32; 256], [0.0f32; 256]);
    let n = allocations_in(|| {
        for i in 0..20 {
            let t = i as f64 / 20.0;
            plate.set_pickup(i % 2, Point::new(0.2 + 0.5 * t, 0.7 - 0.3 * t));
            plate.set_drive(Point::new(0.4 + 0.1 * t, 0.5));
            plate.set_loss(Loss::emt140(seconds(1.0 + 3.0 * t)).unwrap());
            plate.set_output(if i % 3 == 0 { Output::Acceleration } else { Output::Velocity });
            plate.process_stereo(&input, &mut l, &mut r);
            plate.tick_stereo(0.1);
            let mut block = input;
            plate.process(&mut block);
            cab.set_speed([Speed::Chorale, Speed::Tremolo, Speed::Stop][i % 3]);
            cab.set_mic(i % 2, Mic::at(30.0 + 5.0 * i as f64, 0.5 + 0.1 * t));
            cab.horn().set_walls(&[Wall::BACK_122H, Wall { angle: 90.0, distance: 0.3, reflection: 0.2 }]);
            cab.drum().set_directivity(Directivity::OMNI);
            for _ in 0..256 {
                cab.tick_stereo(0.1);
            }
        }
        plate.reset();
        cab.reset();
    });
    assert_eq!(n, 0, "plate and rotary setters allocated {n} times");
}

/// The loudspeaker driver and the cabinet stages (DSP.md E9, H7): `tick`,
/// `reset` and every realtime setter never allocate after `prepare` —
/// the drive and source resistance, a microphone's placement (its
/// response re-blended and crossfaded), gains and polarity, timing, a
/// close microphone's placement and breakup, the room's switches.
#[test]
#[cfg_attr(miri, ignore)]
fn speaker_and_cabinet_never_allocate_after_prepare() {
    use stave_dsp::physical::cabinet::{Breakup, Captured, CloseMic, Pattern, Placement, Position, Timing};
    use stave_dsp::effects::reverb::{Room, Shoebox};
    use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
    use stave_dsp::ImpulseResponse;

    let mut driver = Driver::new(ThieleSmall::JENSEN_P12N_LR2, LargeSignal::from_limits(1.0e-3, 1.2e-3, 6e-3));
    driver.prepare(48_000.0);
    let ir = |k: usize| {
        let v: Vec<f32> = (0..2_048).map(|n| ((n * k) as f32 * 0.37).sin() * (-(n as f32) / 300.0).exp()).collect();
        ImpulseResponse::mono(&v)
    };
    let mut cab = Captured::new(vec![
        vec![Position::new(0.0, ir(1)), Position::new(1.0, ir(2)), Position::new(2.0, ir(3))],
        vec![Position::new(0.0, ir(4))],
    ]);
    cab.prepare(48_000.0);
    let mut mic = CloseMic::new(Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::LINEAR), Placement::CLOSE, Breakup::PAPER_12);
    mic.prepare(48_000.0);
    let mut room = Room::new(Shoebox::LIVE_ROOM);
    room.prepare(48_000.0);
    let n = allocations_in(|| {
        for k in 0..20_000 {
            let x = ((k as f32) * 0.05).sin() * 0.5;
            if k % 1_000 == 0 {
                driver.set_volts(5.0 + (k / 1_000) as f64);
                driver.set_source_ohms((k / 1_000) as f64 * 0.5);
                cab.set_placement(0, (k / 1_000) as f32 * 0.1);
                cab.set_gain(1, 0.5, k % 2_000 == 0);
                cab.set_timing(if k % 3_000 == 0 { Timing::Aligned } else { Timing::AsCaptured });
                let distance = 0.03 + (k / 1_000) as f64 * 0.01;
                mic.set_placement(Placement { distance, offset: 0.01, pattern: Pattern::SUPERCARDIOID });
                mic.set_breakup(if k % 2_000 == 0 { Breakup::NONE } else { Breakup::PAPER_12 });
                room.set_early(k % 2_000 == 0);
            }
            driver.tick(x);
            cab.tick(x);
            mic.tick(x);
            room.tick(x);
        }
        driver.reset();
        cab.reset();
        mic.reset();
        room.reset();
    });
    assert_eq!(n, 0, "the speaker and cabinet stages allocated {n} times");
}

/// The instruments (DSP.md H10): strings plucked, struck and bowed, a
/// blown tube, a drumhead and a bar — `tick`, `reset` and every setter
/// (excitations, the bow, retuning, the valve, the strike and pickup
/// points, the loss) never allocate after `prepare`.
#[test]
#[cfg_attr(miri, ignore)]
fn instruments_never_allocate_after_prepare() {
    use stave_dsp::physical::bar::{Bar, Beam, Boundary, Material};
    use stave_dsp::physical::membrane::{Drumhead, Loss, Membrane, Output, Polar};
    use stave_dsp::physical::string::{Bow, Friction, Waveguide as StringGuide};
    use stave_dsp::physical::tube::{Bore, Shape, Valve, Waveguide as TubeGuide};

    let loss = Loss::two_point(hz(100.0), seconds(2.0), hz(2_000.0), seconds(0.5)).unwrap();
    let clarinet = Bore { length: 0.6, radius: 0.0075, shape: Shape::Cylinder };
    let mut drum = Membrane::new(Drumhead::tuned(0.15, 0.26, hz(100.0)), loss);
    drum.set_bandwidth(hz(2_000.0));
    let key = Beam::tuned(Material::ALUMINIUM, 0.008, Boundary::Free, hz(880.0));
    no_allocation("String", &mut StringGuide::<Lagrange3>::new(hz(40.0), hz(110.0)));
    no_allocation("Tube", &mut TubeGuide::<Lagrange3>::new(clarinet, Valve::Reed));
    no_allocation("Membrane", &mut drum.clone());
    no_allocation("Bar", &mut Bar::new(key, Boundary::Free, loss));

    let mut string = StringGuide::<Thiran>::new(hz(40.0), hz(110.0));
    string.prepare(48_000.0);
    let mut tube = TubeGuide::<Lagrange3>::new(clarinet, Valve::Reed);
    tube.prepare(48_000.0);
    drum.prepare(48_000.0);
    let mut bar = Bar::new(key, Boundary::Clamped, loss);
    bar.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..400 {
            let t = i as f64 / 400.0;
            if i % 50 == 0 {
                string.set_pitch(hz(110.0 + 40.0 * t as f32));
                string.set_inharmonicity(4e-4 * t);
                string.set_dispersion_sections(4 + i % 12);
                string.set_decay(seconds(3.0), seconds(0.3 + t));
                string.set_position(0.05 + 0.4 * t);
                string.pluck(0.5, seconds(0.002));
                drum.set_drive(Polar::new(0.6 * t, t));
                drum.set_pickup(Polar::new(0.3, 2.0 * t));
                drum.set_loss(loss);
                drum.set_output(if i % 100 == 0 { Output::Acceleration } else { Output::Velocity });
                bar.set_drive(t);
                bar.set_pickup(1.0 - t);
                bar.set_loss(loss);
                tube.set_bore(Bore { length: 0.6 - 0.2 * t, ..clarinet });
                tube.set_valve(if i % 100 == 0 { Valve::Lip { frequency: hz(300.0), q: 10.0 } } else { Valve::Reed });
            }
            string.set_bow(if i % 3 == 0 { Some(Bow { force: 0.4, velocity: 0.2 }) } else { None });
            string.set_friction(Friction::ROSIN);
            if i % 25 == 0 {
                string.strike(0.3, seconds(0.001));
            }
            tube.set_blow(0.6 * t);
            tube.set_embouchure(0.3);
            for _ in 0..32 {
                string.tick(0.0);
                tube.tick(0.0);
                drum.tick(0.1);
                bar.tick(0.1);
            }
        }
        string.reset();
        tube.reset();
        drum.reset();
        bar.reset();
    });
    assert_eq!(n, 0, "instrument setters allocated {n} times");
}

/// The tape transport (DSP.md H5): the capstan and the spacing — `tick`,
/// `reset` and every realtime call (stop, start, splices, speed, wear)
/// never allocate after `prepare`.
#[test]
#[cfg_attr(miri, ignore)]
fn tape_transport_never_allocates_after_prepare() {
    use stave_dsp::physical::tape::{Capstan, Condition, Drift, Machine, Motor, Spacing, Splices};

    let full = || Capstan::new(3).with_drift(Drift::LOOSE).with_motor(Motor::CONSUMER).with_splices(Splices::EDITED);
    no_allocation("Capstan", &mut full());
    no_allocation("Spacing", &mut Spacing::new(Condition::WORN, 3));

    let mut capstan = full();
    capstan.prepare(48_000.0);
    let mut spacing = Spacing::new(Condition::WORN, 3);
    spacing.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..400 {
            match i % 100 {
                10 => capstan.stop(),
                60 => capstan.start(),
                _ => {}
            }
            if i % 7 == 0 {
                capstan.splice();
                spacing.splice();
            }
            capstan.set_speed(Machine::ips(if i % 2 == 0 { 7.5 } else { 15.0 }));
            spacing.set_speed(Machine::ips(7.5));
            spacing.set_wear((i % 10) as f64 / 10.0);
            for _ in 0..256 {
                let y = capstan.tick(0.1);
                spacing.set_speed_ratio(capstan.speed_ratio());
                spacing.tick(y);
            }
        }
        capstan.reset();
        spacing.reset();
    });
    assert_eq!(n, 0, "the tape transport allocated {n} times");
}
