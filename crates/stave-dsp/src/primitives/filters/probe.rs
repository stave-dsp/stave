//! Test measurements shared by the nonlinear filters' and the Hilbert
//! transformer's tests: small-signal response with phase, harmonics,
//! self-oscillation, aliasing — all by coherent correlation over whole
//! periods, so no window and no FFT stand between a tone and its number.

use crate::kernel::node::Stage;
use crate::units::{db, Db};
use std::f64::consts::TAU;

/// What every nonlinear filter offers the measurements.
pub(crate) trait Filter: Stage {
    fn f64_tick(&mut self, x: f64) -> f64;
    fn drive(&mut self, drive: Db);
    fn cutoff(&mut self, hz: f32);
    fn capped(&self) -> u64;
}

macro_rules! filter {
    ($($t:ty),*) => {$(
        impl Filter for $t {
            fn f64_tick(&mut self, x: f64) -> f64 { self.tick_f64(x) }
            fn drive(&mut self, drive: Db) { self.set_drive(drive) }
            fn cutoff(&mut self, hz: f32) { self.set_cutoff(crate::units::Hz::new(hz)) }
            fn capped(&self) -> u64 { self.stats().capped }
        }
    )*};
}
filter!(
    super::ladder::Huovilainen,
    super::ladder::Stinchcombe,
    super::sallen_key::SallenKey,
    super::korg35::Korg35
);

/// `(re, im)` of a tone's component at `hz` in `y` (sampled at `fs`),
/// as a phasor of the tone's peak against `cos`: the least-squares fit of
/// `re·cos − im·sin`, exact for a pure tone over any length (no leakage
/// from a fractional period).
pub(crate) fn phasor(y: &[f64], fs: f64, hz: f64) -> (f64, f64) {
    let (mut yc, mut ys, mut cc, mut ss, mut cs) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, v) in y.iter().enumerate() {
        let (s, c) = (TAU * hz * i as f64 / fs).sin_cos();
        yc += v * c;
        ys += v * s;
        cc += c * c;
        ss += s * s;
        cs += c * s;
    }
    let det = cc * ss - cs * cs;
    let a = (yc * ss - ys * cs) / det;
    let b = (ys * cc - yc * cs) / det;
    (a, -b)
}

/// The complex gain at `hz` of a sine of peak `amp`: settle, then
/// correlate. The phase is relative to the input `sin` (so a cosine
/// reference gives −90°: the input is `amp·sin`).
pub(crate) fn response<F: Filter>(f: &mut F, fs: f64, hz: f64, amp: f64) -> (f64, f64) {
    f.prepare(fs as f32);
    let settle = ((0.3f64).max(40.0 / hz) * fs) as usize;
    let measure = (0.2 * fs) as usize;
    let mut y = Vec::with_capacity(measure);
    for i in 0..settle + measure {
        let x = amp * (TAU * hz * i as f64 / fs).sin();
        let out = f.f64_tick(x);
        if i >= settle {
            y.push(out);
        }
    }
    assert_eq!(f.capped(), 0, "Newton capped");
    // Reference: the input's own phasor over the same samples.
    let x: Vec<f64> = (settle..settle + measure)
        .map(|i| amp * (TAU * hz * i as f64 / fs).sin())
        .collect();
    let (yr, yi) = phasor(&y, fs, hz);
    let (xr, xi) = phasor(&x, fs, hz);
    let m = xr * xr + xi * xi;
    ((yr * xr + yi * xi) / m, (yi * xr - yr * xi) / m)
}

/// `20·log10 |z|` and the angle in degrees.
pub(crate) fn db_deg(z: (f64, f64)) -> (f64, f64) {
    (
        10.0 * (z.0 * z.0 + z.1 * z.1).log10(),
        z.1.atan2(z.0).to_degrees(),
    )
}

/// Harmonics 1..=`count` (peak amplitudes) of a settled sine of peak
/// `amp` at `hz`.
pub(crate) fn harmonics<F: Filter>(
    f: &mut F,
    fs: f64,
    hz: f64,
    amp: f64,
    count: usize,
) -> Vec<f64> {
    f.prepare(fs as f32);
    let settle = (0.3 * fs) as usize;
    let measure = (0.2 * fs) as usize;
    let mut y = Vec::with_capacity(measure);
    for i in 0..settle + measure {
        let out = f.f64_tick(amp * (TAU * hz * i as f64 / fs).sin());
        if i >= settle {
            y.push(out);
        }
    }
    assert_eq!(f.capped(), 0, "Newton capped");
    (1..=count)
        .map(|h| {
            let (re, im) = phasor(&y, fs, hz * h as f64);
            (re * re + im * im).sqrt()
        })
        .collect()
}

/// Kick the filter with one sample of `kick` and let it ring for
/// `seconds`: the RMS of the last tenth (a decaying ring leaves nothing,
/// a self-oscillation its limit cycle), and the pitch from the zero
/// crossings of the second half.
pub(crate) fn ring<F: Filter>(f: &mut F, fs: f64, kick: f64, seconds: f64) -> (f64, f64) {
    f.prepare(fs as f32);
    let n = (seconds * fs) as usize;
    let y: Vec<f64> = (0..n)
        .map(|i| f.f64_tick(if i == 0 { kick } else { 0.0 }))
        .collect();
    assert_eq!(f.capped(), 0, "Newton capped");
    let rms = |s: &[f64]| (s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt();
    let growth = rms(&y[n - n / 10..]);
    let half = &y[n / 2..];
    let mut crossings = Vec::new();
    for i in 1..half.len() {
        if half[i - 1] < 0.0 && half[i] >= 0.0 {
            crossings.push(i as f64 - 1.0 + half[i - 1] / (half[i - 1] - half[i]));
        }
    }
    let pitch = if crossings.len() > 2 {
        fs * (crossings.len() - 1) as f64 / (crossings[crossings.len() - 1] - crossings[0])
    } else {
        0.0
    };
    (growth, pitch)
}

// ---------------------------------------------------------------- suites

pub(crate) type C = (f64, f64);
pub(crate) fn mul(a: C, b: C) -> C {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}
pub(crate) fn div(a: C, b: C) -> C {
    let m = b.0 * b.0 + b.1 * b.1;
    ((a.0 * b.0 + a.1 * b.1) / m, (a.1 * b.0 - a.0 * b.1) / m)
}
/// A real polynomial (highest power first) at `s`.
pub(crate) fn poly(c: &[f64], s: C) -> C {
    c.iter().fold((0.0, 0.0), |acc, &k| {
        let m = mul(acc, s);
        (m.0 + k, m.1)
    })
}

/// The frequency the bilinear transform prewarped at `fc` maps `f` to,
/// relative to `fc`: where the analog prototype is to be read.
pub(crate) fn warped(f: f64, fc: f64, fs: f64) -> f64 {
    (std::f64::consts::PI * f / fs).tan() / (std::f64::consts::PI * fc / fs).tan()
}

/// Measured against `want` at `f` (cutoff `fc`, rate `fs`): the worst
/// magnitude (dB) and phase (degrees) error.
pub(crate) fn check<F: Filter>(
    name: &str,
    f: &mut F,
    fs: f64,
    fc: f64,
    freqs: &[f64],
    want: impl Fn(f64) -> C,
) -> (f64, f64) {
    let (mut worst_db, mut worst_deg) = (0.0f64, 0.0f64);
    for &hz in freqs {
        let (m_db, m_deg) = db_deg(response(f, fs, hz, 1e-4));
        let (w_db, w_deg) = db_deg(want(warped(hz, fc, fs)));
        let d_deg = (m_deg - w_deg + 540.0).rem_euclid(360.0) - 180.0;
        worst_db = worst_db.max((m_db - w_db).abs());
        worst_deg = worst_deg.max(d_deg.abs());
        println!("{name} fc {fc} @ {hz:.1} Hz, fs {fs}: {m_db:.4} dB {m_deg:.3}°, want {w_db:.4} dB {w_deg:.3}°");
    }
    (worst_db, worst_deg)
}

pub(crate) fn cutoff_sweep<F: Filter>(
    name: &str,
    make: impl Fn(f64, f32) -> F,
    want: impl Fn(f64, f64) -> C,
) {
    let fs = 48_000.0;
    let (mut worst_db, mut worst_deg) = (0.0f64, 0.0f64);
    for fc in [50.0, 200.0, 1_000.0, 5_000.0, 16_000.0] {
        for r in [0.0f32, 0.5, 0.8] {
            let mut f = make(fc, r);
            let freqs: Vec<f64> = [0.5, 1.0, 1.5]
                .iter()
                .map(|m| m * fc)
                .filter(|&h| h < 0.45 * fs)
                .collect();
            let (d, a) = check(name, &mut f, fs, fc, &freqs, |w| want(w, f64::from(r)));
            worst_db = worst_db.max(d);
            worst_deg = worst_deg.max(a);
        }
    }
    println!("{name}: worst {worst_db:.2e} dB, {worst_deg:.2e}°");
    assert!(
        worst_db < 0.01 && worst_deg < 0.1,
        "{name}: {worst_db} dB, {worst_deg}°"
    );
}

/// Below resonance 1 a kick dies away; above it the filter sings, the
/// saturation holding its level. Just past the onset (1.01) it sings at
/// its cutoff within `pitch_budget` (relative); further up (1.3) the
/// saturation flattens the pitch, printed for the docs.
pub(crate) fn self_oscillates<F: Filter>(name: &str, make: impl Fn(f32) -> F, pitch_budget: f64) {
    let (fs, fc) = (48_000.0, 1_000.0);
    let (below, _) = ring(&mut make(0.99), fs, 0.01, 1.0);
    let (above, pitch) = ring(&mut make(1.01), fs, 0.01, 2.0);
    let (hot, hot_pitch) = ring(&mut make(1.3), fs, 0.01, 1.0);
    let error = (pitch / fc - 1.0).abs();
    println!(
        "{name}: left ringing {below:.1e} at 0.99; sings at {above:.3} RMS, {pitch:.2} Hz at 1.01; \
         {hot:.3} RMS, {hot_pitch:.2} Hz at 1.3"
    );
    assert!(below < 1e-6, "{name}: rings on at 0.99 ({below})");
    assert!(above > 1e-3, "{name}: no oscillation at 1.01 ({above})");
    assert!(error < pitch_budget, "{name}: pitch {pitch} Hz");
    assert!(
        hot_pitch < pitch && hot_pitch > 0.9 * fc,
        "{name}: pitch {hot_pitch} Hz at 1.3"
    );
}

/// R1's drive characterisation: harmonics against level at a quarter of
/// the cutoff. The stages are odd, so even harmonics stay at the
/// solver's floor; the third rises 2 dB per dB while the stages are
/// nearly linear (a cubic), then bends; and drive is input gain with
/// the output compensated, so +6 dB of drive is +6 dB of level.
pub(crate) fn drive_follows_the_cubic_law<F: Filter>(name: &str, make: impl Fn() -> F) {
    let fs = 48_000.0;
    let tone = 250.0;
    let mut h3 = Vec::new();
    for level in [-40.0, -30.0, -20.0, -10.0, 0.0, 6.0, 12.0] {
        let amp = 10f64.powf(level / 20.0);
        let h = harmonics(&mut make(), fs, tone, amp, 3);
        let dbc = |x: f64| 20.0 * (x / h[0]).max(1e-12).log10();
        println!(
            "{name} {level:+} dB: H1 {:.4}, H2 {:.1} dBc, H3 {:.1} dBc",
            h[0] / amp,
            dbc(h[1]),
            dbc(h[2])
        );
        assert!(
            dbc(h[1]) < -120.0,
            "{name}: H2 {} dBc at {level}",
            dbc(h[1])
        );
        h3.push(dbc(h[2]));
    }
    let slope = (h3[1] - h3[0]) / 10.0;
    assert!((slope - 2.0).abs() < 0.05, "{name}: H3 slope {slope} dB/dB");
    assert!(
        h3.windows(2).take(4).all(|w| w[1] > w[0]),
        "{name}: H3 {h3:?}"
    );
    // Drive +6 dB at −16 dB is level −10 dB at drive 0.
    let mut driven = make();
    driven.drive(db(6.0));
    let h = harmonics(&mut driven, fs, tone, 10f64.powf(-16.0 / 20.0), 3);
    let same = 20.0 * (h[2] / h[0]).log10();
    assert!((same - h3[3]).abs() < 1e-6, "{name}: {same} vs {}", h3[3]);
}

/// The same filter at 44.1–192 kHz: small-signal response within 0.2 dB
/// of the analog prototype at half, one and twice the cutoff (exact at
/// the cutoff; above it the bilinear transform's warping at 44.1 kHz),
/// the third harmonic of a hot tone within 0.2 dB, the pitch just past
/// the onset within 0.5 %.
pub(crate) fn rate_invariant<F: Filter>(
    name: &str,
    make: impl Fn(f32) -> F,
    want: impl Fn(f64) -> C,
) {
    let fc = 1_000.0;
    let mut h3s = Vec::new();
    for fs in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for f in [500.0, 1_000.0, 2_000.0] {
            let (m, _) = db_deg(response(&mut make(0.5), fs, f, 1e-4));
            let (w, _) = db_deg(want(f / fc));
            assert!(
                (m - w).abs() < 0.2,
                "{name} at {fs}: {m} dB vs analog {w} at {f} Hz"
            );
        }
        let h = harmonics(&mut make(0.5), fs, 250.0, 1.0, 3);
        h3s.push(20.0 * (h[2] / h[0]).log10());
        let (_, pitch) = ring(&mut make(1.01), fs, 0.01, 2.0);
        assert!(
            (pitch / fc - 1.0).abs() < 0.005,
            "{name} at {fs}: pitch {pitch}"
        );
    }
    let spread =
        h3s.iter().copied().fold(f64::MIN, f64::max) - h3s.iter().copied().fold(f64::MAX, f64::min);
    println!("{name}: H3 at 0 dB {h3s:?} across rates");
    assert!(spread < 0.2, "{name}: H3 spread {spread} dB");
}

/// Stability under audio-rate modulation (after McClellan 2026's result
/// for time-varying zero-delay-feedback filters): the cutoff swept
/// exponentially between 50 Hz and 15 kHz at 1.7 kHz, resonance past the
/// onset and below it, +12 dB of drive, a full-scale 110 Hz saw in, at
/// 48 kHz. The output stays finite and bounded (the peak is printed),
/// and Newton converges on every sample.
pub(crate) fn bounded_under_audio_rate_modulation<F: Filter>(name: &str, make: impl Fn(f32) -> F) {
    let fs = 48_000.0;
    for resonance in [0.9f32, 1.4] {
        let mut f = make(resonance);
        f.drive(db(12.0));
        f.prepare(fs as f32);
        let mut peak = 0.0f64;
        for i in 0..48_000 {
            let t = i as f64 / fs;
            let sweep = 0.5 + 0.5 * (TAU * 1_700.0 * t).sin();
            f.cutoff((50.0 * 300f64.powf(sweep)) as f32);
            let saw = 2.0 * (110.0 * t).fract() - 1.0;
            let y = f.f64_tick(saw);
            assert!(y.is_finite(), "{name}: not finite at sample {i}");
            peak = peak.max(y.abs());
        }
        println!(
            "{name}: resonance {resonance}, cutoff swept at 1.7 kHz: peak {peak:.3}, {} capped",
            f.capped()
        );
        assert!(peak < 2.0, "{name}: peak {peak} at resonance {resonance}");
        assert_eq!(f.capped(), 0, "{name}: Newton capped under modulation");
    }
}
