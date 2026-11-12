//! S14/S15: recursive state survives one bad sample, and kernel forms
//! never panic when ticked before `prepare`.

use core::num::NonZeroU32;
use stave_dsp::{Biquad, Chain, FilterSpec, FilterStage, Loop, StageId, StageKind, Stage, Svf};

const BLOCK: usize = 64;

/// Feed one NaN (and, separately, one +inf), then a block of finite
/// signal: every sample after the first post-injection tick is finite.
fn recovers(mut tick: impl FnMut(f32) -> f32, what: &str) {
    for bad in [f32::NAN, f32::INFINITY] {
        for i in 0..BLOCK {
            tick((i as f32 * 0.1).sin() * 0.5);
        }
        let _ = tick(bad);
        // The injection tick itself may emit the bad value; the state
        // must be clean by the next.
        for i in 0..BLOCK {
            let y = tick((i as f32 * 0.1).sin() * 0.5);
            assert!(y.is_finite(), "{what}: non-finite {y} {i} ticks after {bad}");
        }
    }
}

struct Gain(f32);
impl Stage for Gain {
    fn prepare(&mut self, _r: f32) {}
    fn reset(&mut self) {}
    fn tick(&mut self, x: f32) -> f32 {
        x * self.0
    }
}

#[test]
fn svf_recovers_from_one_nan() {
    let mut f = Svf::new();
    f.prepare(48_000.0);
    f.set(1_000.0, 0.9);
    recovers(|x| f.process(x, 0).lp, "svf lp");
    recovers(|x| f.process_lp(x, 1), "svf process_lp");
}

#[test]
fn biquad_recovers_from_one_nan() {
    let mut f = Biquad::new();
    f.set(FilterSpec::Lowpass { hz: 800.0, q: 4.0 }, 48_000.0);
    recovers(|x| f.process(x), "biquad");
}

#[test]
fn loop_feedback_recovers_from_one_nan() {
    let mut l = Loop::new(Gain(0.7), NonZeroU32::new(7).unwrap());
    l.prepare(48_000.0);
    recovers(|x| l.tick(x), "loop");
}

#[test]
fn chain_global_feedback_recovers_from_one_nan() {
    // Stateless stage: only the feedback register can hold the NaN.
    let mut c = Chain::<2>::new();
    let a = c.push(StageKind::Bypass);
    c.set_feedback(0.5, a, a, None);
    c.prepare(48_000.0);
    recovers(|x| c.tick(x), "chain (no loop filter)");

    // Filtered feedback around a filter stage.
    let mut c = Chain::<2>::new();
    let a = c.push(StageKind::Filter(FilterStage::new(FilterSpec::Lowpass {
        hz: 2_000.0,
        q: 0.7,
    })));
    c.set_feedback(
        -0.4,
        a,
        StageId(0),
        Some(FilterSpec::Highpass { hz: 100.0, q: 0.7 }),
    );
    c.prepare(48_000.0);
    recovers(|x| c.tick(x), "chain (filtered feedback)");
}

#[test]
fn loop_tick_before_prepare_does_not_panic() {
    let mut l = Loop::new(Gain(0.5), NonZeroU32::new(16).unwrap());
    for i in 0..BLOCK {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let y = l.tick(x);
        assert!(y.is_finite());
        assert_eq!(y, x * 0.5, "unprepared loop has no history yet");
    }
    let mut io = [0.25f32; BLOCK];
    l.process_block(&mut io);
    assert!(io.iter().all(|y| y.is_finite()));
}
