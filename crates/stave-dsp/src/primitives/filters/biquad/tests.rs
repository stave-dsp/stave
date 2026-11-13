use super::*;

/// RMS gain in dB of `f` on a settled sine at `hz`.
fn tone_db(f: &mut impl FnMut(f32) -> f32, hz: f64, fs: f64) -> f64 {
    let n = fs as usize / 2;
    let (mut acc_in, mut acc_out) = (0.0f64, 0.0f64);
    for i in 0..2 * n {
        let x = (core::f64::consts::TAU * hz * i as f64 / fs).sin() as f32;
        let y = f(x);
        if i >= n {
            acc_in += f64::from(x) * f64::from(x);
            acc_out += f64::from(y) * f64::from(y);
        }
    }
    10.0 * (acc_out / acc_in).log10()
}

/// |H(e^{jω})| in dB of `[b0, b1, b2] / [a0, a1, a2]`.
fn cookbook_db(b: [f64; 3], a: [f64; 3], hz: f64, fs: f64) -> f64 {
    let w = core::f64::consts::TAU * hz / fs;
    let eval = |c: [f64; 3]| {
        let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
        let im = -(c[1] * w.sin() + c[2] * (2.0 * w).sin());
        (re * re + im * im).sqrt()
    };
    20.0 * (eval(b) / eval(a)).log10()
}

#[test]
fn lowpass_and_peak_match_the_cookbook() {
    // Bristow-Johnson's formulas written out here, independently of the
    // section's own design code, and a tone measured through `tick`:
    // within 0.01 dB from 50 Hz to 16 kHz.
    let fs = 48_000.0;
    let (f0, q, db) = (1_200.0f64, 0.9f64, 6.0f64);
    let w0 = core::f64::consts::TAU * f0 / fs;
    let (cw, alpha) = (w0.cos(), w0.sin() / (2.0 * q));
    let lp = (
        [(1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0],
        [1.0 + alpha, -2.0 * cw, 1.0 - alpha],
    );
    let a = 10f64.powf(db / 40.0);
    let peak = (
        [1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a],
        [1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a],
    );
    let specs = [
        (FilterSpec::Lowpass { hz: f0 as f32, q: q as f32 }, lp),
        (FilterSpec::Peak { hz: f0 as f32, db: db as f32, q: q as f32 }, peak),
    ];
    for (spec, (b, a)) in specs {
        for hz in [50.0, 300.0, 1_000.0, 1_200.0, 2_500.0, 7_000.0, 16_000.0] {
            let mut f = Biquad::new(spec);
            f.prepare(fs as f32);
            let measured = tone_db(&mut |x| f.tick(x), hz, fs);
            let want = cookbook_db(b, a, hz, fs);
            assert!(
                (measured - want).abs() < 0.01,
                "{spec:?} at {hz} Hz: {measured:.4} dB, cookbook {want:.4}"
            );
        }
    }
}
