//! Tape (DSP.md E8) against its published references: the play head's
//! loss formula and Chowdhury's figures (DAFx-19), a measured transport's
//! flutter spectrum (Mikkonen et al., DAFx-23), and the measurement
//! design's aliasing (A4) and sample-rate invariance (C11); the transport
//! (DSP.md H5) against its own processes' laws — drift, the motor's
//! curves, dropouts' statistics — and the spacing loss against eq. 13.
//!
//! The figures are digitised in `crates/stave-test/datasheet/`
//! (`chowdhury2019_fig2_loop`, `chowdhury2019_fig3_loss`,
//! `akai4000d_flutter`), each with its method and uncertainty. A miss is
//! a **known gap**: asserted still a miss and no worse than recorded.

use stave_dsp::physical::tape::{
    Capstan, Chowdhury, Condition, Drift, Flutter, Formulation, Machine, Motor, Phase, Playback,
    PlayHead, Recording, Spacing, Stop, Transport,
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

// ---- H5: the transport ----

/// Mean and (population) variance.
fn moments(x: &[f64]) -> (f64, f64) {
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let var = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / x.len() as f64;
    (mean, var)
}

#[test]
fn transport_off_is_bit_identical() {
    // The machine as E8 left it, composed by hand: record (15× inside at
    // 48 kHz, the bias on 14 inner samples), flutter, play head.
    let machine = Machine::TC260;
    let mut recording = Recording::new(machine);
    recording.set_bias_samples(14);
    let mut record = Oversampled::linear(recording, 15);
    let mut flutter = Flutter::new(Transport::AKAI_4000D);
    let mut play = Playback::new(machine.play, machine.formulation.thickness, machine.speed, Phase::Linear);
    record.prepare(48_000.0);
    flutter.prepare(48_000.0);
    play.prepare(48_000.0);
    let mut plain = Chowdhury::new(machine);
    let mut off = Chowdhury::new(machine)
        .with_capstan(Capstan::new(5))
        .with_spacing(Spacing::new(Condition::NONE, 5));
    off.prepare(48_000.0);
    assert_eq!(off.latency(), plain.latency());
    let mut differ = 0;
    for n in 0..24_000 {
        let x = 0.5 * (std::f32::consts::TAU * 997.0 * n as f32 / 48e3).sin();
        let today = play.tick(flutter.tick(record.tick(x)));
        let (a, b) = (plain.tick(x), off.tick(x));
        differ += usize::from(a.to_bits() != today.to_bits()) + usize::from(b.to_bits() != today.to_bits());
    }
    println!("H5 | everything off against E8's chain, 24 000 samples | {differ} differing | budget 0 (bit-identical) | pass");
    assert_eq!(differ, 0);
}

#[test]
fn spacing_loss_matches_eq_13() {
    // A held spacing (full wear of `extra` µm) as the stage's impulse
    // response, against eq. 13 with the head moved that much further
    // from the tape, over the eq. 13 one: the spacing law, e^(−kΔd).
    let head = Machine::TC260.play;
    let thickness = Formulation::FERRIC.thickness;
    let mut worst: f64 = 0.0;
    for ips in [3.75, 7.5, 15.0] {
        let speed = Machine::ips(ips);
        for rate in [44_100.0f64, 48_000.0, 96_000.0] {
            for extra in [0.5, 2.0, 5.0, 10.0, 19.0] {
                if extra * 1e-6 / speed > 1e-4 {
                    continue; // beyond 100 µs of travel: the valid range
                }
                let held = Condition { wear: 1.0, wear_um: extra, ..Condition::NONE };
                let mut s = Spacing::new(held, 1);
                s.set_speed(speed);
                s.prepare(rate as f32);
                let taps: Vec<f64> = (0..2 * s.latency() as usize + 1)
                    .map(|n| f64::from(s.tick(if n == 0 { 1.0 } else { 0.0 })))
                    .collect();
                assert_eq!(s.clamps(), 0);
                let near = Playback::new(head, thickness, speed, Phase::Linear);
                let far = Playback::new(PlayHead { spacing: head.spacing + extra * 1e-6, ..head }, thickness, speed, Phase::Linear);
                let mut case: f64 = 0.0;
                let mut hz = 20.0;
                while hz <= 20_000.0 {
                    let base = near.loss(hz);
                    if base.abs() > 1e-6 {
                        let want = 20.0 * (far.loss(hz) / base).abs().log10();
                        if want > FLOOR_DB {
                            case = case.max((fir_db(&taps, hz, rate) - want).abs());
                        }
                    }
                    hz *= 2f64.powf(1.0 / 24.0);
                }
                worst = worst.max(case);
                assert!(case <= LOSS_DB, "{ips} ips, {rate} Hz, {extra} µm: {case} dB");
            }
        }
    }
    println!("H5 | spacing loss against eq. 13, 0.5–19 µm, 3.75–15 ips, 44.1–96 kHz | worst {worst:.3} dB | budget {LOSS_DB} dB | pass");

    // A splice joint's peak on a 10 kHz tone at 7.5 ips against Wallace's
    // 54.6·d/λ dB: a 20 mm joint (105 ms), its middle 5 ms read.
    let speed = Machine::ips(7.5);
    for depth in [1.0, 3.0, 8.0] {
        let joint = Condition { joint_um: depth, joint_mm: 20.0, ..Condition::NONE };
        let mut s = Spacing::new(joint, 1);
        s.set_speed(speed);
        s.prepare(48_000.0);
        let d = s.latency() as usize;
        let length = (20e-3 / speed * 48_000.0) as usize;
        let tone = |n: usize| (std::f64::consts::TAU * 10_000.0 * n as f64 / 48_000.0).sin();
        let mut y = Vec::new();
        for n in 0..length + 4 * d {
            if n == 2 * d {
                s.splice();
            }
            y.push(f64::from(s.tick(tone(n) as f32)));
        }
        // The joint's middle in the output: begun at 2d, delayed d.
        let middle = 3 * d + length / 2;
        let window = &y[middle - 120..middle + 120];
        let (re, im) = window.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, v)| {
            let w = std::f64::consts::TAU * 10_000.0 * i as f64 / 48_000.0;
            (re + v * w.cos(), im + v * w.sin())
        });
        let got = 20.0 * (2.0 * (re * re + im * im).sqrt() / 240.0).log10();
        let want = -54.6 * depth * 1e-6 / (speed / 10_000.0);
        println!("H5 | joint of {depth} µm at 10 kHz, 7.5 ips | {got:.2} dB against 54.6·d/λ = {want:.2} dB | budget 0.5 dB | pass");
        assert!((got - want).abs() < 0.5, "{depth} µm: {got} vs {want}");
    }
}

#[test]
fn dropouts_have_their_process_statistics() {
    // 2000 s at 8 kHz, ten dropouts a second: about 20 000 events.
    let c = Condition {
        dropouts_per_minute: 600.0,
        depth_um: 1.5,
        duration_ms: 4.0,
        duration_spread: 0.5,
        ..Condition::NONE
    };
    let rate = 8_000.0;
    let mut s = Spacing::new(c, 11);
    s.prepare(rate as f32);
    let seconds = 2_000.0;
    let (mut starts, mut depths, mut logs) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..(seconds * rate) as usize {
        s.tick(0.0);
        if let Some(d) = s.started() {
            starts.push(n as f64 / rate);
            depths.push(d.depth_um);
            logs.push(d.seconds.ln());
        }
    }
    assert_eq!(s.clamps(), 0);
    let n = starts.len() as f64;
    let expect = 10.0 * seconds;
    let rate_z = (n - expect) / expect.sqrt();
    let gaps: Vec<f64> = starts.windows(2).map(|w| w[1] - w[0]).collect();
    let (gap_mean, gap_var) = moments(&gaps);
    let gap_cv = gap_var.sqrt() / gap_mean;
    let (depth_mean, _) = moments(&depths);
    let tail = depths.iter().filter(|&&d| d > 3.0).count() as f64 / n;
    let (log_mean, log_var) = moments(&logs);
    let p = (-2f64).exp();
    println!("H5 | dropouts: {n} in {seconds} s against {expect} | z {rate_z:+.2} | budget 3 | pass");
    println!("H5 | gaps: mean {gap_mean:.5} s against 0.1, CV {gap_cv:.4} against 1 | budget 2 % / 5 % | pass");
    println!("H5 | depth: mean {depth_mean:.4} µm against 1.5, P(> 2·mean) {tail:.4} against {p:.4} | budget 3 SE | pass");
    println!("H5 | duration: ln mean {log_mean:.4} against {:.4}, spread {:.4} against 0.5 | budget 3 SE / 3 % | pass", (4e-3f64).ln(), log_var.sqrt());
    assert!(rate_z.abs() < 3.0, "{rate_z}");
    assert!((gap_mean / 0.1 - 1.0).abs() < 0.02, "{gap_mean}");
    assert!((gap_cv - 1.0).abs() < 0.05, "{gap_cv}");
    assert!((depth_mean - 1.5).abs() < 3.0 * 1.5 / n.sqrt(), "{depth_mean}");
    assert!((tail - p).abs() < 3.0 * (p * (1.0 - p) / n).sqrt(), "{tail}");
    assert!((log_mean - (4e-3f64).ln()).abs() < 3.0 * 0.5 / n.sqrt(), "{log_mean}");
    assert!((log_var.sqrt() / 0.5 - 1.0).abs() < 0.03, "{}", log_var.sqrt());
}

#[test]
fn capstan_drift_has_its_process_statistics() {
    // σ = 0.2 %, τ = 50 ms, the loop's τs = 200 ms, at 1 kHz for 2²²
    // samples: about 20 000 of the loop's time constants.
    let (sigma, tau, servo, rate) = (2e-3, 0.05, 0.2, 1_000.0);
    let mut c = Capstan::new(21).with_drift(Drift { rms: sigma, tau_s: tau }).with_servo(servo);
    c.prepare(rate as f32);
    let base = f64::from(c.latency()) / rate;
    let n = 1 << 22;
    let (mut eps, mut delivered, mut lag) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for _ in 0..n {
        c.tick(0.0);
        eps.push(c.speed_error());
        delivered.push(c.speed_ratio() - 1.0);
        lag.push(c.lag() - base);
    }
    assert_eq!(c.clamps(), 0);
    let (_, var) = moments(&eps);
    let (_, var_delivered) = moments(&delivered);
    let (_, var_lag) = moments(&lag);
    let want_delivered = sigma * sigma * servo / (tau + servo);
    let want_lag = sigma * sigma * tau * servo * servo / (tau + servo);
    let e = var / (sigma * sigma) - 1.0;
    let ed = var_delivered / want_delivered - 1.0;
    let el = var_lag / want_lag - 1.0;
    println!("H5 | drift: speed error variance {:+.2} % of σ² | budget 4 % | pass", 100.0 * e);
    println!("H5 | drift: delivered speed variance {:+.2} % of σ²·τs/(τ + τs) | budget 6 % | pass", 100.0 * ed);
    println!("H5 | drift: lag variance {:+.2} % of σ²·τ·τs²/(τ + τs) | budget 6 % | pass", 100.0 * el);
    assert!(e.abs() < 0.04, "{var}");
    assert!(ed.abs() < 0.06, "{var_delivered} vs {want_delivered}");
    assert!(el.abs() < 0.06, "{var_lag} vs {want_lag}");
    for lag_samples in [25usize, 50, 100] {
        let r = eps[..n - lag_samples].iter().zip(&eps[lag_samples..]).map(|(a, b)| a * b).sum::<f64>()
            / (n - lag_samples) as f64
            / var;
        let want = (-(lag_samples as f64) / (tau * rate)).exp();
        println!("H5 | drift: correlation at {lag_samples} ms {r:.4} against {want:.4} | budget 0.03 | pass");
        assert!((r - want).abs() < 0.03, "{lag_samples}: {r} vs {want}");
    }
}

#[test]
fn capstan_ramps_follow_their_time_constants() {
    let rate = 48_000.0;
    // The motor's curves, sample by sample, against the closed forms.
    for motor in [Motor::STUDIO, Motor::CONSUMER] {
        let mut c = Capstan::new(4).with_motor(motor);
        c.prepare(rate as f32);
        let mut worst: f64 = 0.0;
        c.stop();
        let mut k = 0;
        while !c.is_stopped() {
            c.tick(0.0);
            k += 1;
            let t = k as f64 / rate;
            let want = match motor.stop {
                Stop::Coast { tau_s } => (-t / tau_s).exp(),
                Stop::Brake { seconds } => (1.0 - t / seconds).max(0.0),
            };
            if !c.is_stopped() {
                worst = worst.max((c.motor_ratio() - want).abs());
            }
        }
        let stop_s = k as f64 / rate;
        c.start();
        for k in 1..(10.0 * motor.start_s * rate) as usize {
            c.tick(0.0);
            let want = 1.0 - (-(k as f64) / (rate * motor.start_s)).exp();
            worst = worst.max((c.motor_ratio() - want).abs());
        }
        println!("H5 | motor {motor:?}: stopped in {stop_s:.3} s, curves within {worst:.1e} | budget 1e-9 | pass");
        assert!(worst < 1e-9, "{worst}");
    }

    // A 1 kHz tone through a spin-up from rest: its pitch, period by
    // period from upward zero crossings, against the speed delivered
    // over that period.
    let motor = Motor::CONSUMER;
    let mut c = Capstan::new(4).with_motor(motor);
    c.prepare(rate as f32);
    let d = f64::from(c.latency());
    let total = (rate * 8.0) as usize;
    let (mut y, mut r) = (Vec::with_capacity(total), Vec::with_capacity(total));
    let mut n = 0usize;
    let sine = |n: usize| (std::f64::consts::TAU * 1e3 * n as f64 / rate).sin() as f32;
    c.stop();
    while !c.is_stopped() {
        c.tick(sine(n));
        n += 1;
    }
    c.start();
    for _ in 0..total {
        y.push(f64::from(c.tick(sine(n))));
        r.push(c.speed_ratio());
        n += 1;
    }
    let crossings: Vec<f64> = (1..y.len())
        .filter(|&i| y[i - 1] < 0.0 && y[i] >= 0.0)
        .map(|i| (i - 1) as f64 + y[i - 1] / (y[i - 1] - y[i]))
        .collect();
    let mut worst: f64 = 0.0;
    let mut periods = 0;
    for w in crossings.windows(2) {
        let (a, b) = (w[0].ceil() as usize, w[1].floor() as usize);
        let speed = r[a..=b].iter().sum::<f64>() / (b - a + 1) as f64;
        if speed < 0.2 {
            continue;
        }
        let hz = rate / (w[1] - w[0]);
        worst = worst.max((hz / (1e3 * speed) - 1.0).abs());
        periods += 1;
    }
    let settled = (c.lag() * rate - d).abs();
    println!("H5 | spin-up from rest (τ = {} s): pitch against the speed over {periods} periods, worst {:.3} % | budget 0.5 % | pass", motor.start_s, 100.0 * worst);
    println!("H5 | spin-up: lag {settled:.2e} samples off its base after {:.0} τ | budget 0.01 | pass", 8.0 / motor.start_s);
    assert!(worst < 5e-3, "{worst}");
    assert!(settled < 0.01, "{settled}");
    assert_eq!(c.clamps(), 0);
}
