//! Conformance: dynamics built from the ModMatrix, measured with the gain-trajectory instrument.
//!
//! The claims pinned here:
//!  1. A gain route driven by an EnvelopeFollower IS a compressor, and its
//!     attack/release knobs are honest (measured τ vs label).
//!  2. Post-chain tap detection (feedback topology) compresses
//!     differently from feed-forward for the same routes — the
//!     program-dependent, self-limiting behaviour of the classic
//!     feedback compressors — and is stable (contractive for
//!     k_gain < 1).

use stave_dsp::{
    shapes, Chain, CurveStage, EnvelopeFollower, EnvelopeTimes, Frame, ModMatrix, Route, Stage,
    StageId, StageKind, Target,
};
use stave_measure::dynamics::GainStepTest;

const STEP: GainStepTest = GainStepTest {
    frequency_hz: 1_000.0,
    sample_rate: FS,
    low_db: -30.0,
    high_db: -6.0,
    hold_seconds: 0.4,
    window: 48,
};

const FS: f64 = 48_000.0;

/// One near-linear gain stage + a Gain route = a compressor.
fn comp(atk_ms: f32, rel_ms: f32, amount: f32) -> (Chain<2>, StageId, ModMatrix<1, 2>) {
    let mut ch = Chain::<2>::new();
    let id = ch.push(StageKind::Curve(CurveStage::from_fn(
        -8.0,
        8.0,
        0.0,
        shapes::knee(1.2),
    )));
    ch.prepare(FS as f32);
    let mut mm = ModMatrix::<1, 2>::new([EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: atk_ms,
        release_ms: rel_ms,
        slow_release_ms: None,
    })]);
    mm.prepare(FS as f32);
    mm.push_route(Route {
        driver: 0,
        stage: id,
        target: Target::Gain,
        amount,
    });
    (ch, id, mm)
}

#[test]
fn modmatrix_gain_route_is_an_honest_compressor() {
    let base = Frame::NEUTRAL;
    let render = |ff: bool| {
        let (mut ch, id, mut mm) = comp(5.0, 100.0, 0.6);
        move |x: f32| {
            let det = if ff { x.abs() } else { ch.sense_out().abs() };
            mm.tick(&[det], &mut ch, &[(id, base)], &[]);
            ch.tick(x)
        }
    };
    // 1 ms windows resolve the 5 ms attack.
    let trajectory = STEP.run(render(true));
    let (gains, hold) = (&trajectory.gains_db, trajectory.hold_windows);
    let gr_settled = gains[2 * hold - 2] - gains[hold - 2];
    println!("feed-forward: settled GR {gr_settled:.2} dB");
    assert!(
        gr_settled < -3.0,
        "should compress the hot step: {gr_settled:.2} dB"
    );
    assert!(
        gains[hold - 2].abs() < 0.5,
        "quiet passage ~unity: {:.2} dB",
        gains[hold - 2]
    );
    // Attack: τ from step onset to settled GR. Label 5 ms; windowing and
    // detector ripple grant a loose ±.
    let atk = trajectory
        .time_constant_s(hold..2 * hold - 2)
        .expect("attack τ");
    println!("measured attack τ {:.1} ms (label 5)", atk * 1000.0);
    assert!(atk > 0.001 && atk < 0.02, "attack τ off: {} s", atk);
    // Release: from step-off back toward unity. Label 100 ms.
    let rel = trajectory
        .time_constant_s(2 * hold..gains.len() - 2)
        .expect("release τ");
    println!("measured release τ {:.0} ms (label 100)", rel * 1000.0);
    assert!(rel > 0.05 && rel < 0.2, "release τ off: {} s", rel);
}

#[test]
fn feedback_detection_differs_and_is_stable() {
    let base = Frame::NEUTRAL;
    let settled_gr = |ff: bool| -> (f64, f64) {
        let (mut ch, id, mut mm) = comp(5.0, 100.0, 0.6);
        let mut render = move |x: f32| {
            let det = if ff { x.abs() } else { ch.sense_out().abs() };
            mm.tick(&[det], &mut ch, &[(id, base)], &[]);
            ch.tick(x)
        };
        let trajectory = STEP.run(&mut render);
        let (gains, hold) = (&trajectory.gains_db, trajectory.hold_windows);
        // settle variance over the last quarter of the hot step — the
        // stability check (limit cycles would thrash the gain).
        let tail = &gains[(2 * hold - hold / 4)..(2 * hold - 1)];
        let mean = tail.iter().sum::<f64>() / tail.len() as f64;
        let var = tail.iter().map(|g| (g - mean) * (g - mean)).sum::<f64>() / tail.len() as f64;
        (gains[2 * hold - 2] - gains[hold - 2], var)
    };
    let (gr_ff, var_ff) = settled_gr(true);
    let (gr_fb, var_fb) = settled_gr(false);
    println!("settled GR: feed-forward {gr_ff:.2} dB | feedback {gr_fb:.2} dB");
    assert!(
        gr_fb < -1.5,
        "feedback topology should still compress: {gr_fb:.2}"
    );
    // Feedback detection sees the ALREADY-REDUCED signal, so for the same
    // routes it reduces less — the self-limiting classic-compressor trait.
    assert!(
        gr_fb > gr_ff + 0.5,
        "feedback should self-limit vs feed-forward: {gr_fb:.2} vs {gr_ff:.2}"
    );
    assert!(
        var_ff < 0.05 && var_fb < 0.05,
        "gain thrash: ff {var_ff:.4} fb {var_fb:.4}"
    );
}

/// Proof: BLOCKING DISTORTION as a sense-driven route. The curve
/// carries a grid-conduction sense column (excess above a threshold);
/// a fast-attack/slow-release driver keyed on that sense pushes the
/// stage's bias — under sustained overdrive the stage chokes (gain
/// drops), and it recovers on silence at the driver's release. This is
/// the amplifier cascade's headline behaviour expressed as data (driver + route +
/// sense), no bespoke code.
#[test]
fn sense_driven_route_reproduces_blocking() {
    let mut ch = Chain::<2>::new();
    let id = ch.push(StageKind::Curve(CurveStage::from_fn_with_sense(
        -6.0,
        6.0,
        0.0,
        shapes::knee(3.0),
        |x| (x - 0.8).max(0.0), // "grid conduction": excess above 0.8
    )));
    ch.prepare(FS as f32);
    let mut mm = ModMatrix::<1, 2>::new([EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: 1.0,
        release_ms: 60.0,
        slow_release_ms: None,
    })]);
    mm.prepare(FS as f32);
    // Conduction pushes bias NEGATIVE (toward cutoff) — the choke.
    mm.push_route(Route {
        driver: 0,
        stage: id,
        target: Target::Bias,
        amount: -1.2,
    });
    let base = Frame {
        bias: 0.0,
        drive: 3.0,
        gain: 1.0,
    };

    let render = |amp: f64, n: usize, ch: &mut Chain<2>, mm: &mut ModMatrix<1, 2>| -> f64 {
        let mut acc = 0.0f64;
        for i in 0..n {
            let x = (amp * (2.0 * core::f64::consts::PI * 220.0 * i as f64 / FS).sin()) as f32;
            let sense = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
                cs.sense_tap()
            } else {
                0.0
            };
            mm.tick(&[sense], ch, &[(id, base)], &[]);
            let y = ch.tick(x) as f64;
            if i >= n / 2 {
                acc += y * y;
            }
        }
        (acc / (n / 2) as f64).sqrt()
    };
    // Quiet: below the conduction threshold — no choke, reference level.
    let quiet_rms = render(0.15, 24_000, &mut ch, &mut mm);
    // Hot sustained: conduction charges the driver, bias chokes the stage.
    let hot_rms = render(0.9, 24_000, &mut ch, &mut mm);
    let bias_hot = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
        cs.pending_mut().bias
    } else {
        0.0
    };
    println!("blocking: quiet rms {quiet_rms:.4}, hot rms {hot_rms:.4}, hot bias {bias_hot:.3}");
    assert!(
        bias_hot < -0.15,
        "conduction should build negative bias: {bias_hot}"
    );
    // Recovery: silence releases the choke at the driver's release τ.
    for _ in 0..(FS * 0.5) as usize {
        let sense = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
            cs.sense_tap()
        } else {
            0.0
        };
        mm.tick(&[sense], &mut ch, &[(id, base)], &[]);
        let _ = ch.tick(0.0);
    }
    let bias_rest = if let Some(StageKind::Curve(cs)) = ch.stage_mut(id) {
        cs.pending_mut().bias
    } else {
        0.0
    };
    assert!(
        bias_rest.abs() < 0.02,
        "blocking should recover on silence: {bias_rest}"
    );
}

/// The self-biased triode's cathode loop: driven into grid conduction the
/// cathode rises, and in silence it recovers faster than its RC by the
/// loop factor 1 + gm·Rk (the shift suppresses the very current that
/// charges the cathode), at any sample rate.
#[test]
fn cathode_loop_recovers_faster_than_its_rc() {
    use stave_dsp::{Bias, Triode};
    let rc_ms = 60.0f64;
    let mut measured = vec![];
    for fs in [48_000.0f64, 96_000.0] {
        let mut t = Triode::new();
        t.prepare(fs as f32);
        t.set_grid_drive(37.11);
        let bypass_farads = rc_ms * 1e-3 / t.cathode_ohms();
        t.set_bias(Bias::Cathode { bypass_farads });
        for i in 0..(fs * 0.75) as usize {
            let x = 0.8 * (2.0 * core::f64::consts::PI * 200.0 * i as f64 / fs).sin();
            let _ = t.process_sample(x as f32);
        }
        let shift_hot = t.cathode_shift();
        assert!(shift_hot > 0.1, "no drift under program: {shift_hot}");
        let recovered = (0..fs as usize)
            .position(|_| {
                let _ = t.process_sample(0.0);
                t.cathode_shift() < shift_hot * (-1.0f64).exp()
            })
            .expect("recovers within a second");
        let tau_obs = recovered as f64 / fs * 1000.0;
        measured.push(tau_obs);
        assert!(
            tau_obs > rc_ms / 3.5 && tau_obs < rc_ms / 1.3,
            "loop factor out of range: {tau_obs} ms for RC {rc_ms}"
        );
    }
    let ratio = (measured[0] / measured[1]).max(measured[1] / measured[0]);
    assert!(ratio < 1.15, "rate-dependent recovery: {measured:?}");
}
