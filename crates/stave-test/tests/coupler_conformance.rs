//! LF one-pole conformance (f64 LF poles): the instrument that
//! justifies `Coupler`/the Hysteresis blocker being f64. At 4× rates a
//! 5 Hz pole sits ~1.6e-4 from unity; this file MEASURES what f32 does
//! wrong there (corner displacement from coefficient rounding, residual
//! DC under program from recursion rounding) and pins both the defect
//! and the fix. MEASURED 2026-07-04 (192 kHz, 5 Hz corner, full-scale
//! 1 kHz + 0.5 DC program): residual DC f32 −103.4 dBFS vs f64
//! −172.7 dBFS — the f32 recursion recirculates rounding error through
//! a pole 1.6e-4 from unity and parks 69 dB of DC above the f64 floor.

use stave_dsp::{Coupler, Stage};
use stave_dsp::units::hz;

const FS4X: f32 = 192_000.0;

/// The pre-Step-6 blocker, verbatim: f32 coefficient AND state.
struct F32Coupler {
    r: f32,
    x1: f32,
    y1: f32,
}

impl F32Coupler {
    fn new(hz: f32, rate: f32) -> Self {
        Self {
            r: (-2.0 * core::f32::consts::PI * hz / rate).exp(),
            x1: 0.0,
            y1: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let out = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = out;
        out
    }
}

/// RMS gain (dB) of a tone at `f` through `tick`, settled first.
fn tone_gain_db(f: f64, fs: f64, mut tick: impl FnMut(f32) -> f32) -> f64 {
    let period = fs / f;
    let settle = (10.0 * fs / f) as usize; // 10 periods to settle
    let n = (64.0 * period) as usize; // integer periods
    let mut i = 0usize;
    let sample = |i: usize| (2.0 * core::f64::consts::PI * f * i as f64 / fs).sin() as f32;
    let mut sum_in = 0.0f64;
    let mut sum_out = 0.0f64;
    for _ in 0..settle {
        let _ = tick(sample(i));
        i += 1;
    }
    for _ in 0..n {
        let x = sample(i);
        let y = tick(x);
        sum_in += f64::from(x) * f64::from(x);
        sum_out += f64::from(y) * f64::from(y);
        i += 1;
    }
    10.0 * (sum_out / sum_in).log10()
}

/// Residual DC (dBFS) after the blocker, under PROGRAM: full-scale
/// 1 kHz tone riding on +0.5 DC. Mean over integer periods after a
/// long settle — the number a DC blocker exists to make small.
fn residual_dc_dbfs(fs: f64, mut tick: impl FnMut(f32) -> f32) -> f64 {
    let f = 1_000.0;
    let period = (fs / f) as usize; // exactly 192 at 192k
    let settle = (4.0 * fs) as usize; // 4 s — many coupler τ (τ ≈ 32 ms)
    let n = 200 * period;
    let mut i = 0usize;
    let sample = |i: usize| (0.5 + (2.0 * core::f64::consts::PI * f * i as f64 / fs).sin()) as f32;
    for _ in 0..settle {
        let _ = tick(sample(i));
        i += 1;
    }
    let mut acc = 0.0f64;
    for _ in 0..n {
        acc += f64::from(tick(sample(i)));
        i += 1;
    }
    let dc = (acc / n as f64).abs().max(1e-30);
    20.0 * dc.log10()
}

#[test]
fn corner_is_accurate_at_4x_rate() {
    let mut c = Coupler::new(hz(5.0));
    c.prepare(FS4X);
    let g5 = tone_gain_db(5.0, FS4X as f64, |x| c.tick(x));
    // A first-order RC is −3.01 dB at its corner. The f64 pole puts it
    // there; tolerance covers the measurement, not the filter.
    assert!(
        (g5 + 3.01).abs() < 0.1,
        "5 Hz corner off: measured {g5:.3} dB (want −3.01)"
    );
    // In-band transparency: −0.01 dB or better by 100 Hz.
    let mut c = Coupler::new(hz(5.0));
    c.prepare(FS4X);
    let g100 = tone_gain_db(100.0, FS4X as f64, |x| c.tick(x));
    assert!(g100 > -0.02, "100 Hz not transparent: {g100:.4} dB");
}

#[test]
fn f64_blocker_beats_f32_on_residual_dc() {
    let mut f64c = Coupler::new(hz(5.0));
    f64c.prepare(FS4X);
    let dc64 = residual_dc_dbfs(FS4X as f64, |x| f64c.tick(x));
    let mut f32c = F32Coupler::new(5.0, FS4X);
    let dc32 = residual_dc_dbfs(FS4X as f64, |x| f32c.tick(x));
    println!("residual DC under program at 192 k: f32 {dc32:.1} dBFS, f64 {dc64:.1} dBFS");
    // MEASURED 2026-07-04: f32 −103.4 dBFS, f64 −172.7 dBFS — a 69 dB
    // win. The f32 number is the DEFECT pin: if it ever reads clean,
    // the instrument stopped seeing the problem (and the f64 migration
    // stopped being justified) — investigate before touching either.
    assert!(
        dc64 < -165.0,
        "f64 blocker floor moved: {dc64:.1} dBFS (pinned −172.7)"
    );
    assert!(
        (-115.0..=-90.0).contains(&dc32),
        "f32 defect out of family: {dc32:.1} dBFS (pinned −103.4) — instrument drifted?"
    );
}
