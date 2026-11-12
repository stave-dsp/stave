//! Tests for the Sympathetic Garden resonator engine.
//!
//! Run: cargo test -p stave-smear-vst3 --release --test garden -- --nocapture

use stave_core::Plugin;
use stave_smear_vst3::Smear;
use stave_test::harness::PluginHarness;

const SR: f32 = 48_000.0;
const BLOCK: usize = 256;

/// Render: `tone_secs` of a 220 Hz tone (A3, on the lattice), then silence
/// to `total_secs`. Returns (wet, dry).
fn render(total_secs: f32, tone_secs: f32, params: &[(&str, f32)]) -> (Vec<f32>, Vec<f32>) {
    let mut h = PluginHarness::<Smear>::new(SR, BLOCK);
    let table = h.plugin_mut().param_table();
    for (k, v) in params {
        table
            .by_name(k)
            .unwrap_or_else(|| panic!("param '{k}'"))
            .set_plain(f64::from(*v));
    }
    h.plugin_mut().activate(SR, BLOCK);
    let total = (total_secs * SR) as usize;
    let tone = (tone_secs * SR) as usize;
    let (mut wet, mut dry) = (Vec::new(), Vec::new());
    let mut n = 0;
    while n < total {
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        for i in 0..BLOCK {
            let s = if n + i < tone {
                (core::f32::consts::TAU * 220.0 * (n + i) as f32 / SR).sin() * 0.4
            } else {
                0.0
            };
            l[i] = s;
            r[i] = s;
            dry.push(s);
        }
        h.set_input(0, &l);
        h.set_input(1, &r);
        let b = h.process_block(&[]);
        for i in 0..BLOCK {
            wet.push(2.0 * b[0][i] - l[i]); // reconstruct wet at mix=0.5
        }
        n += BLOCK;
    }
    dry.truncate(wet.len());
    (wet, dry)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Spectral flatness (geometric mean / arithmetic mean of the power
/// spectrum). ~0 = tonal/pitched, ~1 = white noise.
fn flatness(x: &[f32]) -> f32 {
    use rustfft::{num_complex::Complex32, FftPlanner};
    let n = 16384.min(x.len());
    let off = x.len().saturating_sub(n);
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let mut buf: Vec<Complex32> = (0..n)
        .map(|i| {
            let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
            Complex32::new(x[off + i] * w, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let mut log_sum = 0.0f64;
    let mut lin_sum = 0.0f64;
    let mut cnt = 0u32;
    // Skip DC; look across the audible band.
    for c in buf.iter().take(n / 2).skip(2) {
        let p = (c.re * c.re + c.im * c.im) as f64 + 1e-12;
        log_sum += p.ln();
        lin_sum += p;
        cnt += 1;
    }
    let gm = (log_sum / cnt as f64).exp();
    let am = lin_sum / cnt as f64;
    (gm / am) as f32
}

#[test]
fn output_is_tonal_not_noise() {
    let (wet, _) = render(4.0, 4.0, &[]); // default patch, sustained tone
    let one = SR as usize;
    let fl = flatness(&wet[one..(3 * one)]);
    eprintln!("default flatness = {fl:.4} (0=tonal, 1=noise)");
    // A pitched resonator field is far from white noise.
    assert!(fl < 0.10, "output is noisy: flatness {fl:.3}");
}

#[test]
fn rings_out_then_decays() {
    // 0.5 s tone, then silence. The garden should keep ringing after the
    // input stops (sympathetic sustain), then decay.
    let (wet, _) = render(6.0, 0.5, &[]);
    let one = SR as usize;
    let during = rms(&wet[(one / 4)..(one / 2)]);
    let tail = rms(&wet[one..(one + one / 2)]); // ~0.5–1 s after note off
    let late = rms(&wet[(5 * one)..]); // ~5 s
    eprintln!("during={during:.4} tail={tail:.4} late={late:.4}");
    assert!(during > 0.05, "wet output too quiet: {during:.4}");
    assert!(tail > during * 0.05, "no sympathetic ring-out");
    assert!(late < during * 0.5 + 1e-3, "never decays");
}

#[test]
fn stable_at_extreme_settings() {
    // Crank everything that feeds back; must stay finite and bounded.
    let (wet, _) = render(
        6.0,
        3.0,
        &[
            ("seed", 1.0),
            ("smear", 1.0),
            ("bloom", 1.0),
            ("decay", 1.0),
            ("shimmer", 1.0),
            ("drive", 1.0),
            ("motion", 1.0),
        ],
    );
    let bad = wet
        .iter()
        .filter(|s| !s.is_finite() || s.abs() > 8.0)
        .count();
    eprintln!(
        "extreme: max={:.3} bad={bad}",
        wet.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    );
    assert_eq!(bad, 0, "extreme settings ran away / non-finite");
}

#[test]
fn responds_to_input_pitch() {
    // With seed up and the tone on the lattice (220 Hz = A3), the strongest
    // wet energy should sit at ~220 Hz, not elsewhere.
    use rustfft::{num_complex::Complex32, FftPlanner};
    let (wet, _) = render(3.0, 3.0, &[("smear", 0.15), ("bloom", 0.2)]);
    let one = SR as usize;
    let seg = &wet[one..(one + 16384)];
    let n = seg.len();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let mut buf: Vec<Complex32> = (0..n)
        .map(|i| {
            let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
            Complex32::new(seg[i] * w, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let bin_hz = SR / n as f32;
    let mut peak_bin = 0;
    let mut peak = 0.0f32;
    for (k, c) in buf.iter().enumerate().take(n / 2).skip(2) {
        let m = c.re * c.re + c.im * c.im;
        if m > peak {
            peak = m;
            peak_bin = k;
        }
    }
    let peak_hz = peak_bin as f32 * bin_hz;
    eprintln!("peak at {peak_hz:.1} Hz (expected ~220)");
    assert!(
        (peak_hz - 220.0).abs() < 15.0,
        "peak not at input pitch: {peak_hz:.1} Hz"
    );
}
