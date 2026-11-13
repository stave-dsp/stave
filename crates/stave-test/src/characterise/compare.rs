//! A model's table against a reference's, characteristic by
//! characteristic (DSP.md C1–C10): each gives its worst error and where.
//!
//! Errors are judged as the measurement design states (principle 6):
//! a harmonic or product in dB relative to *that* component, both sides
//! held at an absolute floor of −120 dBc so that two inaudible
//! components agree; phases in degrees, compared only where the
//! reference's component stands above −80 dBc; responses in dB and
//! degrees. Aliasing is the model's inharmonic energy in excess of the
//! reference's (which holds any unsettled leakage both share), in dBc.

use stave_measure::suite::analysis::{gain, Bla, Harmonics, Recovery, Spectrum};
use stave_measure::suite::multisine::Multisine;
use stave_measure::suite::{db, divide, wrap, Complex};

use super::plan::{Kind, Plan, Run, Tier, HARMONICS, MULTISINE_PERIOD};
use super::table::{Measured, Table};

/// The absolute floor, re the fundamental: −120 dBc.
pub const FLOOR: f64 = 1e-6;
/// Phases are compared above −80 dBc, where a residual at the floor moves
/// a component's phase by at most 0.6°.
pub const PHASE_FLOOR: f64 = 1e-4;

/// A characteristic of the matrix.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    /// C1: DC node voltages, in volts (a supply: B+ at each load).
    OperatingPoint,
    /// C2: the small-signal response, dB and degrees.
    Response,
    /// C2: group delay from 500 Hz to 4 kHz, seconds (the audibility
    /// budget is 0.2 ms; Liski et al. 2021).
    GroupDelay,
    /// C3: the best linear approximation at each level, dB and degrees.
    Bla,
    /// C3: even- and odd-order distortion on the detection bins, dB.
    Detection,
    /// C4: the fundamental's gain and phase at each level.
    Compression,
    /// C5: each harmonic H2–H15 in dB re itself and in (shape) phase.
    Harmonics,
    /// C6: THD+N in the AES17 band (20 Hz – 20 kHz), unweighted, dB.
    ThdN,
    /// C7: each intermodulation product, dB re itself and degrees.
    Intermodulation,
    /// C8: bias shift (volts) and the probe's recovery (dB) after a burst.
    Dynamics,
    /// C10: inharmonic energy on empty bins, dBc — the model's alone,
    /// held to a ceiling.
    Aliasing,
    /// C10 by audibility: the model's A-weighted noise-to-mask ratio at
    /// a shipping rate, dB (Kahles et al. 2019; audible above −10 dB) —
    /// its own, held to a ceiling.
    Anmr,
    /// R16, the null by audibility: the residual (reference − model,
    /// level and latency aligned) under the reference as its masker —
    /// the worst run's total NMR, dB, and the worst band's
    /// ([`super::audibility`]). Inaudible at −10 dB and below.
    Audibility,
}

impl Check {
    pub const ALL: [Check; 13] = [
        Check::OperatingPoint,
        Check::Response,
        Check::GroupDelay,
        Check::Bla,
        Check::Detection,
        Check::Compression,
        Check::Harmonics,
        Check::ThdN,
        Check::Intermodulation,
        Check::Dynamics,
        Check::Aliasing,
        Check::Anmr,
        Check::Audibility,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Check::OperatingPoint => "C1 operating point",
            Check::Response => "C2 response",
            Check::GroupDelay => "C2 group delay",
            Check::Bla => "C3 BLA",
            Check::Detection => "C3 detection bins",
            Check::Compression => "C4 compression",
            Check::Harmonics => "C5 harmonics",
            Check::ThdN => "C6 THD+N 20k flat",
            Check::Intermodulation => "C7 IMD",
            Check::Dynamics => "C8 dynamics",
            Check::Aliasing => "C10 aliasing",
            Check::Anmr => "C10 ANMR",
            Check::Audibility => "R16 null NMR",
        }
    }

    /// The unit of the first error (the second, where there is one, is
    /// degrees — or dB for dynamics, the worst band's NMR for audibility).
    pub fn unit(self) -> &'static str {
        match self {
            Check::OperatingPoint => "V",
            Check::Dynamics => "V",
            Check::Aliasing => "dBc",
            Check::Anmr | Check::Audibility => "dB NMR",
            Check::GroupDelay => "s",
            _ => "dB",
        }
    }

    pub fn second_unit(self) -> Option<&'static str> {
        match self {
            Check::Response
            | Check::Bla
            | Check::Compression
            | Check::Harmonics
            | Check::Intermodulation => Some("°"),
            Check::Dynamics => Some("dB"),
            Check::Audibility => Some("dB NMR"),
            _ => None,
        }
    }
}

/// The worst error of one check, and where.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worst {
    pub error: f64,
    pub at: String,
}

impl Worst {
    fn offer(&mut self, error: f64, at: impl FnOnce() -> String) {
        if error > self.error || (error.is_nan() && !self.error.is_nan()) {
            self.error = error;
            self.at = at();
        }
    }
}

/// One check's result at one rate: the worst first error, the worst
/// second (degrees, or dB for dynamics), and how many points it read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measurement {
    pub first: Worst,
    pub second: Worst,
    pub points: usize,
}

fn floored_db(x: f64) -> f64 {
    db(x.max(FLOOR))
}

fn degrees(a: f64) -> f64 {
    wrap(a).abs().to_degrees()
}

/// Compare `model` to `reference` for `check` over `plan`'s `tier`.
/// `None` where the check does not apply (no runs of its kind, or no
/// data on one side).
pub fn compare(
    check: Check,
    plan: &Plan,
    tier: Tier,
    reference: &Table,
    model: &Table,
) -> Option<Measurement> {
    let mut m = Measurement::default();
    let pairs = || {
        plan.runs(tier).filter_map(|run| {
            Some((run, reference.runs.get(&run.id)?, model.runs.get(&run.id)?))
        })
    };
    match check {
        Check::OperatingPoint => {
            for (node, r) in &reference.op {
                if let Some((_, v)) = model.op.iter().find(|(n, _)| n == node) {
                    m.points += 1;
                    m.first.offer((v - r).abs(), || format!("node {node}: {r:.9} V vs {v:.9} V"));
                }
            }
            for (run, r, md) in pairs() {
                if let Kind::Load { .. } = run.kind {
                    let (r, v) = (dc(&r.periods[0]), dc(&md.periods[0]));
                    m.points += 1;
                    m.first.offer((v - r).abs(), || format!("{}: B+ {r:.6} V vs {v:.6} V", run.id));
                }
            }
        }
        Check::Response => {
            for (hz, r) in &reference.ac {
                if let Some((_, v)) = model.ac.iter().find(|(h, _)| (h / hz - 1.0).abs() < 1e-9) {
                    m.points += 1;
                    let ratio = divide(*v, *r);
                    m.first.offer(db(ratio.norm()).abs(), || {
                        format!("{hz:.1} Hz: {:.6} dB vs {:.6} dB", db(r.norm()), db(v.norm()))
                    });
                    m.second.offer(degrees(ratio.arg()), || {
                        format!(
                            "{hz:.1} Hz: {:.4}° vs {:.4}°",
                            r.arg().to_degrees(),
                            v.arg().to_degrees()
                        )
                    });
                }
            }
        }
        Check::GroupDelay => {
            let (gr, gm) = (group_delay(&reference.ac), group_delay(&model.ac));
            for ((hz, r), (_, v)) in gr.iter().zip(&gm) {
                m.points += 1;
                m.first.offer((v - r).abs(), || format!("{hz:.0} Hz: {r:.4e} s vs {v:.4e} s"));
            }
        }
        Check::Bla | Check::Detection => {
            let ms = Multisine::odd_odd(MULTISINE_PERIOD, 20.0, 20_000.0, 3.0);
            for (run, r, md) in pairs() {
                let Kind::Multisine { .. } = run.kind else { continue };
                let (rs, ms_) = (&r.periods[0], &md.periods[0]);
                if check == Check::Bla {
                    // Compared once per level, averaged over its realisations.
                    let Kind::Multisine { offset_db, seed, .. } = run.kind else { continue };
                    if seed != 1 {
                        continue;
                    }
                    let (Some(br), Some(bm)) = (
                        averaged_bla(plan, tier, reference, offset_db),
                        averaged_bla(plan, tier, model, offset_db),
                    ) else {
                        continue;
                    };
                    let count = plan
                        .runs(tier)
                        .filter(|r| matches!(r.kind, Kind::Multisine { offset_db: o, .. } if o == offset_db))
                        .count();
                    for (k, gm) in &bm {
                        let Some((_, gr)) = br.iter().find(|(b, _)| b == k) else { continue };
                        m.points += 1;
                        let ratio = divide(*gm, *gr);
                        let hz = run.period().hz(*k);
                        let at = || format!("multisine {offset_db:+} dB ({count} realisations) at {hz:.0} Hz");
                        m.first.offer(db(ratio.norm()).abs(), at);
                        m.second.offer(degrees(ratio.arg()), at);
                    }
                } else {
                    let (er, or) = stave_measure::suite::analysis::detection(&ms, rs);
                    let (em, om) = stave_measure::suite::analysis::detection(&ms, ms_);
                    m.points += 2;
                    m.first.offer((floored_db(em) - floored_db(er)).abs(), || {
                        format!("{} even: {:.2} vs {:.2} dB", run.id, db(er), db(em))
                    });
                    m.first.offer((floored_db(om) - floored_db(or)).abs(), || {
                        format!("{} odd: {:.2} vs {:.2} dB", run.id, db(or), db(om))
                    });
                }
            }
        }
        Check::Compression => {
            for (run, r, md) in pairs() {
                let Kind::Tone { bin, .. } = run.kind else { continue };
                let (Some(cr), Some(cm)) = (r.periods[0].get(bin), md.periods[0].get(bin)) else {
                    continue;
                };
                m.points += 1;
                let tone = &run.excitation.tones[0];
                let (gr, gm) = (gain(tone, cr), gain(tone, cm));
                let ratio = divide(gm, gr);
                m.first.offer(db(ratio.norm()).abs(), || {
                    format!("{}: {:.6} vs {:.6} dB", run.id, db(gr.norm()), db(gm.norm()))
                });
                m.second.offer(degrees(ratio.arg()), || {
                    format!(
                        "{}: {:.4}° vs {:.4}°",
                        run.id,
                        gr.arg().to_degrees(),
                        gm.arg().to_degrees()
                    )
                });
            }
        }
        Check::Harmonics => {
            for (run, r, md) in pairs() {
                let (bin, count) = match run.kind {
                    Kind::Tone { bin, .. } => (bin, HARMONICS),
                    Kind::Load { ripple } => (ripple, 10),
                    _ => continue,
                };
                harmonics(&mut m, run, bin, count, &r.periods[0], &md.periods[0]);
            }
        }
        Check::ThdN => {
            for (run, r, md) in pairs() {
                let (Some(tr), Some(tm)) = (r.thdn, md.thdn) else { continue };
                m.points += 1;
                m.first.offer((floored_db(tm) - floored_db(tr)).abs(), || {
                    format!("{}: {:.3} vs {:.3} dB", run.id, db(tr), db(tm))
                });
            }
        }
        Check::Intermodulation => {
            for (run, r, md) in pairs() {
                let Kind::TwoTone { low, high, .. } = run.kind else { continue };
                let (rs, ms_) = (&r.periods[0], &md.periods[0]);
                let scale = rs
                    .magnitude(low)
                    .unwrap_or(0.0)
                    .max(rs.magnitude(high).unwrap_or(0.0));
                for &k in &run.bins {
                    if k == 0 || k == low || k == high {
                        continue;
                    }
                    let (Some(cr), Some(cm)) = (rs.get(k), ms_.get(k)) else { continue };
                    component(&mut m, &run.id, k, cr, cm, scale, cr.arg(), cm.arg());
                }
            }
        }
        Check::Dynamics => {
            for (run, r, md) in pairs() {
                let Kind::Burst { probe, .. } = run.kind else { continue };
                let (rr, rm) = (Recovery::of(&r.periods, probe), Recovery::of(&md.periods, probe));
                for j in 0..rr.dc.len().min(rm.dc.len()) {
                    m.points += 1;
                    m.first.offer((rm.dc[j] - rr.dc[j]).abs(), || {
                        format!("{} period {j}: mean {:.6} vs {:.6} V", run.id, rr.dc[j], rm.dc[j])
                    });
                    let e = (rm.probe_db[j] - rr.probe_db[j]).abs();
                    m.second.offer(e, || {
                        format!(
                            "{} period {j}: probe {:.4} vs {:.4} dB",
                            run.id, rr.probe_db[j], rm.probe_db[j]
                        )
                    });
                }
            }
        }
        Check::Anmr => {
            m.first.error = f64::NEG_INFINITY;
            for (run, _, md) in pairs() {
                let Some(a) = md.anmr else { continue };
                m.points += 1;
                m.first.offer(a, || format!("{}: ANMR {a:.1} dB", run.id));
            }
        }
        Check::Audibility => {
            let playback = super::audibility::playback(plan, reference, super::audibility::ANALYSIS_RATE);
            let a = super::audibility::Audibility::of(plan, tier, reference, model, playback)?;
            let aligned = format!(
                "aligned {:+.4} dB, {:+.3} µs",
                a.alignment.gain_db(),
                a.alignment.delay * 1e6
            );
            m.first.error = f64::NEG_INFINITY;
            m.second.error = f64::NEG_INFINITY;
            for r in &a.runs {
                m.points += 1;
                m.first.offer(r.nmr.total_db, || format!("{}: total {:.1} dB, {aligned}", r.run, r.nmr.total_db));
                m.second.offer(r.nmr.max_db, || format!("{}: {:.1} dB at {:.0} Hz", r.run, r.nmr.max_db, r.nmr.max_hz));
            }
        }
        Check::Aliasing => {
            m.first.error = f64::NEG_INFINITY;
            for (run, r, md) in pairs() {
                let Some(fm) = md.floor else { continue };
                m.points += 1;
                let fr = r.floor.unwrap_or(0.0);
                let excess = (fm * fm - fr * fr).max(0.0).sqrt();
                let level = floored_db(excess);
                m.first.offer(level, || {
                    format!(
                        "{}: {:.1} dBc over the reference's {:.1} dBc",
                        run.id,
                        db(fm),
                        db(fr)
                    )
                });
            }
        }
    }
    (m.points > 0).then_some(m)
}

/// The BLA at one level, averaged over its realisations in `table`
/// (each realisation's `Y/U`, then the mean): `(bin, response)`.
pub fn averaged_bla(
    plan: &Plan,
    tier: Tier,
    table: &Table,
    offset_db: f64,
) -> Option<Vec<(u32, Complex<f64>)>> {
    let mut sum: Vec<(u32, Complex<f64>)> = Vec::new();
    let mut count = 0.0;
    for run in plan.runs(tier) {
        let Kind::Multisine { offset_db: o, .. } = run.kind else { continue };
        if o != offset_db {
            continue;
        }
        let measured = table.runs.get(&run.id)?;
        let bla = Bla::of(&run.excitation, &measured.periods[0]);
        if sum.is_empty() {
            sum = bla.bins.iter().map(|&k| (k, Complex::new(0.0, 0.0))).collect();
        }
        for (k, g) in bla.bins.iter().zip(&bla.response) {
            if let Some(s) = sum.iter_mut().find(|(b, _)| b == k) {
                s.1 = s.1 + *g;
            }
        }
        count += 1.0;
    }
    (count > 0.0).then(|| sum.into_iter().map(|(k, s)| (k, s.scale(1.0 / count))).collect())
}

/// Group delay between neighbouring response points from 500 Hz to
/// 4 kHz: `(geometric-mean Hz, seconds)`.
fn group_delay(ac: &[(f64, Complex<f64>)]) -> Vec<(f64, f64)> {
    let mut points: Vec<&(f64, Complex<f64>)> = ac.iter().filter(|(hz, _)| (500.0..=4_000.0).contains(hz)).collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
        .windows(2)
        .map(|w| {
            let (f0, c0) = w[0];
            let (f1, c1) = w[1];
            let dphi = wrap(divide(*c1, *c0).arg());
            ((f0 * f1).sqrt(), -dphi / (std::f64::consts::TAU * (f1 - f0)))
        })
        .collect()
}

fn dc(s: &Spectrum) -> f64 {
    s.get(0).map_or(0.0, |c| c.re)
}

fn harmonics(m: &mut Measurement, run: &Run, bin: u32, count: u32, r: &Spectrum, md: &Spectrum) {
    let (hr, hm) = (Harmonics::of(r, bin, count), Harmonics::of(md, bin, count));
    let Some(fundamental) = hr.get(1) else { return };
    let scale = fundamental.norm();
    for h in 2..=count {
        let (Some(cr), Some(cm)) = (hr.get(h), hm.get(h)) else { continue };
        let (Some(pr), Some(pm)) = (hr.shape_phase(h), hm.shape_phase(h)) else { continue };
        component(m, &run.id, h * bin, cr, cm, scale, pr, pm);
    }
}

/// One component (a harmonic, a product) re `scale`: the magnitude in
/// dB re itself with both held at the floor, the phase where the
/// reference stands above the phase floor.
#[allow(clippy::too_many_arguments)]
fn component(
    m: &mut Measurement,
    id: &str,
    bin: u32,
    cr: Complex<f64>,
    cm: Complex<f64>,
    scale: f64,
    phase_r: f64,
    phase_m: f64,
) {
    m.points += 1;
    let (lr, lm) = (cr.norm() / scale, cm.norm() / scale);
    let e = (floored_db(lm) - floored_db(lr)).abs();
    m.first.offer(e, || {
        format!("{id} bin {bin}: {:.3} vs {:.3} dBc", db(lr), db(lm))
    });
    if lr >= PHASE_FLOOR && lm >= FLOOR {
        m.second.offer(degrees(phase_m - phase_r), || {
            format!(
                "{id} bin {bin} ({:.1} dBc): {:.3}° vs {:.3}°",
                db(lr),
                phase_r.to_degrees(),
                phase_m.to_degrees()
            )
        });
    }
}

/// A run's measured results, for callers that read one characteristic
/// directly (the docs site's summary, a test's own assertion).
pub fn run_of<'a>(table: &'a Table, run: &Run) -> Option<&'a Measured> {
    table.runs.get(&run.id)
}
