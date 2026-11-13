//! Tape (DSP.md E8) against its published references: the play head's
//! loss formula and Chowdhury's figures (DAFx-19), a measured transport's
//! flutter spectrum (Mikkonen et al., DAFx-23), and the measurement
//! design's aliasing (A4) and sample-rate invariance (C11).
//!
//! The figures are digitised in `crates/stave-test/datasheet/`
//! (`chowdhury2019_fig2_loop`, `chowdhury2019_fig3_loss`,
//! `akai4000d_flutter`), each with its method and uncertainty. A miss is
//! a **known gap**: asserted still a miss and no worse than recorded.

use stave_dsp::physical::tape::{
    Chowdhury, Flutter, Formulation, Machine, Phase, Playback, Recording, Transport,
};
use stave_dsp::Oversampled;
use stave_dsp::circuit::devices::magnetic::{JilesAtherton, JilesAthertonCore};
use stave_dsp::circuit::methods::newton::NewtonSettings;
use stave_dsp::primitives::spectral::{Complex, Fft};
use stave_dsp::Stage;
use stave_measure::aliasing::SteppedTone;
use stave_test::reference::Datasheet;

/// The loss filter's budget against eq. 13, dB, from 20 Hz to 20 kHz
/// wherever the formula is above [`FLOOR_DB`].
const LOSS_DB: f64 = 0.5;
const FLOOR_DB: f64 = -60.0;

/// The FIR's gain at `hz`, dB.
fn fir_db(taps: &[f64], hz: f64, rate: f64) -> f64 {
    let w = -std::f64::consts::TAU * hz / rate;
    let (re, im) = taps
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (j, h)| (re + h * (w * j as f64).cos(), im + h * (w * j as f64).sin()));
    10.0 * (re * re + im * im).max(1e-300).log10()
}

#[test]
fn loss_filter_matches_the_gap_loss_formula_at_every_speed() {
    let head = Machine::TC260.play;
    let thickness = Formulation::FERRIC.thickness;
    let mut worst: f64 = 0.0;
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for ips in [3.75, 7.5, 15.0, 30.0] {
            for phase in [Phase::Linear, Phase::Minimum] {
                let mut p = Playback::new(head, thickness, Machine::ips(ips), phase);
                p.prepare(rate as f32);
                let taps: Vec<f64> = p.taps().collect();
                for i in 0..=120 {
                    let hz = 20.0 * 1000f64.powf(f64::from(i) / 120.0);
                    let formula = 20.0 * p.loss(hz).abs().log10();
                    if formula < FLOOR_DB {
                        continue;
                    }
                    let e = (fir_db(&taps, hz, rate) - formula).abs();
                    assert!(e <= LOSS_DB, "{rate} Hz, {ips} ips, {phase:?}: {e:.3} dB at {hz:.0} Hz");
                    worst = worst.max(e);
                }
            }
        }
    }
    println!("E8 | playback loss vs eq. 13 | worst {worst:.3} dB | budget {LOSS_DB} dB | pass");
    // The gap null at 3.75 ips: v/g = 0.09525 / 5e-6 = 19.05 kHz.
    let slow = Playback::new(head, thickness, Machine::ips(3.75), Phase::Linear);
    assert!(slow.loss(19_050.0).abs() < 1e-3);
}

#[test]
fn loss_filter_matches_chowdhury_fig_3() {
    // Fig. 3 is eq. 13 at 15 ips through a 100-tap FIR: its shape is the
    // formula's; its level sits a constant ~1.2 dB above (the FIR's own
    // normalisation, the table notes), so the shape is held after
    // removing the mean offset, and the offset is reported.
    let sheet = Datasheet::load("chowdhury2019_fig3_loss");
    let curve = &sheet.curves()[0].reference;
    let mut p = Playback::new(Machine::TC260.play, Formulation::FERRIC.thickness, Machine::ips(15.0), Phase::Linear);
    p.prepare(48_000.0);
    let taps: Vec<f64> = p.taps().collect();
    let pairs: Vec<(f64, f64)> = curve
        .abscissa()
        .iter()
        .zip(curve.values())
        .filter(|(hz, _)| **hz >= 1_000.0) // the paper's FIR flattens below (480 Hz resolution)
        .map(|(&hz, &db)| (db, fir_db(&taps, hz, 48_000.0)))
        .collect();
    let offset = pairs.iter().map(|(fig, ours)| fig - ours).sum::<f64>() / pairs.len() as f64;
    let worst = pairs.iter().map(|(fig, ours)| (fig - ours - offset).abs()).fold(0.0, f64::max);
    let budget = sheet.uncertainty_y().at(0.0) + 0.3;
    println!("E8 | playback loss vs Chowdhury Fig. 3 | shape {worst:.2} dB (offset {offset:.2} dB) | budget {budget:.2} dB | pass");
    assert!(worst <= budget && (0.8..1.6).contains(&offset), "{worst} {offset}");
}

/// A settled loop of `model` under `peak` A/m, normalised: the descending
/// branch's M/Mp at each H/Hp of the table.
fn loop_shape(model: JilesAtherton, peak: f64, at: &[f64]) -> Vec<f64> {
    let mut core = JilesAthertonCore::new(model, NewtonSettings::DEFAULT);
    let n = 4_000;
    let mut last = Vec::new();
    for cycle in 0..6 {
        for i in 0..n {
            let h = peak * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let m = core.step(h);
            if cycle == 5 && i <= n / 2 {
                last.push((h / peak, m));
            }
        }
    }
    let mp = last[0].1;
    at.iter()
        .map(|&x| {
            let w = last.windows(2).find(|w| (w[0].0 - x) * (w[1].0 - x) <= 0.0).expect("on the branch");
            let f = (x - w[0].0) / (w[1].0 - w[0].0);
            (w[0].1 + f * (w[1].1 - w[0].1)) / mp
        })
        .collect()
}

#[test]
fn hysteresis_loop_against_chowdhury_fig_2() {
    // Fig. 2's outer loop at 2 kA/m, normalised. Neither parameter set
    // the paper names reproduces it (the table's notes): the stated
    // constants, at the stated drive, are held as known gaps.
    let sheet = Datasheet::load("chowdhury2019_fig2_loop");
    let curve = &sheet.curves()[0].reference;
    let budget = sheet.uncertainty_y().at(0.0) + 0.05;
    let steel = JilesAtherton {
        saturation: 1.6e6,
        shape: 1_100.0,
        coupling: 1.6e-3,
        pinning: 400.0,
        reversibility: 0.2,
    };
    for (name, model, recorded) in [
        ("ferric tape (Fig. 6)", Formulation::FERRIC.magnetic, 0.24),
        ("Jiles & Atherton 1986 steel", steel, 0.27),
    ] {
        let ours = loop_shape(model, 2_000.0, curve.abscissa());
        let worst = ours.iter().zip(curve.values()).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        println!("E8 | Fig. 2 loop shape, {name} at 2 kA/m | worst {:.3} of peak | budget {budget:.3} | known gap", worst);
        assert!(worst > budget, "{name}: {worst} now passes: promote");
        assert!(worst <= recorded, "{name}: {worst} worse than recorded {recorded}");
    }
}

/// The record side under a 1 kHz tone at `dbfs`: (gain dB, H3 dBc), the
/// tape at `rate` with flutter off and the playback losses bypassed by
/// reading the record side alone.
fn tone(rate: f64, dbfs: f64, bias: f64) -> (f64, f64) {
    let mut deck = Chowdhury::new(Machine::TC260);
    deck.set_flutter(0.0);
    deck.set_bias(bias);
    deck.prepare(rate as f32);
    let period = rate / 1_000.0;
    let a = 10f64.powf(dbfs / 20.0);
    let total = (rate * 0.3) as usize;
    let start = total - (period * 100.0).round() as usize;
    let y: Vec<f64> = (0..total)
        .map(|n| f64::from(deck.tick((a * (std::f64::consts::TAU * n as f64 / period).sin()) as f32)))
        .collect();
    let y = &y[start..];
    let h = |k: f64| {
        let w = std::f64::consts::TAU * k / period;
        let (re, im) = y.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, v)| {
            (re + v * (w * i as f64).cos(), im + v * (w * i as f64).sin())
        });
        2.0 * (re * re + im * im).sqrt() / y.len() as f64
    };
    let (h1, h3) = (h(1.0), h(3.0));
    let loss3 = deck.playback().loss(3_000.0).abs() / deck.playback().loss(1_000.0).abs();
    (20.0 * (h1 / a).log10(), 20.0 * (h3 / h1 / loss3).log10())
}

#[test]
fn harmonics_against_bias() {
    // Chowdhury publishes no harmonic figures; his claims, measured: the
    // dead zone unbiased, linear with the TC-260's bias, cleaner but less
    // sensitive overbiased. Reported, and the trend held.
    let mut rows = Vec::new();
    for bias in [0.0, 1.0, 2.0, 5.0, 8.0] {
        let (g, h3) = tone(48_000.0, -6.0, bias);
        println!("E8 | 1 kHz at -6 dBFS, bias {bias} | gain {g:.1} dB, H3 {h3:.1} dBc | reported");
        rows.push((g, h3));
    }
    assert!(rows[0].1 > rows[3].1 + 30.0, "the dead zone: {rows:?}");
    assert!(rows[1].0 > rows[3].0 && rows[3].0 > rows[4].0, "sensitivity falls past its peak: {rows:?}");
}

#[test]
fn aliasing_at_the_host_rate() {
    // A4's stepped tone through the record side at 48 kHz (15x inside),
    // where the only nonlinearity is: the flutter is modulation and the
    // play head linear, and the play head's own loss (-66 dB at 8 kHz,
    // 7.5 ips) would bury the fundamental the measurement is relative to.
    // Up to 2.5 kHz everything inharmonic is the digital fold; above, the
    // bias beats with the tone's harmonics (55.4 kHz - 4 x 10 kHz =
    // 15.4 kHz), which the hysteresis makes in continuous time too: the
    // second sweep is reported, not held.
    for (amplitude, budget) in [(0.5, -100.0), (2.0, -110.0)] {
        for (low, high, held) in [(100.0, 2_500.0, true), (2_500.0, 10_000.0, false)] {
            let sweep = SteppedTone {
                sample_rate: 48_000.0,
                window: 8192,
                amplitude,
                low_hz: low,
                high_hz: high,
                steps: 6,
                warmup_windows: 3,
            };
            let report = sweep.through(|| {
                let mut record = Oversampled::linear(Recording::new(Machine::TC260), 15);
                record.prepare(48_000.0);
                move |x: f32| record.tick(x)
            });
            let worst = report.worst_audible();
            let verdict = if held { "pass" } else { "reported (bias beats)" };
            println!(
                "E8 | record side at 48 kHz, peak {amplitude}, {low}-{high} Hz | worst audible inharmonic {:.1} dB at {:.0} Hz (worst bin {:.0} Hz) | budget {budget} dB | {verdict}",
                worst.audible_db, worst.hz, worst.worst_hz
            );
            if held {
                assert!(worst.audible_db <= budget, "{worst:?}");
            }
        }
    }
}

#[test]
fn the_same_tape_at_every_rate() {
    // C11: the 1 kHz gain and third harmonic at 0 and +12 dBFS, at every
    // host rate, against 48 kHz's.
    for dbfs in [0.0, 12.0] {
        let (g48, h48) = tone(48_000.0, dbfs, 5.0);
        for rate in [44_100.0, 88_200.0, 96_000.0, 192_000.0] {
            let (g, h) = tone(rate, dbfs, 5.0);
            // The bias is 14 inner samples a cycle at every rate, so the
            // hysteresis sees the same sampled field: the same sound.
            println!("E8 | {rate} Hz at {dbfs} dBFS | gain {:+.4} dB, H3 {:+.3} dB re 48 kHz ({h48:.1} dBc) | budget 0.01 / 0.1 dB | pass", g - g48, h - h48);
            assert!((g - g48).abs() < 0.01, "{rate}: gain {g} vs {g48}");
            assert!((h - h48).abs() < 0.1, "{rate}: H3 {h} vs {h48}");
        }
    }
}

#[test]
fn flutter_matches_the_measured_spectrum() {
    let sheet = Datasheet::load("akai4000d_flutter");
    let spectrum = sheet.curve(&[("spectrum", 1.0)]).reference.clone();
    let rms = sheet.curve(&[("rms", 1.0)]).reference.values()[0];
    // The trajectory at 100 Hz, as the paper sampled it, for 40 minutes.
    let mut f = Flutter::new(Transport::AKAI_4000D);
    f.prepare(1_000.0);
    let segment = 512;
    let segments = 480;
    let mut x = Vec::with_capacity(segment * segments);
    for n in 0..segment * segments * 10 {
        f.tick(0.0);
        if n % 10 == 0 {
            x.push(f.deviation());
        }
    }
    let got_rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
    // Welch: Hann, half overlap, power per bin.
    let hann: Vec<f64> = (0..segment).map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / segment as f64).cos()).collect();
    let bins = segment / 2;
    let mut power = vec![0.0; bins];
    let mut count = 0;
    let mut start = 0;
    let fft = Fft::<f64>::new(segment);
    let mut buf = vec![Complex::new(0.0, 0.0); segment];
    while start + segment <= x.len() {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = Complex::new(x[start + i] * hann[i], 0.0);
        }
        fft.forward(&mut buf);
        for (p, b) in power.iter_mut().zip(&buf) {
            *p += b.norm_sqr();
        }
        count += 1;
        start += segment / 2;
    }
    // Band powers, ours against the table's, each relative
    // to its own total over the table's range (the table's 0 dB is the
    // figure's own).
    let table: Vec<(f64, f64)> = spectrum.abscissa().iter().copied().zip(spectrum.values().iter().copied()).collect();
    let bin_hz = 100.0 / segment as f64;
    let band = |lo: f64, hi: f64| -> (f64, f64) {
        let ours: f64 = (0..bins).filter(|&k| (lo..hi).contains(&(k as f64 * bin_hz))).map(|k| power[k] / count as f64).sum();
        let theirs: f64 = table.iter().filter(|(hz, _)| (lo..hi).contains(hz)).map(|(_, db)| 10f64.powf(db / 10.0)).sum();
        (ours, theirs)
    };
    // Octave bands from the table's first bin: a strong line's leakage
    // (Hann) and its phase walk spread it across a third-octave's edge.
    let mut edges: Vec<f64> = (0..7).map(|i| 0.58 * 2f64.powi(i)).collect();
    edges.push(50.0);
    let bands: Vec<(f64, f64, f64)> = edges.windows(2).map(|w| {
        let (o, t) = band(w[0], w[1]);
        (w[0], o, t)
    }).collect();
    let (ours_total, theirs_total): (f64, f64) = bands.iter().fold((0.0, 0.0), |(a, b), (_, o, t)| (a + o, b + t));
    let mut worst: f64 = 0.0;
    for (lo, o, t) in &bands {
        let e = 10.0 * ((o / ours_total) / (t / theirs_total)).log10();
        println!("E8 | flutter band from {lo:.2} Hz | {e:+.2} dB");
        worst = worst.max(e.abs());
    }
    let rms_err = got_rms / rms - 1.0;
    println!("E8 | flutter spectrum, octave bands | worst {worst:.2} dB | budget 1.5 dB | pass");
    println!("E8 | flutter rms {:.4} ms against {:.4} ms | {:+.1}% | budget 5% | pass", got_rms * 1e3, rms * 1e3, 100.0 * rms_err);
    assert!(worst <= 1.5, "{worst}");
    assert!(rms_err.abs() <= 0.05, "{got_rms} vs {rms}");
}
