//! Conformance: global chain feedback, measured.
//!
//! Two claims pinned:
//!  1. Negative feedback LINEARISES: at low drive, THD with NFB is
//!     measurably below the open-loop chain (the whole reason amps have
//!     it), while clipping behaviour remains at high drive.
//!  2. Stability is a MEASURED property: the probe sweeps loop gain and
//!     finds the oscillation onset; the asserted safe range sits below it
//!     with margin. Feedback knobs must be clamped to measured-safe
//!     ranges, never "tuned by ear".

use stave_dsp::{shapes, Chain, CurveStage, Frame, Stage, StageKind};
use stave_measure::dynamics::SilenceResidualTest;
use stave_measure::nonlinear::GainSurface;

const FS: f64 = 192_000.0; // inner rate: chains run inside 4× stacks

fn nfb_chain(fb: f32) -> Chain<3> {
    let mut ch = Chain::<3>::new();
    let mut a = CurveStage::from_fn(-8.0, 8.0, 0.0, shapes::knee(2.0));
    a.set_pending(Frame {
        bias: 0.05,
        drive: 3.0,
        gain: 1.0,
    });
    let id_a = ch.push(StageKind::Curve(a));
    let mut b = CurveStage::from_fn(-4.0, 4.0, 0.0, shapes::knee(4.0));
    b.set_pending(Frame {
        bias: 0.0,
        drive: 1.2,
        gain: 1.0,
    });
    let id_b = ch.push(StageKind::Curve(b));
    // NFB: output fed back (inverted) into the first stage's input.
    ch.set_feedback(fb, id_b, id_a, None);
    ch.prepare(FS as f32);
    ch
}

fn thd(ch: &mut Chain<3>, amp: f64) -> f64 {
    let f0 = 1_000.0;
    let n = (FS / f0) as usize * 32;
    let sig = |i: usize| (amp * (2.0 * core::f64::consts::PI * f0 * i as f64 / FS).sin()) as f32;
    for i in 0..n {
        let _ = ch.tick(sig(i));
    }
    let out: Vec<f64> = (n..2 * n).map(|i| ch.tick(sig(i)) as f64).collect();
    let g = |f: f64| stave_measure::spectrum::goertzel(&out, f / FS);
    let h1 = g(f0).max(1e-12);
    ((2..=6).map(|k| g(k as f64 * f0).powi(2)).sum::<f64>()).sqrt() / h1
}

#[test]
fn negative_feedback_linearises_low_drive_thd() {
    let open = thd(&mut nfb_chain(0.0), 0.15);
    let closed = thd(&mut nfb_chain(-0.4), 0.15);
    println!(
        "low-drive THD: open {:.3}% | NFB {:.3}%",
        open * 100.0,
        closed * 100.0
    );
    assert!(
        closed < open * 0.6,
        "NFB should linearise low-drive THD: {open} → {closed}"
    );
    // And the loop-gain surface still compresses when driven (feedback
    // does not remove the clip, it firms the approach to it).
    let s = GainSurface {
        freqs_hz: [1_000.0].to_vec(),
        levels_db: [-30.0, 0.0].to_vec(),
        window: 8192,
        sample_rate: FS,
        warmup_windows: 1,
    }
    .measure(|| {
        let mut ch = nfb_chain(-0.4);
        move |x: f32| ch.tick(x)
    });
    let comp = s.gains_db[0][0] - s.gains_db[1][0];
    println!("NFB chain compression (−30 → 0 dBFS): {comp:.2} dB");
    // Closed-loop small-signal gain is A/(1+βA) ≈ 1.3 here, so the knee
    // depth is shallower than open-loop by design — the limit reflects the
    // measured closed-loop figure.
    assert!(
        comp > 1.2,
        "NFB chain should still compress hot input: {comp:.2} dB"
    );
}

#[test]
fn stability_onset_is_measured_and_safe_range_holds() {
    // Sweep loop gain; residual after silence must stay at the noise
    // floor through the working range. Print the onset if one exists in
    // the swept span — that measurement is the spec for any knob range.
    // MEASURED result: this topology (A ≈ 3.6 small-signal, one-tick
    // loop delay) is dead-quiet through β = −0.4 and self-oscillates by
    // β = −0.6 — classic |βA| > 1 with delay. The safe knob range is
    // therefore 0..−0.4 with the onset as the hard wall; both are pinned.
    let mut onset: Option<f32> = None;
    for step in 0..=10 {
        let g = -0.1 * step as f32; // 0 .. −1.0
        let mut ch = nfb_chain(g);
        let resid = SilenceResidualTest {
            frequency_hz: 500.0,
            sample_rate: FS,
            amplitude: 0.8,
            burst_seconds: 0.05,
            silence_seconds: 0.1,
        }
        .residual_db(|x: f32| ch.tick(x));
        println!("loop gain {g:+.1}: post-silence AC residual {resid:.1} dB");
        if step <= 4 {
            assert!(
                resid < -80.0,
                "working range must be dead-quiet after silence: gain {g} residual {resid:.1} dB"
            );
        }
        if resid > -60.0 && onset.is_none() {
            onset = Some(g);
        }
    }
    let onset = onset.expect("sweep should find the oscillation wall");
    println!("measured oscillation onset: β = {onset:+.1}");
    assert!(
        onset <= -0.5,
        "onset moved into the working range — clamps are now wrong: {onset:+.1}"
    );
}
