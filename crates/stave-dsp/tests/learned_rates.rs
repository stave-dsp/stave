//! DSP.md M7 — recurrent networks at host rates they were not trained
//! at, by [`RatePolicy::Interpolated`] (Carson et al. 2024's cubic
//! Lagrange delay line of past states), against the network run at its
//! trained rate and resampled to the host's.
//!
//! **The reference.** A sine whose period is a whole number of samples
//! at every rate here (a multiple of 100 Hz: 10 ms is 441, 480, 882,
//! 960 and 1920 samples) drives the network until its output repeats,
//! then one period is captured. The trained-rate run resampled to the
//! host rate is exactly its Fourier series kept below the lower Nyquist
//! (band-limited resampling of a periodic signal), so the two runs are
//! compared bin by bin on the 100 Hz grid, no resampler in the way.
//!
//! **The figures**, both in dB, both over 20 Hz–20 kHz:
//! * `SNRH` — the harmonics of the tone (Carson et al.'s SNR of the
//!   harmonic components): the response the network learned.
//! * `SNR` — every bin, so also each run's own aliasing, which differs
//!   between rates by construction (the trained run folds at 24 kHz,
//!   the host run at its own Nyquist).
//!
//! The tones are 700 Hz and 3.1 kHz at 0.5 (7 and 31 are prime to
//! 480, so the trained run's folds land between the harmonics, not on
//! them); each figure is the worse tone's. `RatePolicy::Any` (the
//! trained recursion at the host rate, every time constant scaled) is
//! printed beside each row as the baseline the policy must beat.
//!
//! Budgets sit 3 dB under the measurement (the reference machine,
//! 2026-11-13), rounded down to a whole dB. Measured: above the trained
//! rate at a fractional ratio (88.2 kHz) 75-86 dB; at whole ratios (96,
//! 192 kHz) 122-139 dB, single precision, the trained recursion
//! interleaved; below it (44.1 kHz, extrapolating 0.08 of a sample)
//! 47-58 dB. `Any` manages -2 to 28 dB.

use stave_dsp::learned::{nam, Gru, RatePolicy};
use stave_dsp::Stage;

const TRAINED: f32 = 48_000.0;
const GRID_HZ: f64 = 100.0;
const TONES_HZ: [f64; 2] = [700.0, 3_100.0];
const AMPLITUDE: f64 = 0.5;
const AUDIBLE_HZ: f64 = 20_000.0;

/// One network at one host rate: the harmonic SNR budget, and the
/// figures measured when it was set (SNRH, SNR, and `Any`'s SNRH).
struct Row {
    network: &'static str,
    rate: f32,
    snrh_budget: f64,
    snrh: f64,
    snr: f64,
    any_snrh: f64,
}

const fn row(
    network: &'static str,
    rate: f32,
    snrh_budget: f64,
    snrh: f64,
    snr: f64,
    any_snrh: f64,
) -> Row {
    Row {
        network,
        rate,
        snrh_budget,
        snrh,
        snr,
        any_snrh,
    }
}

const ROWS: &[Row] = &[
    row("LSTM 2 x 5 (fixture)", 44_100.0, 48.0, 51.5, 51.5, 23.5),
    row("LSTM 2 x 5 (fixture)", 88_200.0, 76.0, 79.5, 79.5, 6.9),
    row("LSTM 2 x 5 (fixture)", 96_000.0, 123.0, 126.1, 114.9, 6.0),
    row("LSTM 2 x 5 (fixture)", 192_000.0, 121.0, 124.3, 113.0, 1.5),
    row("GRU 2 x 4 (fixture)", 44_100.0, 44.0, 47.1, 47.1, 28.4),
    row("GRU 2 x 4 (fixture)", 88_200.0, 71.0, 74.8, 74.8, 6.5),
    row("GRU 2 x 4 (fixture)", 96_000.0, 120.0, 123.4, 109.7, 5.0),
    row("GRU 2 x 4 (fixture)", 192_000.0, 119.0, 122.9, 107.8, -1.5),
    row("LSTM 1 x 16 (random)", 44_100.0, 55.0, 58.2, 58.2, 26.4),
    row("LSTM 1 x 16 (random)", 88_200.0, 83.0, 86.4, 86.4, 9.5),
    row("LSTM 1 x 16 (random)", 96_000.0, 135.0, 138.8, 126.9, 8.5),
    row("LSTM 1 x 16 (random)", 192_000.0, 133.0, 136.2, 125.2, 3.7),
    row("GRU 1 x 16 (random)", 44_100.0, 47.0, 50.2, 50.2, 21.8),
    row("GRU 1 x 16 (random)", 88_200.0, 74.0, 77.7, 77.7, 7.4),
    row("GRU 1 x 16 (random)", 96_000.0, 121.0, 124.1, 116.2, 6.7),
    row("GRU 1 x 16 (random)", 192_000.0, 119.0, 122.1, 114.5, 4.0),
];

/// Weights in ±`scale` from an LCG (the datasheet's generator), rounded
/// so the JSON stays short.
fn weights(n: usize, scale: f32) -> String {
    let mut lcg: u32 = 11;
    let w: Vec<String> = (0..n)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            let unit = (lcg >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
            format!("{:.4}", 2.0 * scale * unit)
        })
        .collect();
    format!("[{}]", w.join(","))
}

/// A network by name, at the trained rate, with `policy`.
fn network(name: &str, policy: RatePolicy) -> Box<dyn Stage> {
    match name {
        "LSTM 2 x 5 (fixture)" => {
            let mut n = nam::parse(include_str!("reference/nam/lstm.nam"))
                .unwrap()
                .network;
            n.set_rate_policy(policy);
            Box::new(n)
        }
        "GRU 2 x 4 (fixture)" => {
            let mut n = Gru::parse(include_str!("reference/nam/gru.json")).unwrap();
            n.set_rate_policy(policy);
            Box::new(n)
        }
        "LSTM 1 x 16 (random)" => {
            let w = weights(4 * 16 * 17 + 4 * 16 + 2 * 16 + 17, 0.5);
            let text = format!(
                r#"{{"version":"0.7.0","architecture":"LSTM","sample_rate":48000,
                "config":{{"input_size":1,"hidden_size":16,"num_layers":1}},"weights":{w}}}"#
            );
            let mut n = nam::parse(&text).unwrap().network;
            n.set_rate_policy(policy);
            Box::new(n)
        }
        "GRU 1 x 16 (random)" => {
            let w = weights(3 * 16 * 17 + 6 * 16 + 17, 0.5);
            let mut n = Gru::parse(&format!(
                r#"{{"input_size":1,"hidden_size":16,"num_layers":1,"sample_rate":48000,"weights":{w}}}"#
            ))
            .unwrap();
            n.set_rate_policy(policy);
            Box::new(n)
        }
        other => panic!("no network {other}"),
    }
}

/// One steady period of the network's output to a sine at `hz`, run at
/// `rate`: two seconds of warm-up, then a period. Also returns the
/// largest difference between the last two periods (the steadiness).
fn period(name: &str, policy: RatePolicy, rate: f32, hz: f64) -> (Vec<f64>, f64) {
    let mut net = network(name, policy);
    net.prepare(rate);
    let n = (f64::from(rate) / GRID_HZ).round() as usize;
    let periods = (2.0 * GRID_HZ) as usize;
    let mut last = vec![0.0f64; n];
    let mut steadiness = 0.0f64;
    for p in 0..periods {
        for (i, slot) in last.iter_mut().enumerate() {
            let t = i as f64 / f64::from(rate);
            let x = AMPLITUDE * (std::f64::consts::TAU * hz * t).sin();
            let y = f64::from(net.tick(x as f32));
            if p == periods - 1 {
                steadiness = steadiness.max((y - *slot).abs());
            }
            *slot = y;
        }
    }
    (last, steadiness)
}

/// The Fourier series coefficient of one period at `hz`.
fn bin(period: &[f64], rate: f32, hz: f64) -> (f64, f64) {
    let n = period.len() as f64;
    let w = std::f64::consts::TAU * hz / f64::from(rate);
    period
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (i, &y)| {
            let a = w * i as f64;
            (re + y * a.cos() / n, im - y * a.sin() / n)
        })
}

/// SNR and harmonic SNR of `host` against `reference` (both one period
/// of the same tone), in dB, over the grid bins up to 20 kHz.
fn snr(host: &[f64], host_rate: f32, reference: &[f64], tone: f64) -> (f64, f64) {
    let (mut signal, mut error, mut h_signal, mut h_error) = (0.0, 0.0, 0.0, 0.0);
    let bins = (AUDIBLE_HZ / GRID_HZ) as usize;
    for k in 1..=bins {
        let hz = k as f64 * GRID_HZ;
        let (a, b) = bin(host, host_rate, hz);
        let (c, d) = bin(reference, TRAINED, hz);
        let (s, e) = (c * c + d * d, (a - c).powi(2) + (b - d).powi(2));
        signal += s;
        error += e;
        if (hz / tone).fract() == 0.0 {
            h_signal += s;
            h_error += e;
        }
    }
    let db = |s: f64, e: f64| 10.0 * (s / e.max(1e-300)).log10();
    (db(signal, error), db(h_signal, h_error))
}

/// The worse tone's (SNR, SNRH) for one network, rate and policy.
fn measure(name: &str, rate: f32, policy: RatePolicy) -> (f64, f64) {
    TONES_HZ
        .iter()
        .map(|&tone| {
            let (reference, steady_ref) = period(name, RatePolicy::Strict, TRAINED, tone);
            let (host, steady) = period(name, policy, rate, tone);
            assert!(
                steady_ref < 1e-4 && steady < 1e-4,
                "{name} at {rate} Hz, {tone} Hz: not periodic ({steady_ref:e}, {steady:e})"
            );
            snr(&host, rate, &reference, tone)
        })
        .fold((f64::INFINITY, f64::INFINITY), |(a, b), (s, h)| {
            (a.min(s), b.min(h))
        })
}

#[test]
#[cfg_attr(miri, ignore)]
fn recurrent_networks_run_at_any_rate() {
    let mut failures = Vec::new();
    for r in ROWS {
        let (snr, snrh) = measure(r.network, r.rate, RatePolicy::Interpolated);
        let (_, any_snrh) = measure(r.network, r.rate, RatePolicy::Any);
        println!(
            "{:<22} {:>5.1} kHz  SNRH {:>6.1} dB (budget {:>5.1}, recorded {:>6.1})  SNR {:>6.1} (recorded {:>6.1})  Any: SNRH {:>6.1} (recorded {:>6.1})",
            r.network,
            r.rate / 1e3,
            snrh,
            r.snrh_budget,
            r.snrh,
            snr,
            r.snr,
            any_snrh,
            r.any_snrh
        );
        if snrh < r.snrh_budget {
            failures.push(format!(
                "{} at {} Hz: SNRH {snrh:.1} dB under its {:.1} dB budget",
                r.network, r.rate, r.snrh_budget
            ));
        }
        if snrh <= any_snrh {
            failures.push(format!(
                "{} at {} Hz: Interpolated ({snrh:.1} dB) no better than Any ({any_snrh:.1} dB)",
                r.network, r.rate
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// At a whole ratio the delay line reads stored states exactly, so the
/// host-rate run is the trained recursion interleaved: every second
/// sample at 96 kHz is the 48 kHz network on every second input sample,
/// bit for bit.
#[test]
#[cfg_attr(miri, ignore)]
fn whole_ratios_interleave_the_trained_recursion() {
    for name in ["LSTM 2 x 5 (fixture)", "GRU 2 x 4 (fixture)"] {
        let mut host = network(name, RatePolicy::Interpolated);
        host.prepare(96_000.0);
        let mut trained = network(name, RatePolicy::Strict);
        trained.prepare(TRAINED);
        let x: Vec<f32> = (0..2000)
            .map(|i| 0.6 * (i as f32 * 0.013).sin() + 0.3 * (i as f32 * 0.31).sin())
            .collect();
        let fast: Vec<f32> = x.iter().map(|&v| host.tick(v)).collect();
        for (i, &v) in x.iter().enumerate().step_by(2) {
            assert_eq!(
                trained.tick(v).to_bits(),
                fast[i].to_bits(),
                "{name} at {i}"
            );
        }
    }
}

/// A WaveNet has no state to delay: `Interpolated` leaves it strict
/// (silent, mismatch reported); a 2x region serves a half-rate host.
#[test]
#[cfg_attr(miri, ignore)]
fn wavenets_stay_rate_bound() {
    let mut net = nam::parse(include_str!("reference/nam/wavenet_standard.nam"))
        .unwrap()
        .network;
    net.set_rate_policy(RatePolicy::Interpolated);
    net.prepare(96_000.0);
    assert_eq!(net.tick(0.3), 0.0);
    assert_eq!(net.rate_mismatch().unwrap().prepared, 96_000.0);
    net.prepare(TRAINED);
    assert!(net.tick(0.3) != 0.0);
}

/// Outside half to sixteen times the trained rate the policy is strict.
#[test]
#[cfg_attr(miri, ignore)]
fn interpolation_has_a_range() {
    for (rate, sounds) in [
        (22_050.0, false),
        (24_000.0, true),
        (768_000.0, true),
        (800_000.0, false),
    ] {
        let mut net = network("GRU 2 x 4 (fixture)", RatePolicy::Interpolated);
        net.prepare(rate);
        let y = (0..64)
            .map(|i| net.tick((i as f32 * 0.1).sin()).abs())
            .fold(0.0, f32::max);
        assert_eq!(y > 0.0, sounds, "at {rate} Hz");
    }
}
