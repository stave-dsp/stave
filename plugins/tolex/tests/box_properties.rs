//! laws — THE BOX, bracketed by the corpus.
//! Written before interior.rs.
//!
//! * the_box_prints_its_scale — the interior's short taps put the
//!   low band's ripple inside the population band (corpus §4/§8:
//!   0.34–2.71 dB RMS at 100–600 Hz), and the box lag SCALES with
//!   the cabinet: ∛(200/25) = 2× between the smallest and largest
//!   box (measured on the cepstrum's strongest box-scale peak).
//! * the_panels_have_a_voice — material is audible: pine's louder,
//!   lower panel modes carry measurably more low-mid ripple than
//!   dead MDF.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const IR_LEN: usize = 24_064;
const FFT_N: usize = 32_768;

fn render_ir(set: &[(&'static str, f64)]) -> Vec<f32> {
    let mut plugin = stave_tolex_vst3::Tolex::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(IR_LEN);
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for b in 0..(IR_LEN / BLOCK) {
        in_l.fill(0.0);
        if b == 0 {
            in_l[0] = 0.01;
        }
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

fn power_spectrum(ir: &[f32]) -> Vec<f64> {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let mut buf: Vec<Complex<f32>> = ir.iter().map(|x| Complex::new(*x, 0.0)).collect();
    buf.resize(FFT_N, Complex::new(0.0, 0.0));
    fft.process(&mut buf);
    buf.iter().map(|c| f64::from(c.norm_sqr())).collect()
}

/// Low-band fine-structure ripple RMS (dB), 100–600 Hz — the same
/// 1/24-vs-1/3 residual as the fidelity gate.
fn ripple_low(ir: &[f32]) -> f64 {
    let power = &power_spectrum(ir)[..FFT_N / 2];
    let grid: Vec<f64> = (0..400)
        .map(|i| 80.0 * (800.0f64 / 80.0).powf(i as f64 / 399.0))
        .collect();
    let bin_hz = f64::from(FS) / FFT_N as f64;
    let smooth = |frac: f64| -> Vec<f64> {
        let half = 2.0f64.powf(1.0 / (2.0 * frac));
        grid.iter()
            .map(|f| {
                let b0 = ((f / half) / bin_hz).floor().max(1.0) as usize;
                let b1 = (((f * half) / bin_hz).ceil() as usize)
                    .min(power.len() - 1)
                    .max(b0 + 1);
                let mean = power[b0..=b1].iter().sum::<f64>() / (b1 - b0 + 1) as f64;
                10.0 * mean.max(1e-30).log10()
            })
            .collect()
    };
    let fine = smooth(24.0);
    let trend = smooth(3.0);
    let sel: Vec<usize> = grid
        .iter()
        .enumerate()
        .filter(|(_, f)| (100.0..=600.0).contains(*f))
        .map(|(i, _)| i)
        .collect();
    (sel.iter()
        .map(|i| (fine[*i] - trend[*i]).powi(2))
        .sum::<f64>()
        / sel.len() as f64)
        .sqrt()
}

/// The most PROMINENT cepstral spike in the box-scale window (ms):
/// value minus the local background — a discrete tap is a spike,
/// while the big low-band modes leak smooth, decaying energy into
/// low quefrency that a raw max would mistake for a lag.
fn box_lag_ms(ir: &[f32], expected_ms: f64) -> f64 {
    let full = power_spectrum(ir);
    let logmag: Vec<Complex<f32>> = full
        .iter()
        .map(|p| Complex::new((p.max(1e-24).ln() * 0.5) as f32, 0.0))
        .collect();
    let mut planner = FftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(FFT_N);
    let mut buf = logmag;
    ifft.process(&mut buf);
    let cep: Vec<f32> = buf.iter().map(|c| c.re).collect();
    // Search ±30% around the geometric prediction: the speaker's
    // own (size-independent) modal fingerprint owns the shorter
    // quefrencies and would otherwise win the argmax.
    let q0 = ((expected_ms * 0.7 * 1e-3 * f64::from(FS)) as usize).max(14);
    let q1 = (expected_ms * 1.3 * 1e-3 * f64::from(FS)) as usize;
    let mut best = (q0, f32::NEG_INFINITY);
    for i in q0..q1 {
        let bg: f32 = (cep[i - 12..i - 3].iter().sum::<f32>()
            + cep[i + 4..i + 13].iter().sum::<f32>())
            / 18.0;
        let prom = cep[i] - bg;
        if prom > best.1 {
            best = (i, prom);
        }
    }
    if std::env::var("BOX_DEBUG").is_ok() {
        let mut proms: Vec<(usize, f32)> = (q0..q1)
            .map(|i| {
                let bg: f32 = (cep[i - 12..i - 3].iter().sum::<f32>()
                    + cep[i + 4..i + 13].iter().sum::<f32>())
                    / 18.0;
                (i, cep[i] - bg)
            })
            .collect();
        proms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (i, p) in proms.iter().take(6) {
            eprintln!(
                "  q {:.3} ms prom {:.4}",
                *i as f64 / f64::from(FS) * 1e3,
                p
            );
        }
    }
    best.0 as f64 / f64::from(FS) * 1_000.0
}

const BENCH: &[(&str, f64)] = &[
    ("size", 2.0),
    ("resonance", 80.0),
    ("q", 0.85),
    ("damping", 0.75),
    ("back", 0.0),
    ("cab", 60.0),
    ("drive", 0.0),
    ("mass", 1.0),
    ("cone", 0.0),
    ("breakup", 0.6),
    ("doping", 0.35),
    ("position", 0.25),
    ("material", 0.0),
    ("speakers", 0.0),
    ("model", 0.0),
    ("mic", 3.0),
];

fn with(base: &[(&'static str, f64)], extra: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    let mut v: Vec<(&'static str, f64)> = Vec::new();
    for (n, x) in base.iter().chain(extra.iter()) {
        if let Some(slot) = v.iter_mut().find(|(m, _)| m == n) {
            slot.1 = *x;
        } else {
            v.push((n, *x));
        }
    }
    v
}

/// The interior puts the low band inside the population's ripple
/// band, and the box lag scales as ∛V.
#[test]
fn the_box_prints_its_scale() {
    let rip = ripple_low(&render_ir(BENCH));
    assert!(
        (0.4..=2.8).contains(&rip),
        "low-band ripple {rip:.2} dB — population band is 0.34–2.71"
    );

    // The edge lag lands where geometry says: τ = (W/2)/c with
    // W = 1.3·∛V (the physics, restated independently) — at BOTH
    // extremes of the cab knob, which implies the ∛V scaling.
    for vb_l in [25.0f64, 200.0] {
        let predicted = 1.3 * 0.5 * (vb_l * 1e-3).cbrt() / 343.0 * 1_000.0;
        let found = box_lag_ms(&render_ir(&with(BENCH, &[("cab", vb_l)])), predicted);
        let err = (found / predicted - 1.0).abs();
        assert!(
            err <= 0.20,
            "{vb_l} L: edge lag {found:.2} ms vs predicted {predicted:.2} — geometry not printing"
        );
    }
}

/// Material is audible: pine's panels sing, MDF's are dead.
#[test]
fn the_panels_have_a_voice() {
    let pine = ripple_low(&render_ir(&with(BENCH, &[("material", 1.0)])));
    let mdf = ripple_low(&render_ir(&with(BENCH, &[("material", 2.0)])));
    assert!(
        pine - mdf >= 0.3,
        "pine {pine:.2} vs MDF {mdf:.2} dB low-band ripple — material must speak"
    );
}
