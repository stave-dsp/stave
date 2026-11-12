//! Tolex's OWN laws — the physics only this project can promise.
//! Each law restates its physics INDEPENDENTLY of the engine's
//! constants — same textbook, different pen — so a transcription
//! error in either place fails the build.
//!
//! * the_box_stiffens_the_spring — the closed back is an air spring:
//!   the response peak lands at fs·√(1 + Cms·k_box), k_box = ρc²Sd²/V,
//!   and opening the back returns the peak to free-air fs.
//! * the_amp_lets_the_cone_bloom — source impedance is REAL coupling:
//!   a no-feedback tube output (damping 1) blooms the resonance
//!   ≥ 6 dB over a solid-state grip (damping 0).
//! * excursion_compresses — Bl(x) droop + Cms(x) stiffening: drive
//!   is loudness-compensated, so linearly the fundamental would
//!   track the program exactly; the real motor compresses it and
//!   pays out H3. The behavior no IR can render.
//! * the_back_door_combs_at_the_mic — the open back's wrap path
//!   prints its geometry: pulling the mic back deepens the comb at
//!   the constructive band (~312 Hz for the default wrap), thins the
//!   deep bass, and leaves the diffraction-lowpassed top alone.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

// The physics, restated from the textbook (NOT from the engine):
// 12" position defaults the engine also uses — the law pins the
// derivation, not private constants.
const SD_12: f64 = 0.055; // m², 12" cone
const MMS_12: f64 = 0.035; // kg
const RHO: f64 = 1.204; // kg/m³
const C_AIR: f64 = 343.0; // m/s

fn render(set: &[(&'static str, f64)], input: &[f32]) -> Vec<f32> {
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
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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

fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / FS).sin() * amp)
        .collect()
}

/// Steady-state RMS gain (dB) of a rendered sine vs its input,
/// skipping the first half (resonance + ramp settling).
fn gain_db(set: &[(&'static str, f64)], freq: f32) -> f64 {
    let input = sine(freq, 0.1, 24_064);
    let out = render(set, &input);
    let rms = |w: &[f32]| {
        (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
    };
    20.0 * (rms(&out[12_032..]) / rms(&input[12_032..])).log10()
}

/// The frequency of maximum gain over a log grid — the measured
/// resonance peak. Grid step ~2.5%, so an 8% assertion is honest.
fn peak_hz(set: &[(&'static str, f64)], lo: f32, hi: f32) -> f64 {
    let n = 41;
    let mut best = (lo as f64, f64::NEG_INFINITY);
    for k in 0..n {
        let f = lo as f64 * (hi as f64 / lo as f64).powf(k as f64 / (n - 1) as f64);
        let g = gain_db(set, f as f32);
        if g > best.1 {
            best = (f, g);
        }
    }
    best.0
}

/// Harmonic level: project the settled tail onto sin/cos at `k·f0`
/// over an integer number of periods (f0 = 93.75 Hz = FS/512 keeps
/// every projection bin-exact).
fn harmonic_db(out: &[f32], f0: f32, k: usize) -> f64 {
    let seg = &out[out.len() - 23_040..]; // 45 exact periods of 512
    let w = 2.0 * std::f64::consts::PI * f0 as f64 * k as f64 / FS as f64;
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for (i, x) in seg.iter().enumerate() {
        s += f64::from(*x) * (w * i as f64).sin();
        c += f64::from(*x) * (w * i as f64).cos();
    }
    let mag = (s * s + c * c).sqrt() * 2.0 / seg.len() as f64;
    20.0 * mag.log10()
}

// The shared bench: 12" driver, fs 80, Qts 0.85, 60 L cab.
const BENCH: &[(&str, f64)] = &[
    ("size", 2.0),
    ("resonance", 80.0),
    ("q", 0.85),
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

/// The closed back is an air spring: the resonance peak lands where
/// ρc²Sd²/V says, and opening the back returns it to free-air fs.
#[test]
fn the_box_stiffens_the_spring() {
    let fs = 80.0f64;
    let cms = 1.0 / (MMS_12 * (2.0 * std::f64::consts::PI * fs).powi(2));
    let k_box = RHO * C_AIR * C_AIR * SD_12 * SD_12 / 0.060; // 60 L
    let fc_pred = fs * (1.0 + cms * k_box).sqrt();

    let closed = with(BENCH, &[("damping", 1.0), ("back", 0.0)]);
    let peak_closed = peak_hz(&closed, 60.0, 170.0);
    let err = (peak_closed / fc_pred - 1.0).abs();
    assert!(
        err < 0.08,
        "closed peak {peak_closed:.1} Hz vs predicted {fc_pred:.1} Hz"
    );

    let open = with(BENCH, &[("damping", 1.0), ("back", 1.0)]);
    let peak_open = peak_hz(&open, 55.0, 140.0);
    let err = (peak_open / fs - 1.0).abs();
    assert!(
        err < 0.08,
        "open peak {peak_open:.1} Hz vs free-air fs {fs:.1} Hz"
    );
}

/// Source impedance is real coupling: tube damping blooms the
/// resonance ≥ 6 dB over solid-state, measured against the same
/// 1 kHz passband reference.
#[test]
fn the_amp_lets_the_cone_bloom() {
    let bloom = |damping: f64| {
        let set = with(BENCH, &[("damping", damping), ("back", 0.0)]);
        let fc = peak_hz(&set, 60.0, 170.0);
        gain_db(&set, fc as f32) - gain_db(&set, 1_000.0)
    };
    let tube = bloom(1.0);
    let solid = bloom(0.0);
    assert!(
        tube - solid >= 6.0,
        "resonance bloom: tube {tube:.1} dB vs solid-state {solid:.1} dB — coupling too weak"
    );
}

/// Bl(x) droop + Cms(x) stiffening at the resonance. Drive is
/// loudness-compensated BY CONSTRUCTION (the motor normalizes its
/// passband by the swing), so in the linear world this measurement
/// is drive-invariant: +36 dB of drive on a 14 dB hotter program
/// should return exactly +14 dB of fundamental. The nonlinear motor
/// gives some of it back as compression — and pays out H3.
#[test]
fn excursion_compresses() {
    let f0 = 93.75f32; // FS/512 — bin-exact projections
    let set_lo = with(BENCH, &[("damping", 0.75), ("back", 0.0), ("drive", -12.0)]);
    let set_hi = with(BENCH, &[("damping", 0.75), ("back", 0.0), ("drive", 24.0)]);
    let out_lo = render(&set_lo, &sine(f0, 0.1, 48_128));
    let out_hi = render(&set_hi, &sine(f0, 0.5, 48_128));

    let fund_lo = harmonic_db(&out_lo, f0, 1);
    let fund_hi = harmonic_db(&out_hi, f0, 1);
    let grew = fund_hi - fund_lo;
    let linear = 20.0 * (0.5f64 / 0.1).log10(); // ≈ 14 dB
    assert!(
        grew < linear - 1.0,
        "fundamental grew {grew:.1} dB vs {linear:.1} dB linear — no excursion compression"
    );

    let h3_lo = harmonic_db(&out_lo, f0, 3) - fund_lo;
    let h3_hi = harmonic_db(&out_hi, f0, 3) - fund_hi;
    assert!(
        h3_lo < -60.0,
        "H3 at low drive is {h3_lo:.1} dBc — should be clean"
    );
    assert!(
        h3_hi > -35.0,
        "H3 at high drive is {h3_hi:.1} dBc — motor isn't working"
    );
}

/// The open back's wrap path prints its geometry at the mic:
/// pulling back 3 → 40 cm deepens the comb's constructive band
/// (~312 Hz for the default wrap), thins the deep bass, and leaves
/// the diffraction-lowpassed top alone. Same motor both renders —
/// alignment cancels; only the rear path can move these numbers.
#[test]
fn the_back_door_combs_at_the_mic() {
    // Difference-in-differences: mic distance now carries honest
    // PROXIMITY (a common-mode LF effect), so the rear path's
    // geometry is isolated by subtracting the CLOSED cab's
    // distance response from the OPEN cab's at each frequency.
    let d = |f: f32| {
        let open_near = with(BENCH, &[("damping", 0.75), ("back", 1.0), ("mic", 3.0)]);
        let open_far = with(BENCH, &[("damping", 0.75), ("back", 1.0), ("mic", 40.0)]);
        let closed_near = with(BENCH, &[("damping", 0.75), ("back", 0.0), ("mic", 3.0)]);
        let closed_far = with(BENCH, &[("damping", 0.75), ("back", 0.0), ("mic", 40.0)]);
        (gain_db(&open_far, f) - gain_db(&open_near, f))
            - (gain_db(&closed_far, f) - gain_db(&closed_near, f))
    };

    let bump = d(312.0);
    assert!(
        bump >= 1.5,
        "constructive band moved {bump:.2} dB — comb not printing"
    );
    let thin = d(55.0);
    assert!(
        thin <= -1.5,
        "deep bass moved {thin:.2} dB — rear cancellation missing"
    );
    let top = d(4_000.0);
    assert!(
        top.abs() <= 1.0,
        "4 kHz moved {top:.2} dB — diffraction lowpass leaking"
    );
}
