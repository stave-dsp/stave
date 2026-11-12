//! Single-tone analysis: gain at the fundamental, harmonic distortion
//! (THD) and distortion + noise (THD+N), by Goertzel on a bin-snapped
//! tone (see [`Sine::bin_snapped`](super::signal::Sine::bin_snapped)) —
//! coherent, so no window and no leakage between harmonics.

use crate::level::db;
use crate::signal::Sine;
use crate::spectrum::goertzel;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Harmonic {
    pub order: u32,
    /// Level relative to the fundamental.
    pub dbc: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ToneReport {
    pub freq_hz: f64,
    /// Output fundamental relative to the input tone.
    pub gain_db: f64,
    pub thd_pct: f64,
    pub thdn_pct: f64,
    /// Harmonics 2, 3, … below `min(max_hz, Nyquist)`, in order.
    pub harmonics: Vec<Harmonic>,
}

/// Analyze the output `x` of a tone at `freq` with input amplitude
/// `input_amp`. Harmonics are counted up to `min(max_hz, Nyquist)`.
/// What a tone analysis needs to know: the tone that went in, and how
/// high to count harmonics.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ToneAnalysis {
    pub input: Sine,
    pub max_harmonic_hz: f64,
}

/// Gain, THD and THD+N of `x`, the output for `analysis.input`.
pub fn analyze(x: &[f32], analysis: &ToneAnalysis) -> ToneReport {
    let Sine {
        frequency_hz: freq,
        amplitude: input_amp,
        sample_rate: rate,
    } = analysis.input;
    let fundamental = goertzel(x, freq / rate);
    let limit = analysis.max_harmonic_hz.min(rate / 2.0);
    let harmonic_amps: Vec<(u32, f64)> = (2..)
        .map(|k| (k, freq * f64::from(k)))
        .take_while(|&(_, f)| f < limit)
        .map(|(k, f)| (k, goertzel(x, f / rate)))
        .collect();
    let harm_power: f64 = harmonic_amps.iter().map(|(_, a)| a * a).sum();
    let mean = x.iter().map(|&v| f64::from(v)).sum::<f64>() / x.len().max(1) as f64;
    let total_power = x
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / x.len().max(1) as f64;
    let fund_power = fundamental * fundamental / 2.0;
    let rel = |a: f64| a / fundamental.max(1e-12);
    ToneReport {
        freq_hz: freq,
        gain_db: db(fundamental / input_amp.max(1e-12)),
        thd_pct: rel(harm_power.sqrt()) * 100.0,
        thdn_pct: ((total_power - fund_power).max(0.0) / fund_power.max(1e-24)).sqrt() * 100.0,
        harmonics: harmonic_amps
            .into_iter()
            .map(|(order, a)| Harmonic {
                order,
                dbc: db(rel(a)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{db_to_amp, Signal, Sine};

    const RATE: f64 = 48_000.0;
    const N: usize = 16_384;

    fn render(f: impl Fn(u64) -> f32) -> Vec<f32> {
        (0..N as u64).map(f).collect()
    }

    #[test]
    fn goertzel_reads_amplitude_of_a_coherent_tone() {
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(-6.0),
            sample_rate: RATE,
        }
        .bin_snapped(N);
        let x = render(|t| s.at(0, t));
        assert!((goertzel(&x, s.frequency_hz / RATE) - s.amplitude).abs() < 1e-6);
        assert!(goertzel(&x, (s.frequency_hz * 2.0) / RATE) < 1e-6);
    }

    #[test]
    fn clean_gain_stage() {
        let s = Sine {
            frequency_hz: 1_000.0,
            amplitude: db_to_amp(-12.0),
            sample_rate: RATE,
        }
        .bin_snapped(N);
        let x = render(|t| 2.0 * s.at(0, t));
        let r = analyze(
            &x,
            &ToneAnalysis {
                input: Sine {
                    frequency_hz: s.frequency_hz,
                    amplitude: s.amplitude,
                    sample_rate: RATE,
                },
                max_harmonic_hz: 20_000.0,
            },
        );
        assert!((r.gain_db - 6.0206).abs() < 1e-3, "{r:?}");
        assert!(r.thd_pct < 1e-4 && r.thdn_pct < 1e-3, "{r:?}");
    }

    #[test]
    fn known_third_harmonic() {
        // x = sin(wt) + 0.1·sin(3wt): THD exactly 10%, H3 at −20 dBc.
        let s = Sine {
            frequency_hz: 500.0,
            amplitude: db_to_amp(0.0),
            sample_rate: RATE,
        }
        .bin_snapped(N);
        let w = std::f64::consts::TAU * s.frequency_hz / RATE;
        let x = render(|t| ((w * t as f64).sin() + 0.1 * (3.0 * w * t as f64).sin()) as f32);
        let r = analyze(
            &x,
            &ToneAnalysis {
                input: Sine {
                    frequency_hz: s.frequency_hz,
                    amplitude: 1.0,
                    sample_rate: RATE,
                },
                max_harmonic_hz: 20_000.0,
            },
        );
        assert!((r.thd_pct - 10.0).abs() < 1e-3, "{r:?}");
        assert!((r.thdn_pct - 10.0).abs() < 1e-2);
        let h3 = r.harmonics.iter().find(|h| h.order == 3).unwrap();
        assert!((h3.dbc + 20.0).abs() < 1e-3);
        assert!(r.harmonics.iter().find(|h| h.order == 2).unwrap().dbc < -100.0);
    }

    #[test]
    fn harmonics_stop_below_the_limit() {
        let r = analyze(
            &[0.0; 64],
            &ToneAnalysis {
                input: Sine {
                    frequency_hz: 5_000.0,
                    amplitude: 1.0,
                    sample_rate: RATE,
                },
                max_harmonic_hz: 16_000.0,
            },
        );
        assert_eq!(
            r.harmonics.iter().map(|h| h.order).collect::<Vec<_>>(),
            [2, 3]
        );
    }
}
