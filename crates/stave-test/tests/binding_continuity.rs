//! The structural-binding continuity check: an IN-KIND re-voice
//! preserves state. The motivating physics: in a diode clipper, a drive move re-voices the
//! RC clipper's conduction table in place, and the cap keeps its
//! charge; no state dump mid-note. Here that continuity rule is
//! framework-tested against the primitive itself.

use stave_dsp::RcClipperConfig;
use stave_dsp::{NonlinearRc, Stage};

const FS: f32 = 192_000.0; // the rate the RC stage runs at inside the 4x stack
const F0: f64 = 220.0;

fn drive_tone(rc: &mut NonlinearRc, n: usize, phase0: usize) -> f32 {
    let mut last = 0.0;
    for i in 0..n {
        let t = (phase0 + i) as f64;
        let x = (0.9 * (2.0 * std::f64::consts::PI * F0 * t / FS as f64).sin()) as f32;
        last = rc.tick(x);
    }
    last
}

#[test]
fn in_kind_revoice_keeps_the_cap_charge() {
    // Charge two identical stages with the same hot program.
    let mut revoiced = NonlinearRc::ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 3.0,
            threshold: 0.5,
            hardness: 4.0,
        },
        1.2,
    );
    let mut untouched = NonlinearRc::ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 3.0,
            threshold: 0.5,
            hardness: 4.0,
        },
        1.2,
    );
    revoiced.prepare(FS);
    untouched.prepare(FS);
    let n = 8192;
    drive_tone(&mut revoiced, n, 0);
    drive_tone(&mut untouched, n, 0);

    // In-kind re-voice mid-note (a drive move in the screamer stack).
    revoiced.set_ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 3.0,
            threshold: 0.5,
            hardness: 4.0,
        },
        1.6,
    );

    // A state-dumped impostor at the new voicing, fed the same tail.
    let mut dumped = NonlinearRc::ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 3.0,
            threshold: 0.5,
            hardness: 4.0,
        },
        1.6,
    );
    dumped.prepare(FS);

    let tail = 64;
    let mut d_untouched = 0.0f32; // voicing delta only (state shared)
    let mut d_dumped = 0.0f32; // state delta (charge lost)
    for i in 0..tail {
        let t = (n + i) as f64;
        let x = (0.9 * (2.0 * std::f64::consts::PI * F0 * t / FS as f64).sin()) as f32;
        let y_r = revoiced.tick(x);
        let y_u = untouched.tick(x);
        let y_d = dumped.tick(x);
        d_untouched = d_untouched.max((y_r - y_u).abs());
        d_dumped = d_dumped.max((y_r - y_d).abs());
    }
    // The re-voiced stage tracks its charged twin far more closely than
    // a fresh stage at the same voicing tracks either: the charge, not
    // the table, dominates the first samples after the move.
    assert!(
        d_dumped > 4.0 * d_untouched,
        "state dump indistinguishable from re-voice: dumped Δ {d_dumped:.4} vs voicing Δ {d_untouched:.4}"
    );
    assert!(
        d_untouched < 0.25,
        "in-kind re-voice jumped by {d_untouched:.4} — charge not preserved"
    );
}
