//! Least-squares refits of device presets to the R23 datasheet tables
//! (DSP.md E11): the tool that produced every `*_PHILIPS`, `*_TUNGSOL`,
//! `*_GEC`, `*_ONSEMI`, `*_MULLARD` and `*_VISHAY` constant, and the
//! curve-fitted TL072 and JRC4558. Each fit is an ignored test; run one
//! (or all) and paste the printed constant:
//!
//! ```text
//! cargo test --release -p stave-test --test fit_datasheet -- --ignored --nocapture [name]
//! ```
//!
//! **Method.** Levenberg–Marquardt on weighted residuals, every
//! parameter held inside physically sensible bounds by a logistic map
//! (log-scaled for parameters spanning decades). A reading's residual is
//! `(model − reading)/u`, with `u` the table's own uncertainty plus the
//! model tolerance R13 states (5% of the table's peak on a linear axis,
//! 10% on a log axis) — the budget `matches_datasheet.rs` holds the
//! model to, so tables of different quality weigh as they deserve and a
//! residual within ±1 is a reading within budget. The model is read
//! where `matches_datasheet.rs` reads it: anywhere within the reading's
//! abscissa uncertainty, the value nearest the reading. Log axes are
//! compared as ln(value).
//!
//! The fit minimises Σ r² first (least squares), then Σ r^p for p = 4,
//! 8, 16, each stage from the last (an L_p continuation toward the
//! minimax fit, Lawson's way of reaching a Chebyshev fit through least
//! squares): the judge is the worst reading, and least squares alone can
//! leave one curve's end over budget to fit the bulk a little better.
//! Every stage is printed. The models are stave-dsp's own, measured as
//! `matches_datasheet.rs` measures them (`bench`), so a fit is a fit of
//! the shipped code. Deterministic: the same tables give the same
//! constants.
//!
//! Each fit prints its parameters, the mean squared residual and the
//! worst residual (in budgets); `matches_datasheet.rs` is the judge.

mod bench;

use bench::{bisect, bjt, diode_volts, nearest, opamp};
use stave_dsp::circuit::devices::bjt::GummelPoon;
use stave_dsp::circuit::devices::diode::Shockley;
use stave_dsp::circuit::devices::opamp::Macromodel;
use stave_dsp::circuit::devices::pentode::koren::Koren as KorenPentode;
use stave_dsp::circuit::devices::pentode::reefman::{Knee, Reefman};
use stave_dsp::circuit::devices::pentode::Pentode;
use stave_dsp::circuit::devices::triode::koren::Koren;
use stave_dsp::units::Hz;
use stave_test::reference::{Curve, Datasheet, Uncertainty};

// ---------------------------------------------------------------- solver

/// A fitted parameter: its starting value and its bounds.
#[derive(Copy, Clone, Debug)]
struct Param {
    name: &'static str,
    init: f64,
    lo: f64,
    hi: f64,
}

const fn param(name: &'static str, init: f64, lo: f64, hi: f64) -> Param {
    Param { name, init, lo, hi }
}

impl Param {
    /// Log-scaled when positive and spanning more than a decade.
    fn log(&self) -> bool {
        self.lo > 0.0 && self.hi / self.lo > 10.0
    }

    fn span(&self) -> (f64, f64) {
        if self.log() {
            (self.lo.ln(), self.hi.ln())
        } else {
            (self.lo, self.hi)
        }
    }

    /// The bounded value of the free coordinate `z`.
    fn value(&self, z: f64) -> f64 {
        let (a, b) = self.span();
        let u = a + (b - a) / (1.0 + (-z).exp());
        if self.log() {
            u.exp()
        } else {
            u
        }
    }

    /// The free coordinate of `v` (clamped just inside the bounds).
    fn free(&self, v: f64) -> f64 {
        let (a, b) = self.span();
        let u = if self.log() { v.ln() } else { v };
        let s = ((u - a) / (b - a)).clamp(1e-6, 1.0 - 1e-6);
        (s / (1.0 - s)).ln()
    }
}

/// A finished fit.
struct Fit {
    values: Vec<f64>,
    residuals: Vec<f64>,
}

impl Fit {
    fn chi2(&self) -> f64 {
        self.residuals.iter().map(|r| r * r).sum::<f64>() / self.residuals.len() as f64
    }

    fn worst(&self) -> f64 {
        self.residuals.iter().fold(0.0, |m: f64, r| m.max(r.abs()))
    }
}

/// `a·x = b` by Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[p][k].abs() < 1e-300 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        for i in k + 1..n {
            let m = a[i][k] / a[k][k];
            for j in k..n {
                a[i][j] -= m * a[k][j];
            }
            b[i] -= m * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k][j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k][k];
    }
    Some(x)
}

/// The threads a fit may use: `RAYON_NUM_THREADS`, else
/// `STAVE_OFFLOAD_JOBS` (a build host's per-job cap), else the machine's
/// parallelism.
fn threads() -> usize {
    ["RAYON_NUM_THREADS", "STAVE_OFFLOAD_JOBS"]
        .iter()
        .find_map(|k| std::env::var(k).ok()?.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// Levenberg–Marquardt (Marquardt's diagonal scaling) over the bounded
/// parameters from their starting values, minimising Σ|r|^p (each
/// residual mapped to sign(r)·|r|^(p/2), so p = 2 is least squares).
fn levenberg_marquardt(
    params: &[Param],
    p: f64,
    raw: &(dyn Fn(&[f64]) -> Vec<f64> + Sync),
) -> Fit {
    let residuals = |v: &[f64]| -> Vec<f64> {
        raw(v)
            .into_iter()
            .map(|r| r.signum() * r.abs().powf(0.5 * p))
            .collect()
    };
    let values = |z: &[f64]| -> Vec<f64> {
        params.iter().zip(z).map(|(p, &z)| p.value(z)).collect()
    };
    let cost = |r: &[f64]| r.iter().map(|r| r * r).sum::<f64>();
    let mut z: Vec<f64> = params.iter().map(|p| p.free(p.init)).collect();
    let mut r = residuals(&values(&z));
    let mut c = cost(&r);
    let mut lambda = 1e-3;
    let n = z.len();
    let mut stalls = 0;
    for _ in 0..150 {
        // Central-difference Jacobian in the free coordinates, columns
        // shared among at most `threads()` threads (the transistor fits'
        // curve tracer is slow).
        let h = 1e-5;
        let column = |k: usize| -> Vec<f64> {
            let mut up = z.clone();
            let mut down = z.clone();
            up[k] += h;
            down[k] -= h;
            let (ru, rd) = (residuals(&values(&up)), residuals(&values(&down)));
            ru.iter().zip(&rd).map(|(a, b)| (a - b) / (2.0 * h)).collect()
        };
        let workers = threads().min(n).max(1);
        let jac: Vec<Vec<f64>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|w| {
                    let column = &column;
                    scope.spawn(move || {
                        (w..n)
                            .step_by(workers)
                            .map(|k| (k, column(k)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut columns: Vec<(usize, Vec<f64>)> = handles
                .into_iter()
                .flat_map(|h| h.join().expect("a Jacobian column"))
                .collect();
            columns.sort_by_key(|(k, _)| *k);
            columns.into_iter().map(|(_, c)| c).collect()
        });
        let jtj: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| jac[i].iter().zip(&jac[j]).map(|(a, b)| a * b).sum()).collect())
            .collect();
        let jtr: Vec<f64> = (0..n).map(|i| jac[i].iter().zip(&r).map(|(a, b)| a * b).sum()).collect();
        let mut improved = false;
        while lambda < 1e12 {
            let mut a = jtj.clone();
            for i in 0..n {
                a[i][i] += lambda * jtj[i][i].max(1e-12);
            }
            let Some(step) = solve(a, jtr.iter().map(|g| -g).collect()) else {
                lambda *= 4.0;
                continue;
            };
            let trial: Vec<f64> = z.iter().zip(&step).map(|(z, s)| z + s).collect();
            let rt = residuals(&values(&trial));
            let ct = cost(&rt);
            if ct.is_finite() && ct < c {
                stalls = if (c - ct) < 1e-10 * c { stalls + 1 } else { 0 };
                z = trial;
                r = rt;
                c = ct;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
        }
        if !improved || stalls >= 3 {
            break;
        }
    }
    let values = values(&z);
    Fit {
        residuals: raw(&values),
        values,
    }
}

/// Least squares, then the L_p continuation toward minimax (module
/// docs), each stage printed.
fn least_squares(params: &[Param], residuals: &(dyn Fn(&[f64]) -> Vec<f64> + Sync)) -> Fit {
    stages(params, &[2.0, 4.0, 8.0, 16.0], residuals)
}

/// [`levenberg_marquardt`] at each exponent of `ps` in turn, each from
/// the last; every stage's parameters are printed as it ends (a slow
/// fit's progress survives an interruption).
fn stages(params: &[Param], ps: &[f64], residuals: &(dyn Fn(&[f64]) -> Vec<f64> + Sync)) -> Fit {
    let mut params = params.to_vec();
    let mut fit = None;
    for &p in ps {
        let f = levenberg_marquardt(&params, p, residuals);
        println!(
            "   p = {p:>2}: mean r² {:.3}, worst |r| {:.3}; {:?}",
            f.chi2(),
            f.worst(),
            f.values
        );
        for (q, &v) in params.iter_mut().zip(&f.values) {
            q.init = v;
        }
        fit = Some(f);
    }
    fit.expect("a stage")
}

/// The best of several starts (each a full set of starting values).
fn best_of(params: &[Param], starts: &[Vec<f64>], residuals: &(dyn Fn(&[f64]) -> Vec<f64> + Sync)) -> Fit {
    starts
        .iter()
        .map(|s| {
            let ps: Vec<Param> = params
                .iter()
                .zip(s)
                .map(|(p, &init)| Param { init, ..*p })
                .collect();
            least_squares(&ps, residuals)
        })
        .min_by(|a, b| a.worst().total_cmp(&b.worst()))
        .expect("a start")
}

fn report(title: &str, params: &[Param], fit: &Fit) {
    println!("== {title}");
    for (p, v) in params.iter().zip(&fit.values) {
        let edge = if (v - p.lo).abs() < 1e-3 * (p.hi - p.lo) || (p.hi - v).abs() < 1e-3 * (p.hi - p.lo) {
            "  (at a bound)"
        } else {
            ""
        };
        println!("   {:<8} = {v:.6e}   [{:e}, {:e}]{edge}", p.name, p.lo, p.hi);
    }
    println!(
        "   mean r² = {:.3}, worst residual {:.3} budgets, N = {}",
        fit.chi2(),
        fit.worst(),
        fit.residuals.len()
    );
}

// ------------------------------------------------------------- residuals

/// Model tolerance on a linear axis, as a fraction of the table's peak
/// reading (`matches_datasheet.rs`'s `TOLERANCE`).
const TOLERANCE: f64 = 0.05;
/// Model tolerance on a log axis or a printed figure, as a fraction of
/// the value (`matches_datasheet.rs`'s `LOG_TOLERANCE`).
const LOG_TOLERANCE: f64 = 0.10;

/// The residuals of `model(curve, x)` against every point of `sheet`'s
/// curves that `keep` keeps, in budgets (see the module docs): the model
/// read nearest the reading within its abscissa uncertainty; ln(value)
/// on a log axis.
fn residuals(
    sheet: &Datasheet,
    log: bool,
    keep: impl Fn(&Curve) -> bool,
    model: impl Fn(&Curve, f64) -> f64,
    out: &mut Vec<f64>,
) {
    let budget = if log {
        let Uncertainty::Relative(u) = sheet.uncertainty_y() else {
            panic!("{}: a log axis needs a relative uncertainty", sheet.name());
        };
        u.ln_1p() + LOG_TOLERANCE.ln_1p()
    } else {
        let peak = sheet
            .curves()
            .iter()
            .flat_map(|c| c.reference.values())
            .fold(0.0, |m: f64, v| m.max(v.abs()));
        sheet.uncertainty_y().at(peak) + TOLERANCE * peak
    };
    for curve in sheet.curves().iter().filter(|c| keep(c)) {
        let r = &curve.reference;
        for (&x, &y) in r.abscissa().iter().zip(r.values()) {
            let ux = sheet.uncertainty_x().at(x);
            let (y, read) = if log {
                (
                    y.ln(),
                    nearest(x, y.ln(), ux, &|x| model(curve, x).max(1e-300).ln()),
                )
            } else {
                (y, nearest(x, y, ux, &|x| model(curve, x)))
            };
            out.push((read - y) / budget);
        }
    }
}

fn all(_: &Curve) -> bool {
    true
}

/// How hard a table limit holds in a fit.
#[derive(Copy, Clone)]
enum Hold {
    /// A residual in units of 1% of the limit, zero inside it: the
    /// curves can buy a little past it.
    Soft,
    /// In units of 0.1%, zero only 1% inside it: no curve's residual buys
    /// a step past a limit `matches_datasheet.rs` holds as a pass.
    Hard,
}

/// A penalty residual holding `value` inside `[min, max]`.
fn limit(value: f64, min: f64, max: f64, hold: Hold) -> f64 {
    let (min, max, unit) = match hold {
        Hold::Soft => (min, max, 0.01),
        Hold::Hard => (1.01 * min, 0.99 * max, 0.001),
    };
    if value < min {
        (min - value) / (unit * min)
    } else if value > max {
        (value - max) / (unit * max)
    } else {
        0.0
    }
}

// ----------------------------------------------------------------- tubes

/// Koren's triode law on Philips' ECC83 plate curves.
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn ecc83() {
    let sheet = Datasheet::load("ecc83_plate");
    let t = Koren::T12AX7;
    let params = [
        param("mu", t.mu, 70.0, 130.0),
        param("ex", t.ex, 1.0, 1.6),
        param("kg1", t.kg1, 100.0, 1e4),
        param("kp", t.kp, 50.0, 5000.0),
        param("kvb", t.kvb, 1.0, 1e5),
    ];
    let tube = |v: &[f64]| Koren {
        name: "ECC83 (Philips)",
        mu: v[0],
        ex: v[1],
        kg1: v[2],
        kp: v[3],
        kvb: v[4],
        typical_grid_bias: t.typical_grid_bias,
    };
    let fit = least_squares(&params, &|v| {
        let tube = tube(v);
        let mut r = Vec::new();
        residuals(&sheet, false, all, |c, va| tube.plate(c.parameter("vg"), va).amps, &mut r);
        r
    });
    report("ECC83: Koren triode on Philips sheet B", &params, &fit);
}

fn screen_volts(sheet: &Datasheet, c: &Curve) -> f64 {
    c.get("vg2")
        .or(sheet.condition("vg2"))
        .expect("a screen voltage")
}

/// Koren's pentode law refitted to one sheet's plate curves (μ, Ex, Kg1,
/// Kp, Kvb), then `Kg2` alone to its screen curves when it has them.
fn koren_pentode(name: &'static str, plate: &str, screen: Option<&str>, start: KorenPentode) {
    let sheet = Datasheet::load(plate);
    let params = [
        param("mu", start.mu, 3.0, 30.0),
        param("ex", start.ex, 1.0, 1.6),
        param("kg1", start.kg1, 50.0, 1e4),
        param("kp", start.kp, 5.0, 1000.0),
        param("kvb", start.kvb, 1.0, 1000.0),
    ];
    let tube = |v: &[f64]| KorenPentode {
        name,
        mu: v[0],
        ex: v[1],
        kg1: v[2],
        kp: v[3],
        kvb: v[4],
        ..start
    };
    let fit = least_squares(&params, &|v| {
        let tube = tube(v);
        let mut r = Vec::new();
        residuals(
            &sheet,
            false,
            all,
            |c, va| tube.currents(c.parameter("vg1"), screen_volts(&sheet, c), va).plate,
            &mut r,
        );
        r
    });
    report(&format!("{name}: Koren pentode on {plate}"), &params, &fit);
    let Some(screen) = screen else { return };
    let tube = tube(&fit.values);
    let sheet = Datasheet::load(screen);
    let params = [param("kg2", start.kg2, 100.0, 1e5)];
    let fit = least_squares(&params, &|v| {
        let tube = KorenPentode { kg2: v[0], ..tube };
        let mut r = Vec::new();
        screen_residuals(&sheet, &tube, &mut r);
        r
    });
    report(&format!("{name}: Koren Kg2 on {screen}"), &params, &fit);
}

/// A screen table's residuals: Ig2 against Va (the curve's `vg1`), or
/// against Vg1 when the curve fixes `va` (the EL34's transfer chart).
fn screen_residuals(sheet: &Datasheet, tube: &impl Pentode, out: &mut Vec<f64>) {
    residuals(
        sheet,
        false,
        all,
        |c, x| {
            let vg2 = screen_volts(sheet, c);
            match c.get("va") {
                Some(va) => tube.currents(x, vg2, va).screen,
                None => tube.currents(c.parameter("vg1"), vg2, x).screen,
            }
        },
        out,
    );
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn el34_koren() {
    koren_pentode("EL34 (Philips)", "el34_plate", Some("el34_screen"), KorenPentode::EL34);
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn t6l6gc_koren() {
    koren_pentode("6L6GC (Tung-Sol)", "6l6gc_plate", Some("6l6gc_screen"), KorenPentode::T6L6GC);
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn kt88_koren() {
    koren_pentode("KT88 (GEC)", "kt88_plate", None, KorenPentode::KT88);
}

/// Reefman's `αs` from the fitted `Kg2/Kg1` and his `α`, the cathode
/// current's fractional loss at zero plate volts (eq. 27: `α = 1 −
/// (Kg1/Kg2)(1 + αs)`). Fitting `α` in [0, 0.5] keeps the space charge
/// roughly constant down to the knee, as Reefman's §4.3 has it (`αs`
/// alone, unbounded by it, runs off into a screen that draws several
/// times the cathode's current at low plate volts, where no table has
/// readings to stop it).
fn screen_excess(ratio: f64, alpha: f64) -> f64 {
    ((1.0 - alpha) * ratio - 1.0).max(0.0)
}

/// Reefman's law fitted to one sheet's plate and screen curves together,
/// each knee tried; the one with the smaller worst residual is printed
/// last.
fn reefman(name: &'static str, plate: &str, screen: &str, start: KorenPentode) {
    let plates = Datasheet::load(plate);
    let screens = Datasheet::load(screen);
    let params = [
        param("mu", start.mu, 3.0, 30.0),
        param("ex", start.ex, 1.0, 1.6),
        param("kg1", 0.5 * start.kg1, 20.0, 1e4),
        param("kg2/kg1", 8.0, 1.2, 100.0),
        param("kp", start.kp, 5.0, 1000.0),
        param("kvb", 100.0, 1.0, 1e6),
        param("a", 1e-4, 0.0, 3e-3),
        param("alpha", 0.1, 0.0, 0.5),
        param("beta", 0.05, 1e-3, 1.0),
    ];
    let mut best: Option<(f64, Knee, Vec<f64>)> = None;
    for knee in [Knee::Hyperbolic, Knee::Exponential] {
        let tube = |v: &[f64]| Reefman {
            name,
            mu: v[0],
            ex: v[1],
            kg1: v[2],
            kg2: v[2] * v[3],
            kp: v[4],
            kvb: v[5],
            a: v[6],
            alpha_s: screen_excess(v[3], v[7]),
            beta: v[8],
            knee,
        };
        let starts: Vec<Vec<f64>> = [0.01, 0.05, 0.2]
            .iter()
            .map(|&beta| {
                params
                    .iter()
                    .map(|p| if p.name == "beta" { beta } else { p.init })
                    .collect()
            })
            .collect();
        let fit = best_of(&params, &starts, &|v| {
            let tube = tube(v);
            let mut r = Vec::new();
            residuals(
                &plates,
                false,
                all,
                |c, va| tube.currents(c.parameter("vg1"), screen_volts(&plates, c), va).plate,
                &mut r,
            );
            screen_residuals(&screens, &tube, &mut r);
            r
        });
        report(&format!("{name}: Reefman {knee:?} on {plate} + {screen}"), &params, &fit);
        if best.as_ref().is_none_or(|b| fit.worst() < b.0) {
            best = Some((fit.worst(), knee, fit.values.clone()));
        }
    }
    let (worst, knee, v) = best.expect("two fits");
    println!(
        "   best: {knee:?} (worst {worst:.3} budgets); kg2 = {:.6e}, alpha_s = {:.6e}",
        v[2] * v[3],
        screen_excess(v[3], v[7])
    );
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn el34_reefman() {
    reefman("EL34 (Philips)", "el34_plate", "el34_screen", KorenPentode::EL34);
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn t6l6gc_reefman() {
    reefman("6L6GC (Tung-Sol)", "6l6gc_plate", "6l6gc_screen", KorenPentode::T6L6GC);
}

#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn el84_reefman() {
    reefman("EL84 (Philips/Mullard)", "el84_plate", "el84_screen", KorenPentode::EL84);
}

// ----------------------------------------------------------- transistors

/// The 2N3904's Gummel–Poon card on onsemi's Figures 15–17 and the
/// table's hFE limits. Figure 16's IC = 1 mA curve (β ≈ 52) contradicts
/// the table's hFE ≥ 70 at 1 mA and is left out; the test still holds the
/// model to it, as a known gap. The curves pull hFE at 50 mA a little
/// under the table's 60, so the limits are held softly through the L_p
/// stages, then hard for a last p = 8 stage. Slow (the curve tracer's
/// nested solves): two to three hours on a busy eight-core host.
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn n2n3904() {
    let hfe = Datasheet::load("2n3904_hfe");
    let on = Datasheet::load("2n3904_on_voltages");
    let sat = Datasheet::load("2n3904_saturation");
    let q0 = GummelPoon::N2N3904;
    let params = [
        param("IS", q0.saturation_amps, 1e-16, 1e-12),
        param("BF", q0.forward_beta, 80.0, 2000.0),
        param("BR", q0.reverse_beta, 0.1, 20.0),
        param("VAF", q0.forward_early_volts, 20.0, 300.0),
        param("IKF", q0.forward_knee_amps, 2e-3, 1.0),
        param("ISE", q0.emitter_leakage_amps, 1e-17, 1e-10),
        param("NE", q0.emitter_leakage_emission, 1.05, 3.0),
        param("RB", q0.base_ohms, 0.1, 100.0),
        param("RE", 0.1, 1e-3, 5.0),
        param("RC", q0.collector_ohms, 1e-2, 10.0),
    ];
    let card = |v: &[f64]| GummelPoon {
        saturation_amps: v[0],
        forward_beta: v[1],
        reverse_beta: v[2],
        forward_early_volts: v[3],
        forward_knee_amps: v[4],
        emitter_leakage_amps: v[5],
        emitter_leakage_emission: v[6],
        base_ohms: v[7],
        emitter_ohms: v[8],
        collector_ohms: v[9],
        ..q0
    };
    let residuals = |v: &[f64], hold: Hold| {
        let q = card(v);
        let mut r = Vec::new();
        let vce = hfe.condition("vce").expect("the chart's VCE");
        let xs = hfe.curves()[0].reference.abscissa();
        let (lo, hi) = (xs[0].ln(), xs[xs.len() - 1].ln());
        let peak = (0..=48)
            .map(|k| bjt::hfe_at(&q, (lo + (hi - lo) * f64::from(k) / 48.0).exp(), vce))
            .fold(0.0, f64::max);
        residuals(&hfe, true, all, |_, ic| bjt::hfe_at(&q, ic, vce) / peak, &mut r);
        residuals(
            &on,
            false,
            all,
            |c, ic| {
                if let Some(vce) = c.get("vce") {
                    let ib = ic / bjt::hfe_at(&q, ic, vce);
                    return bjt::terminal_vbe(&q, ib, vce);
                }
                let ib = ic / c.parameter("ic_over_ib");
                let vce = bjt::vce_for(&q, ib, ic);
                match c.parameter("kind") as u8 {
                    1 => bjt::terminal_vbe(&q, ib, vce),
                    _ => vce,
                }
            },
            &mut r,
        );
        residuals(
            &sat,
            false,
            |c| c.parameter("ic") > 1.5e-3,
            |c, ib| bjt::vce_for(&q, ib, c.parameter("ic")),
            &mut r,
        );
        for (ic, min, max) in [
            (0.1e-3, 40.0, f64::INFINITY),
            (1e-3, 70.0, f64::INFINITY),
            (10e-3, 100.0, 300.0),
            (50e-3, 60.0, f64::INFINITY),
            (100e-3, 30.0, f64::INFINITY),
        ] {
            r.push(limit(bjt::hfe_at(&q, ic, 1.0), min, max, hold));
        }
        r
    };
    let soft = least_squares(&params, &|v| residuals(v, Hold::Soft));
    report("2N3904: Gummel–Poon on onsemi Figs. 15–17, limits soft", &params, &soft);
    let from: Vec<Param> = params
        .iter()
        .zip(&soft.values)
        .map(|(p, &init)| Param { init, ..*p })
        .collect();
    let fit = stages(&from, &[8.0], &|v| residuals(v, Hold::Hard));
    report("2N3904: Gummel–Poon on onsemi Figs. 15–17, limits hard", &params, &fit);
}

/// kT/q at 25 °C, the AC128 sheet's Tamb.
const THERMAL_VOLTS_25C: f64 = 1.380_649e-23 * 298.15 / 1.602_176_634e-19;

/// A typical AC128 from Mullard's curves: the DAFx-17 vintage card's
/// reverse-region and leakage-collector terms kept, the forward terms
/// and ohmic resistances fitted at 25 °C, inside the table's static hFE
/// limits.
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn ac128() {
    let output = Datasheet::load("ac128_output");
    let hfe = Datasheet::load("ac128_hfe");
    let transfer = Datasheet::load("ac128_transfer");
    let input = Datasheet::load("ac128_input");
    let q0 = GummelPoon::AC128;
    let params = [
        param("IS", q0.saturation_amps, 1e-8, 1e-3),
        param("BF", 100.0, 40.0, 1000.0),
        param("NF", q0.forward_emission, 0.95, 1.6),
        param("VAF", 100.0, 5.0, 1000.0),
        param("IKF", 2.0, 0.05, 20.0),
        param("ISE", q0.emitter_leakage_amps, 1e-10, 1e-3),
        param("NE", q0.emitter_leakage_emission, 1.05, 3.0),
        param("RB", q0.base_ohms, 0.01, 100.0),
        param("RE", q0.emitter_ohms, 1e-3, 3.0),
    ];
    let card = |v: &[f64]| GummelPoon {
        saturation_amps: v[0],
        forward_beta: v[1],
        forward_emission: v[2],
        forward_early_volts: v[3],
        forward_knee_amps: v[4],
        emitter_leakage_amps: v[5],
        emitter_leakage_emission: v[6],
        base_ohms: v[7],
        emitter_ohms: v[8],
        thermal_volts: THERMAL_VOLTS_25C,
        ..q0
    };
    let fit = least_squares(&params, &|v| {
        let q = card(v);
        let mut r = Vec::new();
        residuals(&output, false, all, |c, vce| bjt::collector_at(&q, c.parameter("ib"), vce), &mut r);
        residuals(&hfe, false, all, |c, ic| bjt::hfe_at(&q, ic, c.parameter("vce")), &mut r);
        residuals(&transfer, false, all, |c, ib| bjt::collector_at(&q, ib, c.parameter("vce")), &mut r);
        residuals(
            &input,
            false,
            all,
            |c, vbe| {
                let vce = c.parameter("vce");
                bisect(0.0, 0.1, |ib| bjt::terminal_vbe(&q, ib, vce) - vbe)
            },
            &mut r,
        );
        for (ic, min, max) in [(50e-3, 55.0, 175.0), (300e-3, 55.0, 175.0), (1.0, 45.0, 165.0)] {
            let ib = bisect(0.0, ic, |ib| {
                let vce = bisect(0.0, 2.0, |v| v - bjt::terminal_vbe(&q, ib, v));
                bjt::collector_at(&q, ib, vce) - ic
            });
            r.push(limit(ic / ib, min, max, Hold::Soft));
        }
        r
    });
    report("AC128: Gummel–Poon on Mullard's typical curves", &params, &fit);
}

/// Model tolerance on a diode's forward voltage, volts
/// (`matches_datasheet.rs`'s `DIODE_VOLTS`).
const DIODE_VOLTS: f64 = 0.020;

/// The 1N4148 on Vishay's forward curve, compared in volts at each read
/// current, in the test's budget: the voltage reading's uncertainty,
/// the current's carried through the model's slope, and
/// [`DIODE_VOLTS`].
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn n4148() {
    let sheet = Datasheet::load("1n4148_forward");
    let curve = &sheet.curves()[0];
    let Uncertainty::Relative(ui) = sheet.uncertainty_y() else {
        panic!("a log current axis");
    };
    let uv = sheet.uncertainty_x().at(1.0);
    let d0 = Shockley::N4148;
    let params = [
        param("IS", d0.saturation_amps, 1e-12, 1e-6),
        param("N", d0.emission, 1.0, 2.5),
        param("RS", d0.series_ohms, 0.01, 10.0),
    ];
    let fit = least_squares(&params, &|v| {
        let d = Shockley {
            saturation_amps: v[0],
            emission: v[1],
            series_ohms: v[2],
            ..d0
        };
        let r = curve.transposed();
        let slope = r
            .abscissa()
            .iter()
            .map(|&i| (diode_volts(&d, i * (1.0 + ui)) - diode_volts(&d, i)).abs())
            .fold(0.0, f64::max);
        let budget = uv + slope + DIODE_VOLTS;
        r.abscissa()
            .iter()
            .zip(r.values())
            .map(|(&i, &vf)| (diode_volts(&d, i) - vf) / budget)
            .collect()
    });
    report("1N4148: Shockley on Vishay Fig. 2", &params, &fit);
}

// --------------------------------------------------------------- op-amps

/// The macromodel's small-signal open loop, gm/(G + sC).
fn open_loop(part: Macromodel, vcc: f64) -> impl Fn(f64) -> f64 {
    let p = part.prepare(-vcc, vcc);
    let c = Macromodel::COMPENSATION_FARADS;
    move |f| p.gm / p.leak.hypot(std::f64::consts::TAU * f * c)
}

/// DC gain and gain-bandwidth on an open-loop plot, the gain-bandwidth
/// bounded to within 9% of the figure the sheet's table prints
/// (`figures`, curve `gbw=1`): `matches_datasheet.rs` holds the model to
/// that figure within [`LOG_TOLERANCE`] (a follower's −3 dB point, which
/// for one pole is the GBW to 1/A0), so the plot cannot pull it out.
fn open_loop_fit(title: &str, table: &str, figures: &str, start: Macromodel) -> Vec<f64> {
    let sheet = Datasheet::load(table);
    let printed = Datasheet::load(figures).curve(&[("gbw", 1.0)]).reference.values()[0];
    let vcc = sheet.condition("vcc").expect("the chart's supply");
    let params = [
        param("A0", start.gain, 1e4, 1e7),
        param("GBW", printed, 0.91 * printed, 1.09 * printed),
    ];
    let fit = least_squares(&params, &|v| {
        let part = Macromodel {
            gain: v[0],
            gain_bandwidth: Hz::new(v[1] as f32),
            ..start
        };
        let model = open_loop(part, vcc);
        let mut r = Vec::new();
        residuals(&sheet, true, all, |_, f| model(f), &mut r);
        r
    });
    report(title, &params, &fit);
    fit.values
}

/// The TL072 on SLOS080W's curves: the open loop (Fig. 5-46), then each
/// edge's slew rate on the drawn follower pulse (Fig. 5-55), the slopes
/// read as `matches_datasheet.rs` reads them, in its budget.
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn tl072() {
    let start = Macromodel::TL072_SLOS080J;
    let v = open_loop_fit("TL072: open loop, Fig. 5-46", "tl072_open_loop", "tl072_figures", start);
    let part = Macromodel {
        gain: v[0],
        gain_bandwidth: Hz::new(v[1] as f32),
        ..start
    };
    let params = [
        param("SR rise", 10e6, 1e6, 50e6),
        param("SR fall", 12e6, 1e6, 50e6),
    ];
    let fit = least_squares(&params, &|v| {
        let part = Macromodel {
            slew_rate: v[0],
            slew_rate_falling: v[1],
            ..part
        };
        opamp::edges("tl072_pulse", part)
            .iter()
            .map(|e| (e.model / e.sheet - 1.0) / (e.reading + LOG_TOLERANCE))
            .collect()
    });
    report("TL072: slew, Fig. 5-55 follower pulse", &params, &fit);
}

/// The JRC4558 on NJM4558_E's curves: the open-loop plot, then the slew
/// rate on the maximum-swing plot. The headroom stays the table's 1 V
/// (±14 V into 10 kΩ): the plot's plateau is into 2 kΩ, a load the
/// macromodel (no output resistance) cannot see, and pedal circuits load
/// the part far more lightly.
#[test]
#[ignore = "a fit, run by hand (see the module docs)"]
fn jrc4558() {
    let start = Macromodel::JRC4558_TABLE;
    let v = open_loop_fit("JRC4558: open loop", "jrc4558_open_loop", "jrc4558_figures", start);
    let part = Macromodel {
        gain: v[0],
        gain_bandwidth: Hz::new(v[1] as f32),
        ..start
    };
    let sheet = Datasheet::load("jrc4558_swing");
    let vcc = sheet.condition("vcc").expect("the chart's supply");
    let params = [param("SR", 1.8e6, 0.3e6, 10e6)];
    let fit = least_squares(&params, &|v| {
        let part = Macromodel {
            slew_rate: v[0],
            slew_rate_falling: v[0],
            ..part
        };
        let model = opamp::swing_at(part, vcc);
        let mut r = Vec::new();
        residuals(&sheet, false, all, |_, f| model(f), &mut r);
        r
    });
    report("JRC4558: slew, maximum swing against frequency", &params, &fit);
}
