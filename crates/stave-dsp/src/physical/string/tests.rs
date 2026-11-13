use super::dispersion::{allpass_group, allpass_lag};
use super::*;
use crate::physical::probe::{cents, frequency, hann, peak, t60};
use crate::units::{hz, seconds};
use core::f64::consts::TAU;

const FS: f32 = 48_000.0;

fn string(pitch: f32, b: f64, decay: (f64, f64), position: f64) -> Waveguide {
    let mut s: Waveguide = Waveguide::new(hz(30.0), hz(pitch));
    s.set_inharmonicity(b);
    s.set_decay(seconds(decay.0), seconds(decay.1));
    s.set_position(position);
    s.prepare(FS);
    s
}

/// The string's response to a one-sample strike, `len` samples.
fn struck(s: &mut Waveguide, len: usize) -> Vec<f64> {
    s.reset();
    s.strike(1.0, seconds(0.0));
    (0..len).map(|_| f64::from(s.tick(0.0))).collect()
}

/// `f_k = k f₀ √(1 + B k²)` with `f₀ = f₁/√(1 + B)`.
fn stiff(f1: f64, b: f64, k: f64) -> f64 {
    k * f1 / (1.0 + b).sqrt() * (1.0 + b * k * k).sqrt()
}

#[test]
#[cfg_attr(miri, ignore)]
fn partials_follow_the_stiff_string_formula() {
    let fs = f64::from(FS);
    let mut report = Vec::new();
    // Piano-like B at each pitch (Rauhala & Välimäki's figure 1 range).
    for (pitch, b, sections, budget) in [(65.4f32, 1e-4, 8usize, 1.5), (110.0, 2e-4, 16, 2.0), (261.6, 4e-4, 8, 1.2), (110.0, 4e-4, 16, 5.0)] {
        let mut s: Waveguide = Waveguide::new(hz(30.0), hz(pitch));
        s.set_inharmonicity(b);
        s.set_dispersion_sections(sections);
        s.set_decay(seconds(12.0), seconds(8.0));
        s.set_position(0.13);
        s.prepare(FS);
        let y = struck(&mut s, FS as usize);
        let f1 = f64::from(pitch);
        let (mut inside, mut beyond) = ((0.0f64, 0usize), 0.0f64);
        let mut k = 1usize;
        while stiff(f1, b, k as f64) <= 6_000.0 {
            let want = stiff(f1, b, k as f64);
            let near = peak(&y, fs, want, 0.3 * f1, 2_400, 9_600);
            let got = frequency(&y, fs, near, 2_400, 2_400 + 4_800, 9_600);
            let c = cents(got, want).abs();
            if k <= 16 && want <= 2_500.0 {
                if c > inside.0 {
                    inside = (c, k);
                }
            } else {
                beyond = beyond.max(c);
            }
            k += 1;
        }
        report.push((pitch, b, sections, inside, beyond, cents(16.0 * f1, stiff(f1, b, 16.0)).abs(), budget));
    }
    for (pitch, b, m, (c, at), beyond, unfit, _) in &report {
        eprintln!("String {pitch} Hz, B = {b:e}, {m} sections: partials to the 16th (≤ 2.5 kHz) within {c:.3} cents of k f₀ √(1 + Bk²) (worst at {at}; a flexible string's 16th is {unfit:.1} off); above, to 6 kHz, {beyond:.1}");
    }
    for (pitch, b, _, (c, _), _, unfit, budget) in report {
        assert!(c < budget, "{pitch} Hz, B = {b}: {c:.3} cents");
        assert!(unfit > 5.0 * c, "B = {b}: the formula is only {unfit} cents from harmonic");
    }
}

/// The designed loop's magnitude and group delay (samples) at `w`.
fn loop_at(t: &Tuning, w: f64) -> (f64, f64) {
    let read = crate::primitives::time::interpolators::Lagrange3;
    let phase = |w: f64| {
        let mut lag = 0.0;
        let mut mag = 1.0;
        for d in [f64::from(t.nut), f64::from(t.bridge)] {
            let (re, im) = read.response(d.fract() as f32, w);
            lag += w * d.floor() - im.atan2(re);
            mag *= (re * re + im * im).sqrt();
        }
        let (s, c) = w.sin_cos();
        let p = t.pole;
        lag += (p * s).atan2(1.0 - p * c) + t.dispersion.lag(w);
        mag *= t.gain * (1.0 - p) / (1.0 - 2.0 * p * c + p * p).sqrt();
        (mag, lag)
    };
    let h = 1e-5;
    let (mag, _) = phase(w);
    let group = (phase(w + h).1 - phase(w - h).1) / (2.0 * h);
    (mag, group)
}

#[test]
#[cfg_attr(miri, ignore)]
fn partials_decay_as_the_loss_filter_predicts() {
    let fs = f64::from(FS);
    let mut worst_design = 0.0f64;
    let mut worst_set = 0.0f64;
    for (pitch, b, decay) in [(110.0f32, 0.0, (4.0, 0.8)), (110.0, 4e-4, (4.0, 0.8)), (220.0, 0.0, (2.0, 0.3)), (82.4, 2e-4, (6.0, 1.5))] {
        let mut s = string(pitch, b, decay, 0.09);
        let y = struck(&mut s, (1.1 * decay.0 * fs) as usize);
        let t = s.tuning();
        let f1 = f64::from(pitch);
        for k in 1..=16 {
            // Partials near a comb notch of the contact point are too
            // weak to time.
            if (core::f64::consts::PI * f64::from(k) * 0.09).sin().abs() < 0.3 {
                continue;
            }
            let f = peak(&y, fs, stiff(f1, b, f64::from(k)), 0.3 * f1, 0, 9_600);
            let (mag, group) = loop_at(&t, TAU * f / fs);
            let predicted = 3.0 * group / (-mag.log10() * fs);
            // Over its first 30 dB, in windows of eight periods.
            let len = (8.0 * fs / f1) as usize;
            let measured = t60(&y, fs, f, len, len, len + (0.5 * predicted * fs) as usize);
            if (measured / predicted - 1.0).abs() > 0.02 {
                eprintln!("  {pitch} Hz B {b}: partial {k} measured {measured:.3} s, predicted {predicted:.3} s (|H| {mag}, τg {group})");
            }
            worst_design = worst_design.max((measured / predicted - 1.0).abs());
            if k == 1 {
                worst_set = worst_set.max((measured / decay.0 - 1.0).abs());
            }
            if k == 8 {
                worst_set = worst_set.max((measured / decay.1 - 1.0).abs());
            }
        }
    }
    eprintln!(
        "String decay: partials 1–16 within {:.2} % of the loop's prediction; fundamental and 8th within {:.2} % of their settings",
        100.0 * worst_design,
        100.0 * worst_set
    );
    assert!(worst_design < 0.02, "{:.2} % from the design", 100.0 * worst_design);
    assert!(worst_set < 0.02, "{:.2} % from the settings", 100.0 * worst_set);
}

#[test]
#[cfg_attr(miri, ignore)]
fn pluck_position_combs_the_partials() {
    let fs = f64::from(FS);
    let f1 = 110.0;
    let level = |beta: f64| {
        let mut s = string(f1 as f32, 0.0, (10.0, 6.0), beta);
        let y = struck(&mut s, FS as usize / 2);
        (1..=20).map(|k| hann(&y, fs, f1 * f64::from(k), 0, FS as usize / 2).0).collect::<Vec<_>>()
    };
    let reference = 1.0 / 23.0;
    let (fifth, base) = (level(0.2), level(reference));
    let (mut worst, mut shallowest) = (0.0f64, f64::NEG_INFINITY);
    for k in 1..=20 {
        let comb = |beta: f64| (core::f64::consts::PI * f64::from(k) * beta).sin().abs();
        let measured = fifth[k as usize - 1] / base[k as usize - 1];
        if k % 5 == 0 {
            // A notch: its partial against what an uncombed one would be.
            let depth = 20.0 * (measured * comb(reference)).log10();
            shallowest = shallowest.max(depth);
        } else {
            let err = 20.0 * (measured / (comb(0.2) / comb(reference))).log10();
            worst = worst.max(err.abs());
        }
    }
    eprintln!("Pluck position 1/5: partials off the notches within {worst:.3} dB of |sin πkβ|, notches (k = 5, 10, 15, 20) at most {shallowest:.1} dB");
    assert!(worst < 0.2, "{worst} dB");
    assert!(shallowest < -40.0, "a notch only {shallowest} dB deep");

    // A pluck (a step in force) is the strike's spectrum over k: 1/k.
    let mut s = string(f1 as f32, 0.0, (10.0, 6.0), reference);
    s.reset();
    s.pluck(1.0, seconds(0.0));
    let y: Vec<f64> = (0..FS as usize / 2).map(|_| f64::from(s.tick(0.0))).collect();
    let plucked: Vec<f64> = (1..=12).map(|k| hann(&y, fs, f1 * f64::from(k), 0, FS as usize / 2).0).collect();
    let tilt = (1..12).map(|k| 20.0 * ((plucked[k] / base[k]) / (plucked[0] / base[0]) * (k + 1) as f64).log10()).fold(0.0f64, |a, e| a.max(e.abs()));
    eprintln!("Pluck against strike: 1/k within {tilt:.3} dB to the 12th");
    assert!(tilt < 0.2, "{tilt} dB");
}

#[test]
#[cfg_attr(miri, ignore)]
fn bowed_string_settles_into_helmholtz_motion() {
    let fs = f64::from(FS);
    let mut report = Vec::new();
    for (pitch, beta, force, velocity) in [(196.0f32, 0.1, 0.5, 0.2), (196.0, 0.15, 0.6, 0.2), (293.7, 0.08, 0.4, 0.15)] {
        let mut s = string(pitch, 0.0, (2.0, 0.4), beta);
        s.set_bow(Some(Bow { force, velocity }));
        let settle = FS as usize;
        let run = FS as usize;
        let (mut stuck, mut onsets, mut slip_sum, mut slip_n) = (0usize, Vec::new(), 0.0f64, 0usize);
        let mut was = true;
        for i in 0..settle + run {
            s.tick(0.0);
            if i < settle {
                was = s.sticking();
                continue;
            }
            let now = s.sticking();
            if now {
                stuck += 1;
            } else {
                slip_sum += f64::from(s.contact_velocity());
                slip_n += 1;
                if was {
                    onsets.push(i);
                }
            }
            was = now;
        }
        let slip = 1.0 - stuck as f64 / run as f64;
        let period = (onsets[onsets.len() - 1] - onsets[0]) as f64 / (onsets.len() - 1) as f64;
        let off = cents(fs / period, f64::from(pitch));
        let slip_velocity = slip_sum / slip_n as f64;
        let ideal = -velocity * (1.0 - beta) / beta;
        report.push((pitch, beta, slip, off, slip_velocity / ideal - 1.0, onsets.len()));
    }
    for (pitch, beta, slip, off, v, n) in &report {
        eprintln!("Bowed {pitch} Hz at β = {beta}: slip fraction {slip:.4}, period {off:+.2} cents ({n} slips), slip velocity {:+.1} % of −v_b(1 − β)/β", 100.0 * v);
    }
    for (pitch, beta, slip, off, v, n) in report {
        // One slip a period: Helmholtz motion, not a higher regime.
        assert!((n as f64 - f64::from(pitch)).abs() <= 2.0, "{pitch} Hz: {n} slips in a second");
        assert!((slip - beta).abs() < 0.02, "{pitch} Hz: slip fraction {slip} against β {beta}");
        assert!(off.abs() < 5.0, "{pitch} Hz: period {off} cents off");
        assert!(v.abs() < 0.15, "{pitch} Hz: slip velocity {:.1} % off", 100.0 * v);
    }
}

/// The partials a loop of `line` samples and `disp` would ring at, by
/// Newton on its phase: `ω line + M lag(ω) = 2πk`.
fn predicted(disp: &Dispersion, line: f64, w1: f64, k: usize) -> f64 {
    let mut w = w1 * k as f64;
    for _ in 0..30 {
        let f = w * line + disp.lag(w) - TAU * k as f64;
        let g = line + disp.sections as f64 * allpass_group(disp.coefficient, w);
        w -= f / g;
    }
    w
}

#[test]
fn rauhala_closed_form_is_the_fits_start() {
    // Rauhala & Välimäki's figure 5(b): C2 (65.406 Hz), B = 1e-4, at
    // 44.1 kHz, eight sections.
    let (fs, f0, b, m) = (44_100.0f64, 65.406f64, 1e-4f64, 8usize);
    let w0 = TAU * f0 / fs;
    let wk = |k: f64| k * w0 * (1.0 + b * k * k).sqrt();
    let w1 = wk(1.0);
    let design = |disp: Dispersion| -> (f64, f64) {
        let line = (TAU - disp.lag(w1)) / w1;
        let (mut worst, mut sum, mut n) = (0.0f64, 0.0, 0.0);
        let mut k = 2;
        while wk(k as f64) / TAU * fs <= 1_500.0 {
            let c = cents(predicted(&disp, line, w1, k), wk(k as f64));
            worst = worst.max(c.abs());
            sum += c * c;
            n += 1.0;
            k += 1;
        }
        (worst, (sum / n).sqrt())
    };
    let closed = Dispersion::rauhala(f0, b, m);
    let partials = (1..=48).take_while(|&k| wk(k as f64) / TAU * fs <= 1_500.0).count();
    let fitted = Dispersion::fit(w0, b, m, partials, 4.0, |_| 0.0, |_| 0.0);
    let (cw, cr) = design(closed);
    let (fw, fr) = design(fitted);
    eprintln!("Dispersion, C2, B = 1e-4, 8 sections, to 1.5 kHz: closed form a = {:.4}, worst {cw:.2} cents (rms {cr:.2}); fitted a = {:.4}, worst {fw:.2} (rms {fr:.2})", closed.coefficient, fitted.coefficient);
    // Their 0.5 % limit, ±8.39 cents.
    assert!(cw < 8.39, "the closed form is {cw} cents off");
    assert!(fr <= cr, "the fit is worse than its start");
    assert!(closed.coefficient < 0.0 && (fitted.coefficient - closed.coefficient).abs() < 0.2);
    // The lag and group delay agree with each other.
    let (a, w) = (-0.6, 0.3);
    let numeric = (allpass_lag(a, w + 1e-6) - allpass_lag(a, w - 1e-6)) / 2e-6;
    assert!((numeric - allpass_group(a, w)).abs() < 1e-6);
}

#[test]
fn tails_cover_the_ring() {
    let mut s = string(440.0, 0.0, (0.3, 0.1), 0.2);
    let y = struck(&mut s, 48_000);
    let Some(n) = s.tail().to_samples() else { panic!() };
    let last = y.iter().rposition(|v| v.abs() > 1e-6).unwrap();
    assert!(last <= n as usize, "rang {last}, reported {n}");
}
