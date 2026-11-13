//! Realtime safety of the delays, reverbs, dynamics, crossovers, formant
//! filters and resonators: after `prepare`,
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
    no_allocation("Delay<Lagrange3>", &mut Delay::<Lagrange3>::new(20.0, 7.3));
    no_allocation("Delay<Lagrange5>", &mut Delay::<Lagrange5>::new(20.0, 7.3));
    no_allocation("Delay<Thiran>", &mut Delay::<Thiran>::new(20.0, 7.3));
    no_allocation("Delay<Sinc<16>>", &mut Delay::<Sinc<16>>::new(20.0, 7.3));
    no_allocation("Fdn", &mut Fdn::new(Mixing::Hadamard));
    let mut graphic = Fdn::new(Mixing::Hadamard);
    graphic.set_t60(T60::uniform(seconds(2.0)));
    graphic.set_kronecker(Kronecker::hadamard());
    graphic.set_angle_modulation(0.4, hz(0.7));
    no_allocation("Fdn (graphic EQ, Kronecker)", &mut graphic);
    no_allocation("VelvetNoise", &mut VelvetNoise::new());
    no_allocation("Dattorro", &mut Dattorro::new(50.0));
    no_allocation("Schroeder", &mut Schroeder::new());
    no_allocation("Schroeder::moorer", &mut Schroeder::moorer());
    no_allocation("Peak", &mut Peak::new(1.0, 50.0));
    no_allocation("Rms", &mut Rms::new(10.0));
    no_allocation("ProgramDependent", &mut ProgramDependent::new(1.0, 50.0, 800.0, 400.0));
    no_allocation("Sidechain", &mut Sidechain::new(Some(80.0), Some(9_000.0)));
    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    no_allocation("Compressor", &mut Compressor::new(curve, Topology::FeedForward));
    no_allocation("Compressor (feedback)", &mut Compressor::new(curve, Topology::Feedback));
    no_allocation("Limiter (true peak)", &mut Limiter::new(Peaks::True, -1.0));
    no_allocation("Limiter (sample peak)", &mut Limiter::new(Peaks::Sample, -1.0));

    // Setters a modulation path calls per sample.
    let mut voice = Delay::<Lagrange3>::new(20.0, 10.0);
    voice.prepare(48_000.0);
    let mut room = Fdn::new(Mixing::Householder);
    room.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..1_000 {
            voice.set_ms(10.0 + (i as f32 * 0.01).sin());
            voice.tick(0.1);
            room.set_t60_s(1.0 + i as f32 * 1e-3);
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
