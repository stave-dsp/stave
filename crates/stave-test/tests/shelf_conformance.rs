//! Shelf curve limits.
//!
//! Measured 2026-07-04, +6 dB low shelf @ 200 Hz / 48 k, bin-centred:
//!   Hz        stave     rbj(S=1)
//!   46.9      +6.19     +5.98
//!   99.6      +6.63     +5.63     ← the stave lip (+0.63 over nominal)
//!   398.4     −0.63     +0.38     ← the stave dip past the corner
//!   802.7     −0.21     +0.03
//! The stave shelf's resonant corner is DELIBERATE VOICING (kept by
//! measurement — it shapes every companding/head-bump consumer); the
//! `_rbj` variants are the monotonic cookbook curve. These limits pin
//! both so neither drifts silently.

use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, Stage};
use stave_measure::tone_window::ToneWindow;
use stave_dsp::units::{Db, Hz};

const FS: f64 = 48_000.0;
const N: usize = 8192;

fn resp(bq: &mut Biquad, m: usize) -> f64 {
    let mut b = *bq;
    ToneWindow {
        bin: m,
        window: N,
        sample_rate: FS,
        amplitude: 0.01,
        warmup_windows: 4,
    }
    .gain_db(|x: f32| b.tick(x))
}

#[test]
fn stave_shelf_lip_and_dip_are_pinned() {
    let mut c = Biquad::new(FilterSpec::LowShelf { frequency: Hz::new(200.0), gain: Db::new(6.0) });
    c.prepare(FS as f32);
    let lip = resp(&mut c, 17); // ~99.6 Hz
    let dip = resp(&mut c, 68); // ~398 Hz
    let flat = resp(&mut c, 341); // ~2 kHz
    assert!(
        (lip - 6.626).abs() < 0.05,
        "lip moved: {lip:.3} dB (pinned 6.626)"
    );
    assert!(
        (dip + 0.629).abs() < 0.05,
        "dip moved: {dip:.3} dB (pinned −0.629)"
    );
    assert!(flat.abs() < 0.06, "tail not flat: {flat:.3} dB");
}

#[test]
fn rbj_shelf_is_monotonic_and_cookbook_flat() {
    let mut c = Biquad::new(FilterSpec::LowShelfRbj { frequency: Hz::new(200.0), gain: Db::new(6.0) });
    c.prepare(FS as f32);
    let below = resp(&mut c, 8); // ~46.9 Hz → near full gain
    let corner = resp(&mut c, 34); // ~199 Hz → ~half gain
    let above = resp(&mut c, 341); // ~2 kHz → ~0
    assert!((below - 5.98).abs() < 0.05, "below-corner: {below:.3}");
    assert!(
        (corner - 3.02).abs() < 0.06,
        "corner half-gain: {corner:.3}"
    );
    assert!(above.abs() < 0.02, "above-corner flat: {above:.3}");
    // Monotonic: no lip anywhere below the corner.
    let mut prev = f64::INFINITY;
    for m in [2usize, 8, 17, 34, 68, 137, 341] {
        let r = resp(&mut c, m);
        assert!(
            r <= prev + 0.02,
            "non-monotonic at bin {m}: {r:.3} after {prev:.3}"
        );
        prev = r;
    }
}
