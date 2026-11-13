//! The plate reverb (DSP.md H3) against the DAFx-26 parameter-estimation
//! challenge's model as its document states it (Ducceschi & Gabrielli
//! 2026: eq. 8's modes, eq. 11's loss, eq. 13's update and its 10 kHz
//! enumeration), the loss law's decay per octave measured on the impulse
//! response (ISO 3382 T30), and the EMT 140's damper range.
//!
//! The challenge's simulator itself was not run (executing its code was
//! not permitted here); the equations below are written from the
//! document, independently of `physical::plate`.

use std::f64::consts::{PI, TAU};

use stave_dsp::physical::plate::{Geometry, Loss, Material, Output, Plate, Point};
use stave_dsp::units::{hz, seconds};
use stave_dsp::Stage;
use stave_measure::tail::t60_band_s;

/// One plate of the challenge's parameter set (Table I's symbols).
struct Challenge {
    lx: f64,
    ly: f64,
    h: f64,
    t0: f64,
    rho: f64,
    e: f64,
    nu: f64,
    tau0: f64,
    tau1: f64,
    input: (f64, f64),
    output: (f64, f64),
    rate: f64,
}

/// A mode as the document computes it: indices, `Ω`, `σ`, `b_m`.
type Row = (u32, u32, f64, f64, f64);

impl Challenge {
    fn modes(&self) -> Vec<Row> {
        let d = self.e * self.h.powi(3) / (12.0 * (1.0 - self.nu * self.nu));
        let mu = self.rho * self.h;
        let max = TAU * 10_000.0;
        // Eq. 11 with Ω0 = 0, Ω1 = 1000π.
        let w1 = 1_000.0 * PI;
        let eta0 = 3.0 * 10f64.ln() / self.tau0;
        let eta1 = 3.0 * 10f64.ln() / (w1 * w1) * (self.tau0 - self.tau1) / (self.tau0 * self.tau1);
        let bound = |l: f64| {
            (l / PI * ((-self.t0 + (self.t0 * self.t0 + 4.0 * max * max * mu * d).sqrt()) / (2.0 * d)).sqrt()).floor() as u32
        };
        let phi = |m: u32, n: u32, p: (f64, f64)| {
            2.0 / (self.lx * self.ly).sqrt() * (f64::from(m) * PI * p.0).sin() * (f64::from(n) * PI * p.1).sin()
        };
        let t = 1.0 / self.rate;
        let mut rows = Vec::new();
        for m in 1..=bound(self.lx) {
            for n in 1..=bound(self.ly) {
                let g = (f64::from(m) * PI / self.lx).powi(2) + (f64::from(n) * PI / self.ly).powi(2);
                // Eq. 8.
                let omega = (g * (self.t0 + d * g) / mu).sqrt();
                if omega > max {
                    continue;
                }
                let sigma = eta0 + eta1 * omega * omega;
                // Eq. 13.
                let b = 4.0 * t * t * phi(m, n, self.input) * phi(m, n, self.output) * (-sigma * t).exp()
                    / (mu * self.lx * self.ly);
                rows.push((m, n, omega, sigma, b));
            }
        }
        rows
    }

    fn plate(&self) -> Plate {
        let geometry = Geometry {
            width: self.lx,
            height: self.ly,
            thickness: self.h,
            tension: self.t0,
            material: Material { density: self.rho, young: self.e, poisson: self.nu },
        };
        let loss = Loss::two_point(hz(0.0), seconds(self.tau0), hz(500.0), seconds(self.tau1)).unwrap();
        let mut p = Plate::new(geometry, loss);
        p.set_bandwidth(hz(10_000.0));
        p.set_drive(Point::new(self.input.0, self.input.1));
        p.set_pickup(0, Point::new(self.output.0, self.output.1));
        p.prepare(self.rate as f32);
        p
    }
}

const PLATES: [Challenge; 3] = [
    // The simulator's default plate.
    Challenge {
        lx: 0.5,
        ly: 1.1,
        h: 0.001,
        t0: 0.01,
        rho: 2_430.0,
        e: 6.7e10,
        nu: 0.25,
        tau0: 6.0,
        tau1: 1.0,
        input: (0.1, 0.1),
        output: (0.61, 0.61),
        rate: 44_100.0,
    },
    // Inside Table I's ranges, its fixed loss and input point.
    Challenge {
        lx: 1.0,
        ly: 2.37,
        h: 0.003,
        t0: 400.0,
        rho: 7_850.0,
        e: 2.0e11,
        nu: 0.25,
        tau0: 6.0,
        tau1: 2.0,
        input: (0.335, 0.467),
        output: (0.8, 0.63),
        rate: 44_100.0,
    },
    // An EMT 140-sized steel plate at 48 kHz.
    Challenge {
        lx: 2.0,
        ly: 1.0,
        h: 0.0005,
        t0: 1_000.0,
        rho: 7_850.0,
        e: 2.0e11,
        nu: 0.3,
        tau0: 6.0,
        tau1: 2.0,
        input: (0.447, 0.513),
        output: (0.219, 0.673),
        rate: 48_000.0,
    },
];

#[test]
fn modes_match_the_dafx26_model() {
    for c in &PLATES {
        let want = c.modes();
        let p = c.plate();
        assert_eq!(p.overdamped(), 0);
        assert_eq!(p.modes().len(), want.len(), "mode count");
        let t = 1.0 / c.rate;
        let mu = c.rho * c.h;
        let mut worst = [0.0f64; 3];
        for &(m, n, omega, sigma, b) in &want {
            let got = p.modes().iter().find(|x| x.m == m && x.n == n).expect("mode present");
            // The update's gain from the model's own mode: eq. 13.
            let gain = 4.0 * t * t * got.drive * got.pickups[0] * (-got.sigma * t).exp() / (mu * c.lx * c.ly);
            let errs = [
                (got.natural / omega - 1.0).abs(),
                (got.sigma / sigma - 1.0).abs(),
                if b == 0.0 { gain.abs() } else { (gain / b - 1.0).abs() },
            ];
            for (w, e) in worst.iter_mut().zip(errs) {
                *w = w.max(e);
            }
        }
        assert!(worst[0] < 1e-12 && worst[1] < 1e-12 && worst[2] < 1e-9, "{worst:?}");
        eprintln!(
            "{} modes ({} × {} m): frequency {:.1e}, decay {:.1e}, gain {:.1e}",
            want.len(),
            c.lx,
            c.ly,
            worst[0],
            worst[1],
            worst[2]
        );
    }
}

/// T30 of the energy-decay curve `E(t) = Σ e_m e^(−2σ_m t)` (a mode's
/// energy weight and decay rate), fitted between −5 and −35 dB as
/// ISO 3382 does.
fn predicted_t30(modes: &[(f64, f64)]) -> f64 {
    let total: f64 = modes.iter().map(|m| m.0).sum();
    let db = |t: f64| 10.0 * (modes.iter().map(|&(e, s)| e * (-2.0 * s * t).exp()).sum::<f64>() / total).log10();
    let dt = 1e-3;
    let points: Vec<(f64, f64)> = (0..200_000)
        .map(|i| i as f64 * dt)
        .map(|t| (t, db(t)))
        .skip_while(|p| p.1 > -5.0)
        .take_while(|p| p.1 > -35.0)
        .collect();
    let n = points.len() as f64;
    let (sx, sy) = points.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (sxx, sxy) = points.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 * p.0, a.1 + p.0 * p.1));
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    -60.0 / slope
}

/// The octave band `t60_band_s` filters with (sixth-order Butterworth
/// edges at `centre/√2` and `centre·√2`), power.
fn band(f: f64, centre: f64) -> f64 {
    let (lo, hi) = (centre / 2f64.sqrt(), centre * 2f64.sqrt());
    let hp = (f / lo).powi(12) / (1.0 + (f / lo).powi(12));
    hp / (1.0 + (f / hi).powi(12))
}

/// A plate's predicted T30 in the octave about `centre`, from its modes:
/// each mode's velocity weight `|w|²/2` over `2σ` (its energy from `t`
/// on), through the band.
fn predicted_band(p: &Plate, centre: f64) -> f64 {
    let mass = p.geometry().mass();
    let modes: Vec<(f64, f64)> = p
        .modes()
        .iter()
        .map(|m| {
            let (w, s) = (m.damped(), m.sigma);
            let a = m.drive * m.pickups[0] / (mass * w);
            let weight = a * a * (w * w + s * s) / 2.0;
            (band(w / TAU, centre) * weight / (2.0 * s), s)
        })
        .collect();
    predicted_t30(&modes)
}

fn impulse_response(p: &mut Plate, seconds: f64, rate: f64) -> Vec<f32> {
    let mut y = vec![0.0f32; (seconds * rate) as usize];
    y[0] = 1.0;
    p.process(&mut y);
    y
}

#[test]
fn octave_decay_follows_the_loss_law() {
    let rate = 48_000.0;
    let loss = Loss::two_point(hz(0.0), seconds(4.0), hz(4_000.0), seconds(0.4)).unwrap();
    let geometry = Geometry { width: 1.0, height: 0.7, thickness: 1e-3, tension: 200.0, material: Material::STEEL };
    let mut p = Plate::new(geometry, loss);
    p.set_output(Output::Velocity);
    p.set_bandwidth(hz(6_000.0));
    p.prepare(rate as f32);
    let y = impulse_response(&mut p, 4.0, rate);
    let mut worst: f64 = 0.0;
    for centre in [125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0] {
        let measured = t60_band_s(&y, rate, centre).expect("decays 35 dB");
        let predicted = predicted_band(&p, centre);
        let law = loss.t60(centre).get();
        let err = (measured / predicted - 1.0).abs();
        worst = worst.max(err);
        eprintln!(
            "{centre} Hz: T30 {measured:.3} s, predicted {predicted:.3} s ({:+.1} %), the law at the centre {law:.3} s",
            100.0 * (measured / predicted - 1.0)
        );
        assert!(err < 0.03, "{centre} Hz: {measured} vs {predicted}");
    }
    eprintln!("octave T30 within {:.1} % of the loss law's prediction", 100.0 * worst);
}

#[test]
fn emt140_damper_spans_its_published_range() {
    // The EMT 140's damper sets its decay at 500 Hz from about 0.8 to 5 s
    // (the unit's stated range, as the research notes record it; the
    // factory manual is not in hand). Measured in the 500 Hz octave.
    let rate = 48_000.0;
    for t in [0.8, 2.5, 5.0] {
        let mut p = Plate::emt140(seconds(t));
        p.set_bandwidth(hz(1_500.0));
        p.prepare(rate as f32);
        let y = impulse_response(&mut p, 0.8 * t + 0.5, rate);
        let measured = t60_band_s(&y, rate, 500.0).expect("decays 35 dB");
        let predicted = predicted_band(&p, 500.0);
        eprintln!("damper at {t} s: T30 {measured:.3} s at 500 Hz (predicted {predicted:.3} s)");
        assert!((measured / predicted - 1.0).abs() < 0.05, "{measured} vs {predicted}");
        assert!((measured / t - 1.0).abs() < 0.05, "{measured} vs the setting {t}");
    }
}
