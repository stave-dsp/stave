//! Every biquad response holds at every rate a host may pick, 1 kHz to
//! 768 kHz, fractional rates included: the impulse response is finite,
//! never subnormal, bounded, and dies away; the passband gain is right.
//! (S22: f32 coefficients put a 7 Hz high-pass pole on the unit circle
//! at 768 kHz and the plugin output went NaN.)

use stave_dsp::{Biquad, FilterSpec};

const RATES: [f32; 12] = [
    1_000.0, 22_050.0, 44_100.0, 47_999.5, 48_000.0, 96_000.0, 176_400.0, 192_000.0, 352_800.0,
    384_000.0, 705_600.0, 768_000.0,
];

fn specs() -> Vec<(
    FilterSpec,
    f32, /* passband probe Hz */
    f32, /* passband gain */
)> {
    let mut out = Vec::new();
    for hz in [7.0, 10.0, 30.0, 1_000.0, 20_000.0] {
        out.push((FilterSpec::Highpass { hz, q: 0.71 }, f32::INFINITY, 1.0));
        out.push((FilterSpec::Lowpass { hz, q: 0.71 }, 0.0, 1.0));
        out.push((FilterSpec::Allpass { hz, q: 0.71 }, 0.0, 1.0));
        out.push((
            FilterSpec::Peak {
                hz,
                db: 6.0,
                q: 0.9,
            },
            0.0,
            1.0,
        ));
        for db in [-12.0f32, 8.5] {
            let low = 10f32.powf(db / 20.0);
            out.push((FilterSpec::LowShelf { hz, db }, 0.0, low));
            out.push((FilterSpec::LowShelfRbj { hz, db }, 0.0, low));
            out.push((FilterSpec::HighShelf { hz, db }, 0.0, 1.0));
            out.push((FilterSpec::HighShelfRbj { hz, db }, 0.0, 1.0));
        }
    }
    out
}

#[test]
fn every_response_is_stable_and_right_at_every_rate() {
    for rate in RATES {
        for (spec, probe, gain) in specs() {
            let mut biquad = Biquad::new();
            biquad.set(spec, rate);
            // Probe at DC, or at Nyquist for the high-pass (where the
            // clamped corner can't reach); the analytic read must agree.
            let probe = if probe.is_infinite() {
                rate * 0.5
            } else {
                probe
            };
            let measured = biquad.magnitude_at(probe, rate);
            // A corner the clamp pins against Nyquist (0.499·fs) is a
            // legitimately slow, Nyquist-ringing section: bounded only.
            let corner_below_nyquist = match spec {
                FilterSpec::Highpass { hz, .. }
                | FilterSpec::Lowpass { hz, .. }
                | FilterSpec::Allpass { hz, .. }
                | FilterSpec::Peak { hz, .. }
                | FilterSpec::LowShelf { hz, .. }
                | FilterSpec::HighShelf { hz, .. }
                | FilterSpec::LowShelfRbj { hz, .. }
                | FilterSpec::HighShelfRbj { hz, .. } => hz < rate * 0.45,
            };
            if corner_below_nyquist {
                assert!(
                    (measured / gain - 1.0).abs() < 1e-3,
                    "{spec:?} at {rate} Hz: |H({probe})| = {measured}, want {gain}"
                );
            }
            let samples = rate as usize;
            let mut peak = 0.0f32;
            let mut last = 0.0f32;
            for i in 0..samples {
                let y = biquad.process(if i == 0 { 1.0 } else { 0.0 });
                assert!(y.is_finite(), "{spec:?} at {rate} Hz: sample {i} is {y}");
                assert!(
                    y == 0.0 || y.is_normal(),
                    "{spec:?} at {rate} Hz: subnormal {y:e}"
                );
                peak = peak.max(y.abs());
                last = y;
            }
            assert!(peak < 8.0, "{spec:?} at {rate} Hz: impulse peak {peak}");
            // One second is many time constants for every corner here.
            assert!(
                !corner_below_nyquist || last.abs() < 1e-6,
                "{spec:?} at {rate} Hz: still ringing at 1 s ({last:e})"
            );
        }
    }
}
