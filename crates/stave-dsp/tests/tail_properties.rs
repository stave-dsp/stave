//! Tail and latency are forwarded, and leaf tails are honest: each
//! filter's reported −120 dB tail covers its measured impulse response
//! without being wildly conservative, and every composite reports what
//! its parts imply.

use stave_dsp::{
    Allpass1, Biquad, Chain, Comb, Coupler, Crossover3, DcBlocker, DelayAllpass, FilterSpec,
    LinkwitzRiley4, OnePole, Oversampled2x, Par2, PassiveToneControlPositions, PassiveToneStack,
    PassiveToneStackComponents, PerChannel, Serial, Stage, StageKind, Svf, Tail,
};
use stave_dsp::units::{Db, Hz};
use stave_dsp::units::hz;
use stave_dsp::units::ms;

const SR: f32 = 48_000.0;

/// The last impulse-response sample louder than −120 dB.
fn measured_tail(stage: &mut dyn Stage, limit: usize) -> usize {
    stage.reset();
    let mut last = 0;
    for i in 0..limit {
        let y = stage.tick(if i == 0 { 1.0 } else { 0.0 });
        if y.abs() > 1e-6 {
            last = i;
        }
    }
    last
}

fn check(name: &str, stage: &mut dyn Stage) {
    stage.prepare(SR);
    let Tail::Samples(reported) = stage.tail() else {
        panic!("{name}: a decaying filter reports a finite tail, got {:?}", stage.tail());
    };
    let measured = measured_tail(stage, 4 * reported as usize + 64);
    assert!(
        measured <= reported as usize,
        "{name}: rang {measured} samples past its reported {reported}"
    );
    assert!(
        measured + 64 >= reported as usize / 4,
        "{name}: reported {reported} for a {measured}-sample ring"
    );
}

#[test]
fn filter_leaves_report_honest_tails() {
    check(
        "biquad lowpass",
        &mut Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(200.0), q: 0.707 }),
    );
    check(
        "biquad peak",
        &mut Biquad::new(FilterSpec::Peak {
            frequency: Hz::new(1_000.0),
            gain: Db::new(6.0),
            q: 2.0,
        }),
    );
    check("svf", &mut Svf::new(hz(300.0), 0.707));
    check("one-pole", &mut OnePole::lowpass(hz(50.0)));
    check("dc blocker", &mut DcBlocker::new(hz(20.0)));
    check("allpass1", &mut Allpass1::new(hz(100.0)));
    check("lr4", &mut LinkwitzRiley4::lowpass(hz(150.0)));
    check("crossover3", &mut Crossover3::new(hz(150.0), hz(2_000.0)));
    check("coupler", &mut Coupler::new(hz(20.0)));
    check("comb", &mut Comb::new(ms(20.0), ms(5.0), 0.7));
    check("delay allpass", &mut DelayAllpass::new(ms(20.0), ms(3.0), 0.6));
    let mut stack = PassiveToneStack::new(PassiveToneStackComponents {
        treble_capacitance_farads: 250e-12,
        bass_capacitance_farads: 100e-9,
        midrange_capacitance_farads: 47e-9,
        treble_potentiometer_ohms: 250e3,
        bass_potentiometer_ohms: 1e6,
        midrange_potentiometer_ohms: 25e3,
        slope_resistance_ohms: 100e3,
    });
    stack.set_controls(PassiveToneControlPositions::new(0.5, 0.5, 0.5));
    check("tone stack", &mut stack);
}

#[test]
fn composites_forward_tail_and_latency() {
    let lp = || Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(200.0), q: 0.707 });
    let mut single = lp();
    single.prepare(SR);
    let t = single.tail();

    let mut serial = Serial((lp(), lp()));
    serial.prepare(SR);
    assert_eq!(serial.tail(), t.then(t), "serial sums");

    let mut par = Par2::new(lp(), Biquad::default());
    par.prepare(SR);
    assert_eq!(par.tail(), t, "parallel maxes");

    let mut kind = StageKind::Filter(lp());
    kind.prepare(SR);
    assert_eq!(kind.tail(), t, "StageKind forwards");
    assert_eq!(StageKind::Bypass.tail(), Tail::None);

    let mut chain = Chain::<2>::new();
    chain.push(StageKind::Filter(lp())).unwrap();
    chain.push(StageKind::Filter(lp())).unwrap();
    chain.prepare(SR);
    assert_eq!(chain.tail(), t.then(t), "chain sums");

    let mut lanes: PerChannel<Biquad> = PerChannel::new(|_| lp());
    lanes.prepare(SR);
    assert_eq!(lanes.tail(), t, "lanes are parallel");

    // A region: the interior's tail at half the rate, plus the pair.
    let mut region = Oversampled2x::front(lp());
    region.prepare(SR);
    let mut inner = lp();
    inner.prepare(2.0 * SR);
    assert_eq!(region.tail(), inner.tail().div_ceil(2).plus(66));
    assert_eq!(region.latency(), 66);
}
