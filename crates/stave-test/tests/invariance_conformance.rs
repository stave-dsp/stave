//! Conformance: invariance properties — the tests that catch whole bug
//! classes rather than single regressions.
//!
//! - **Sample-rate invariance**: a knob labelled in milliseconds or hertz
//!   must deliver that value at every rate AND at the rate the component
//!   actually runs (the rate contract). This is the class of bug where a
//!   component prepared at the host rate ticks inside an oversampler —
//!   every time constant silently scales by the factor.
//! - **Block-size invariance**: re-applying identical parameters at any
//!   block boundary must be a bit-exact no-op (catches per-block filter
//!   resets / zipper classes).

use stave_anvil_vst3::RcCoupledTriodeCascade;
use stave_dsp::{
    Bias, EnvelopeFollower, EnvelopeTimes, Oversampler2xFir, Oversampler2xFirInner, SmoothSat,
    Triode,
};
use stave_measure::dynamics::time_to_fraction;
use stave_measure::tone_test::ToneTest;

/// The cathode self-bias `time_ms` sets the PHYSICAL cathode RC. Measure
/// the observed bias-shift decay in silence, at three rates.
///
/// Accuracy notes — two distinct claims, with different tolerances:
///
/// 1. **Loop physics.** The RC sits inside feedback (bias shift → less
///    plate current → lower cathode target), so the observed recovery is
///    `τ_RC / (1 + gm·Rk·depth)` — measured 30.9 ms for a 60 ms RC at the
///    12AX7 default operating point (loop factor ≈ 2, matching
///    gm ≈ 1.6 mA/V · Rk ≈ 0.9 kΩ · depth 1). The limit brackets that
///    physical factor, NOT the raw label.
/// 2. **Rate invariance** (the actual bug class this guards): the
///    observed τ must agree across rates within 10%. The historical bug —
///    preparing at the host rate while ticking inside a 2×/4×
///    oversampler — was a clean 2×/4× error, far outside this band.
#[test]
fn cathode_tau_is_rate_invariant() {
    let tau_ms = 60.0f32;
    let mut measured = vec![];
    for sample_rate in [44_100.0f64, 48_000.0, 96_000.0] {
        let mut t = Triode::new();
        t.prepare(sample_rate as f32);
        t.set_grid_drive(37.11);
        let bypass_farads = f64::from(tau_ms) * 1e-3 / t.cathode_ohms();
        t.set_bias(Bias::Cathode { bypass_farads });
        // Drive into grid conduction to build a clear bias shift, long
        // enough to settle (≥10 τ).
        let drive_samples = (sample_rate * 0.75) as usize;
        for i in 0..drive_samples {
            let s =
                0.8 * (2.0 * core::f64::consts::PI * 200.0 * i as f64 / sample_rate).sin() as f32;
            let _ = t.process_sample(s);
        }
        // Silence: record the bias shift decaying toward 0.
        let mut shift = Vec::with_capacity((sample_rate * 0.5) as usize);
        for _ in 0..(sample_rate * 0.5) as usize {
            let _ = t.process_sample(0.0);
            shift.push(t.bias_shift() as f32);
        }
        let k = time_to_fraction(&shift, (-1.0f32).exp())
            .expect("bias shift never decayed — cathode dynamics broken");
        let tau_meas = k as f64 / sample_rate * 1000.0;
        println!(
            "sample_rate {sample_rate:>6.0}: cathode τ set {tau_ms} ms, measured {tau_meas:.1} ms"
        );
        measured.push(tau_meas);
    }
    for &m in &measured {
        // Observed ≈ RC / loop-factor; bracket [τ/3, τ/1.4] around the
        // measured 30.9 ms so a plain coefficient bug (observed == τ, or
        // == τ/2 from a rate error on top of the loop) still trips it.
        assert!(
            m > tau_ms as f64 / 3.0 && m < tau_ms as f64 / 1.4,
            "cathode observed τ outside the loop-physics window: {m:.1} ms for RC {tau_ms} ms"
        );
    }
    let (lo, hi) = (
        measured.iter().cloned().fold(f64::INFINITY, f64::min),
        measured.iter().cloned().fold(0.0f64, f64::max),
    );
    assert!(
        hi / lo < 1.10,
        "cathode τ varies with sample rate: {measured:?}"
    );
}

/// Memory's envelope presets are labelled in ms; the TubeBias release is
/// exactly one pole of the labelled τ, so the limit is tight (±10%).
#[test]
fn memory_time_is_rate_invariant() {
    let tau_ms = 120.0f32;
    let mut measured = vec![];
    for sample_rate in [44_100.0f64, 48_000.0, 96_000.0] {
        let mut m = EnvelopeFollower::new(EnvelopeTimes {
            attack_ms: 0.35 * tau_ms,
            release_ms: tau_ms,
            slow_release_ms: None,
        });
        m.prepare(sample_rate as f32);
        for _ in 0..(sample_rate * 0.9) as usize {
            let _ = m.tick(0.7);
        }
        let mut state = Vec::with_capacity(sample_rate as usize);
        for _ in 0..sample_rate as usize {
            let _ = m.tick(0.0);
            state.push(m.state());
        }
        let k = time_to_fraction(&state, (-1.0f32).exp()).expect("memory never released");
        let tau_meas = k as f64 / sample_rate * 1000.0;
        println!(
            "sample_rate {sample_rate:>6.0}: memory τ set {tau_ms} ms, measured {tau_meas:.1} ms"
        );
        measured.push(tau_meas);
        assert!(
            tau_meas > tau_ms as f64 * 0.9 && tau_meas < tau_ms as f64 * 1.1,
            "memory τ off: {tau_meas:.1} ms vs {tau_ms} ms at sample_rate {sample_rate}"
        );
    }
    let (lo, hi) = (
        measured.iter().cloned().fold(f64::INFINITY, f64::min),
        measured.iter().cloned().fold(0.0f64, f64::max),
    );
    assert!(
        hi / lo < 1.05,
        "memory τ varies with sample rate: {measured:?}"
    );
}

/// The preamp's coupling knob is labelled in Hz: prepared at the rate it
/// runs, the coupling high-pass must put its −3 dB point AT that
/// frequency, independent of the rate. (This is the knob that was
/// silently 4× off when the cascade was prepared at the host rate but
/// run inside the 4× oversampler.)
#[test]
fn preamp_coupling_cutoff_is_rate_invariant() {
    let coupling_hz = 41.0f32; // sits exactly on a bin at both rates below
    for (sample_rate, n) in [(48_000.0f64, 16_384usize), (96_000.0, 32_768)] {
        // Bin of ~41 Hz and of a passband reference ~410 Hz.
        let m_fc = (coupling_hz as f64 * n as f64 / sample_rate).round() as usize;
        let m_ref = m_fc * 10;
        let probe = |m: usize| -> f64 {
            let mut p = RcCoupledTriodeCascade::new();
            p.set_stage_count(1);
            p.prepare(sample_rate as f32); // prepared AT the rate it runs (no OS here)
            p.set_coupling_hz(coupling_hz);
            p.set_drive_db(0.0); // minimal grid swing → ~linear stage
            p.set_blocking_depth(0.0);
            // Small amplitude keeps the triode in its linear region so the
            // response measurement isn't polluted by harmonics.
            ToneTest {
                bin: m,
                window: n,
                sample_rate,
                amplitude: 0.05,
                warmup_windows: 1,
            }
            .gain_db(|x: f32| p.process_sample(x))
        };
        let rel = probe(m_fc) - probe(m_ref);
        println!("sample_rate {sample_rate:>6.0}: coupling {coupling_hz} Hz sits {rel:.2} dB below passband (expect ≈ −3)");
        assert!(
            rel > -4.5 && rel < -1.8,
            "coupling cutoff not at its label at sample_rate {sample_rate}: {rel:.2} dB"
        );
    }
}

/// Re-applying the same parameter values at any block cadence must be a
/// bit-exact no-op. (The regression class: per-block `set_*` calls that
/// reset filter state — a discontinuity every buffer, audible as zipper.)
#[test]
fn smoothsat_chain_is_block_size_invariant() {
    let sample_rate = 48_000.0f64;
    let n = 8192usize;
    let render = |block: usize| -> Vec<f32> {
        let mut s = SmoothSat::new();
        s.prepare(sample_rate as f32);
        let mut outer = Oversampler2xFir::new();
        outer.prepare(sample_rate as f32);
        let mut inner = Oversampler2xFirInner::new();
        inner.prepare(sample_rate as f32 * 2.0);
        let apply = |s: &mut SmoothSat| {
            s.set_drive(1.6);
            s.set_even(0.15);
            s.set_odd(0.10);
            s.set_smoothness(0.7);
            s.set_hf_smoothing(6.0);
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if i % block == 0 {
                apply(&mut s);
            }
            let x = (0.7 * (2.0 * core::f64::consts::PI * 171.0 * i as f64 / n as f64).sin()
                + 0.2 * (2.0 * core::f64::consts::PI * 1195.0 * i as f64 / n as f64).sin())
                as f32;
            let pre = s.process_pre(x);
            let shaped = outer.process_one(pre, |a| inner.process_one(a, |b| s.shape(b)));
            out.push(s.process_post(shaped));
        }
        out
    };
    let per_sample = render(1);
    let per_64 = render(64);
    let per_4096 = render(4096);
    for i in 0..n {
        assert!(
            per_sample[i] == per_64[i] && per_sample[i] == per_4096[i],
            "block-size dependent output at sample {i}: {} / {} / {}",
            per_sample[i],
            per_64[i],
            per_4096[i]
        );
    }
}
