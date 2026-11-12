//! Conformance: high-frequency response accuracy ("cramping") and
//! anti-aliasing behaviour near Nyquist — the region where digital
//! saturators typically fall apart.
//!
//! What "cramping" can mean in this architecture, and where each is
//! tested:
//!
//!  1. **Decimator passband droop/warp** — the classic oversampler cost.
//!     Covered by `fir_round_trip_flat_to_20k_at_44k1_and_48k`
//!     (±0.1 dB limit; measured ±0.0002 dB design ripple).
//!  2. **Bilinear warping of the base-rate companding shelves.** The
//!     pre/post high shelves at 2.5 kHz both cramp near Nyquist — the
//!     claim "the response stays flat" holds only if cut and boost cramp
//!     *reciprocally* so the product cancels exactly. Tested here at the
//!     deepest companding setting, at 44.1 kHz where warping is worst.
//!  3. **Near-Nyquist fundamentals.** For a 15–18 kHz tone EVERY harmonic
//!     is ultrasonic: the output should be essentially the (level-scaled)
//!     fundamental, and everything else in the audible band is artifact.
//!     This is the hardest honest alias test and was previously untested.
//!
//! Known, deliberate trade (documented, not hidden): the half-band's
//! transition shoulder folds 20 kHz–Nyquist onto itself. At 44.1 kHz that
//! band reaches down to exactly 20 kHz, which is why these tests run at
//! 44.1 kHz as well as 48 kHz.

use stave_dsp::{Oversampler2xFir, Oversampler2xFirInner, SmoothSat};
use stave_measure::tone_test::ToneTest;
use stave_multiband_comp_vst3::saturation::{BandSaturator, SaturationCharacter};

/// Companding reciprocity: with the shaper ~linear (tiny signal, gentle
/// drive), the full pre-emphasis → 4× stack → post-emphasis chain must be
/// FLAT even at the deepest HF-smooth setting — i.e. the −18 dB and
/// +18 dB shelves must be exact inverses INCLUDING their bilinear cramp
/// near Nyquist. A reciprocity error would read as a treble shift that
/// grows with the smooth-highs knob.
#[test]
fn companding_shelves_cancel_exactly_up_to_20k() {
    for fs in [44_100.0f64, 48_000.0] {
        let n = 8192usize;
        let top = (19_990.0 * n as f64 / fs) as usize;
        // 1 kHz reference; probes from 8 kHz up into the cramping zone.
        let m_ref = (1_000.0 * n as f64 / fs) as usize;
        let probes = [(8_000.0), (12_000.0), (16_000.0), (18_500.0)];
        let chain_gain = |m: usize| -> f64 {
            let mut s = SmoothSat::new();
            s.prepare(fs as f32);
            s.set_drive(0.5);
            s.set_hf_smoothing(18.0); // deepest companding = worst case
            let mut outer = Oversampler2xFir::new();
            outer.prepare(fs as f32);
            let mut inner = Oversampler2xFirInner::new();
            inner.prepare(fs as f32 * 2.0);
            // Tiny amplitude keeps the Chebyshev shape in its linear
            // region, so the measurement isolates the FILTERS.
            ToneTest {
                bin: m,
                window: n,
                sample_rate: fs,
                amplitude: 0.02,
                warmup_windows: 1,
            }
            .gain_db(|x: f32| {
                let pre = s.process_pre(x);
                let sh = outer.process_one(pre, |a| inner.process_one(a, |b| s.shape(b)));
                s.process_post(sh)
            })
        };
        let g_ref = chain_gain(m_ref);
        let mut worst = 0.0f64;
        let mut worst_hz = 0.0f64;
        for &hz in &probes {
            let m = (hz * n as f64 / fs).round() as usize;
            let dev = chain_gain(m) - g_ref;
            if dev.abs() > worst {
                worst = dev.abs();
                worst_hz = hz;
            }
        }
        // And the very top of the band.
        let dev_top = chain_gain(top) - g_ref;
        if dev_top.abs() > worst {
            worst = dev_top.abs();
            worst_hz = top as f64 * fs / n as f64;
        }
        println!("fs {fs}: companding reciprocity worst dev {worst:.3} dB @ {worst_hz:.0} Hz");
        assert!(
            worst < 0.25,
            "companding shelves don't cancel at fs {fs}: {worst:.3} dB @ {worst_hz:.0} Hz — the ±shelves cramp differently"
        );
    }
}

/// Near-Nyquist fundamentals through the production 4× tube path. For
/// tones at ~12/15/18 kHz every generated harmonic is ultrasonic; any
/// audible-band energy besides the fundamental is alias/artifact.
/// Run at both 44.1 kHz (shoulder reaches 20 kHz) and 48 kHz.
#[test]
fn saturator_tube_is_clean_on_near_nyquist_tones() {
    let n = 8192usize;
    let mut worst = -300.0f64;
    for fs in [44_100.0f64, 48_000.0] {
        for hz in [12_000.0f64, 15_000.0, 18_000.0] {
            let m = (hz * n as f64 / fs).round() as usize;
            let mut sat = BandSaturator::new();
            sat.prepare(fs as f32);
            sat.set_type(SaturationCharacter::Tube);
            sat.set_mix(1.0);
            sat.set_drive_db(12.0);
            let r = ToneTest {
                bin: m,
                window: n,
                sample_rate: fs,
                amplitude: 0.5,
                warmup_windows: 2,
            }
            .alias(|x: f32| sat.process(x));
            println!(
                "tube 4× {:.1} kHz @ {:.1}k: audible {:>6.1} dB (full {:>6.1} dB)",
                hz / 1000.0,
                fs / 1000.0,
                r.audible_db,
                r.full_db
            );
            worst = worst.max(r.audible_db);
        }
    }
    // Measured worst −47.2 dB (18 kHz @ 44.1 k). The dominant residual is
    // the ADAA inner-rate fold (e.g. the 9th harmonic of 18 kHz at
    // 162 kHz folding around the 176.4 kHz inner rate to 14.4 kHz) — a
    // ceiling no decimator can lift; only more oversampling or higher
    // ADAA order can. 48 k measures −52…−95 dB. Regression limit with
    // ~3 dB margin on a torture signal (an isolated near-Nyquist tone at
    // half scale, driven 12 dB).
    assert!(
        worst < -44.0,
        "near-Nyquist tube alias regressed: {worst:.1} dB"
    );
}

/// Same sweep through the Velvet chain (SmoothSat + nested 4×).
#[test]
fn smoothsat_is_clean_on_near_nyquist_tones() {
    let n = 8192usize;
    let mut worst = -300.0f64;
    for fs in [44_100.0f64, 48_000.0] {
        for hz in [12_000.0f64, 15_000.0, 18_000.0] {
            let m = (hz * n as f64 / fs).round() as usize;
            let mut s = SmoothSat::new();
            s.prepare(fs as f32);
            s.set_drive(1.6);
            s.set_smoothness(0.7);
            s.set_hf_smoothing(4.0);
            let mut outer = Oversampler2xFir::new();
            outer.prepare(fs as f32);
            let mut inner = Oversampler2xFirInner::new();
            inner.prepare(fs as f32 * 2.0);
            let r = ToneTest {
                bin: m,
                window: n,
                sample_rate: fs,
                amplitude: 0.7,
                warmup_windows: 2,
            }
            .alias(|x: f32| {
                let pre = s.process_pre(x);
                let sh = outer.process_one(pre, |a| inner.process_one(a, |b| s.shape(b)));
                s.process_post(sh)
            });
            println!(
                "velvet 4× {:.1} kHz @ {:.1}k: audible {:>6.1} dB (full {:>6.1} dB)",
                hz / 1000.0,
                fs / 1000.0,
                r.audible_db,
                r.full_db
            );
            worst = worst.max(r.audible_db);
        }
    }
    // Measured worst −52.6 dB (15 kHz @ 44.1 k; 48 k measures −84…−107;
    // re-baselined after the smoothness DECAY_MAX floor re-voiced the
    // tail). The bounded Chebyshev spectrum keeps even the torture case
    // clearly cleaner than the tube — the engine-choice argument in one
    // number.
    assert!(
        worst < -49.0,
        "near-Nyquist velvet alias regressed: {worst:.1} dB"
    );
}

/// The "silk sounds like a low-pass" regression guard. The smoothness
/// control shapes the DISTORTION spectrum of loud content; it must not
/// re-gain quiet content. Mechanism this pins down: the odd Chebyshev
/// weights contribute ±n·aₙ to the shaper's small-signal slope, so
/// before normalisation a smoothness sweep moved f′(0) by ~6.6 dB at
/// typical settings (and could INVERT it at max odd) — quiet material
/// and small HF components got quieter as silk went up, reading exactly
/// like a low-pass. With slope normalisation, a quiet 12 kHz probe must
/// pass at identical gain at every smoothness (and every odd weight).
#[test]
fn smoothness_does_not_regain_quiet_highs() {
    let fs = 48_000.0;
    let n = 8192usize;
    let m = (12_000.0 * n as f64 / fs).round() as usize;
    let gain_at = |smooth: f32, odd: f32| -> f64 {
        let mut s = SmoothSat::new();
        s.prepare(fs as f32);
        s.set_drive(2.0);
        s.set_even(0.21);
        s.set_odd(odd);
        s.set_smoothness(smooth);
        s.set_hf_smoothing(0.0);
        // Amplitude far below the knee: measures the small-signal gain.
        ToneTest {
            bin: m,
            window: n,
            sample_rate: fs,
            amplitude: 0.02,
            warmup_windows: 1,
        }
        .gain_db(|x: f32| s.process(x))
    };
    let mut worst = 0.0f64;
    let reference = gain_at(0.0, 0.12);
    for &smooth in &[0.0f32, 0.3, 0.5, 0.7, 0.9, 0.99] {
        for &odd in &[0.0f32, 0.12, 0.35, 0.6] {
            let dev = gain_at(smooth, odd) - reference;
            if dev.abs() > worst {
                worst = dev.abs();
            }
        }
    }
    println!("quiet 12 kHz probe gain spread across silk×body matrix: {worst:.3} dB");
    assert!(
        worst < 0.35,
        "smoothness/odd re-gains quiet highs by {worst:.2} dB — small-signal slope not normalised"
    );
}

/// Level dependence: alias behaviour must not have a hidden bad spot at
/// quiet or hot input (LUT edges, envelope makeup, table extrapolation
/// are all level-dependent machinery).
#[test]
fn saturator_alias_floor_holds_across_input_levels() {
    let fs = 48_000.0;
    let n = 8192usize;
    let m = 1195usize; // ~7 kHz
    let mut worst = -300.0f64;
    for amp in [0.1f64, 0.25, 0.5, 0.9] {
        let mut sat = BandSaturator::new();
        sat.prepare(fs as f32);
        sat.set_type(SaturationCharacter::Tube);
        sat.set_mix(1.0);
        sat.set_drive_db(12.0);
        let r = ToneTest {
            bin: m,
            window: n,
            sample_rate: fs,
            amplitude: amp,
            warmup_windows: 2,
        }
        .alias(|x: f32| sat.process(x));
        println!("tube 4× 7 kHz amp {amp}: audible {:>6.1} dB", r.audible_db);
        worst = worst.max(r.audible_db);
    }
    // Measured: −60 dB (quiet) degrading smoothly to −49.6 dB at 0.9 —
    // monotone in level, no hidden hot spot. Regression limit with margin.
    assert!(
        worst < -46.0,
        "level sweep exposed an alias hot spot: {worst:.1} dB"
    );
}
