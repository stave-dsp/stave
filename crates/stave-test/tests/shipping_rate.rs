//! The shipping-rate report (DSP.md R12): every case of the
//! characterisation matrix at 44.1, 48 and 96 kHz in each way it could
//! ship — the plain trapezoidal rule at the host rate, prewarped, run 2×
//! or 4× oversampled, and the implicit midpoint rule (Germain 2017) —
//! against the same ngspice tables, with C2 (response), C5 (harmonics)
//! and C10 (aliasing) side by side, and the configuration each case
//! ships in (`circuits::SHIPPING`). The defaults' budgets are T0's and
//! T1's own (`circuits/budgets.rs`); this prints what they were chosen
//! from. Ignored by default (about a minute a configuration):
//!
//! ```text
//! cargo test --release -p stave-test --test shipping_rate -- --ignored --nocapture
//! ```
//!
//! **The implicit midpoint rule.** For a circuit whose nonlinear devices
//! are resistive and whose reactances are linear — every netlist the DK
//! and wave-digital methods take — the midpoint rule's update is the
//! trapezoidal rule's with the input averaged, `½(u[n] + u[n−1])`, and
//! its solved voltages are those at `t − T/2` (Germain 2017, §4.2: the
//! two rules are conjugate). The companions are the same `2C/T`
//! conductances; only the history differs (`2C/T·v` against
//! `2C/T·v + i`), and the update `x' = 2·(2C/T)·v − x` has the same form
//! with `v` read at the midpoint. So the states ring alike — the
//! difference Germain measures belongs to state-space forms whose
//! nonlinearity sits in `f(x)`, not to a resistive device solved at each
//! instant — and what remains is a two-tap average on the input (a zero
//! at Nyquist: −6 dB at 16 kHz at 48 kHz) and half a sample of delay.
//! It runs here as exactly that, on `Circuit<Dk>`, and its rows report
//! magnitudes only (the half sample is a phase ramp, not an error the
//! circuit makes).

#[allow(dead_code)]
mod circuits;

use circuits::{oversample, shipping, CIRCUITS};
use stave_test::characterise::model::circuit;
use stave_test::characterise::{deck, Characterisation, Characterise, Check, Plan, Rates, Shipping, Tier};

/// The ways `name` could ship: before (1×, the plain trapezoidal rule)
/// and its default (`circuits::SHIPPING`); with `STAVE_SHIPPING_ALL=1`
/// also a 16 kHz prewarp, 2×, 4× and 8×, and the implicit midpoint rule
/// (`None`).
fn configurations(name: &str) -> Vec<Option<Shipping>> {
    if let Some(list) = circuits::configs_from_env() {
        return list.into_iter().map(Some).collect();
    }
    let mut v = vec![Some(Shipping::PLAIN)];
    if std::env::var_os("STAVE_SHIPPING_ALL").is_some() {
        v.extend([
            Some(Shipping::PLAIN.prewarped(16e3)),
            Some(Shipping::oversampled(2)),
            Some(Shipping::oversampled(4)),
            Some(Shipping::oversampled(8)),
            None,
        ]);
    }
    let shipped = Some(shipping(name));
    if !v.contains(&shipped) {
        v.push(shipped);
    }
    v
}

/// The worst shipping-rate error of `check` at the host rate `host`,
/// its second error, and where the first was.
fn at(c: &Characterisation, check: Check, host: f64, m: u32) -> (f64, f64, String) {
    let rate = host * f64::from(m);
    c.rows
        .iter()
        .find(|r| r.check == check && r.class == Rates::Shipping && r.rate == rate)
        .map_or((f64::NAN, f64::NAN, String::new()), |r| {
            (r.measurement.first.error, r.measurement.second.error, r.measurement.first.at.clone())
        })
}

/// `Circuit<Dk>` by the implicit midpoint rule: the trapezoidal circuit
/// on the averaged input, read at the midpoint.
fn midpoint(name: &str) -> Characterise {
    let plan = Plan::parse(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{name}.plan")),
        )
        .unwrap(),
    )
    .unwrap();
    let input = plan.input.clone();
    let deck = deck(name);
    Characterise::ngspice(name).stage(move |rate| {
        let mut c = circuit(&deck, &input, rate);
        let (mut first, mut last) = (true, 0.0);
        move |x: f64| {
            if std::mem::take(&mut first) {
                last = x;
                return c.output();
            }
            let y = c.tick_f64(0.5 * (x + last));
            last = x;
            y
        }
    })
}

#[test]
#[ignore = "R12 report: run on demand"]
fn shipping_rate_report() {
    let only = std::env::var("STAVE_SHIPPING_ONLY").ok();
    let hosts = [44_100.0, 48_000.0, 96_000.0];
    println!(
        "| circuit | shipping | rate | C2 response dB / ° | C5 harmonics dB | C6 THD+N dB | C10 aliasing dBc | worst harmonic |\n|---|---|---|---|---|---|---|---|"
    );
    let cases: Vec<_> = CIRCUITS
        .iter()
        .filter(|c| only.as_deref().is_none_or(|o| o.split(',').any(|n| n == c.name)))
        .flat_map(|c| configurations(c.name).into_iter().map(move |s| (c, s)))
        .collect();
    let rows = circuits::par_map(&cases, |&(c, config)| {
        let shipped = Some(shipping(c.name));
        let label = config.map_or("1×, midpoint".to_string(), |s| s.to_string());
        let (characterise, m) = match config {
            Some(s) => (circuits::model(c.name, Tier::T1).shipped(s), s.oversample),
            None => (midpoint(c.name).tier(Tier::T1), 1),
        };
        let result = characterise.oversampled(oversample(c.name, Tier::T1)).run();
        let mark = if config.is_some() && config == shipped { " (default)" } else { "" };
        let mut out = String::new();
        for host in hosts {
            let (r, rd, _) = at(&result, Check::Response, host, m);
            let (h, _, worst) = at(&result, Check::Harmonics, host, m);
            let (t, _, _) = at(&result, Check::ThdN, host, m);
            let (a, _, _) = at(&result, Check::Aliasing, host, m);
            let degrees = if config.is_some() { format!("{rd:.3}") } else { "—".into() };
            out.push_str(&format!(
                "| {} | {label}{mark} | {:.1} kHz | {r:.3} / {degrees} | {h:.3} | {t:.3} | {a:.1} | {worst} |\n",
                c.name,
                host / 1e3
            ));
        }
        out
    });
    for r in rows {
        print!("{r}");
    }
}

/// C10 by audibility for the same choice: each circuit's worst ANMR over
/// the piano at +12 and +20 dB re its knee at 44.1 kHz (the criterion
/// the defaults are chosen by: −10 dB, inaudible), for each oversampling
/// with and without stateful ADAA.
#[test]
#[ignore = "R12 report: run on demand"]
fn anmr_report() {
    use circuits::piano_anmr;
    let only = std::env::var("STAVE_SHIPPING_ONLY").ok();
    let configs = circuits::configs_from_env().unwrap_or_else(|| vec![
        Shipping::PLAIN,
        Shipping::oversampled(2),
        Shipping::oversampled(4),
        Shipping::oversampled(8),
        Shipping::PLAIN.antialiased(),
        Shipping::oversampled(2).antialiased(),
        Shipping::oversampled(4).antialiased(),
        Shipping::oversampled(8).antialiased(),
        Shipping::oversampled(16),
    ]);
    let cases: Vec<_> = CIRCUITS
        .iter()
        .filter(|c| !c.supply)
        .filter(|c| only.as_deref().is_none_or(|o| o.split(',').any(|n| n == c.name)))
        .flat_map(|c| configs.iter().map(move |&s| (c, s)))
        .collect();
    let rows: Vec<String> = circuits::par_map(&cases, |&(c, s)| {
        let (a12, _) = piano_anmr(c.name, 44_100.0, s, 12.0);
        let (a20, hz) = piano_anmr(c.name, 44_100.0, s, 20.0);
        format!("| {} | {s} | {a12:.1} | {a20:.1} | {hz:.0} |", c.name)
    });
    println!("| circuit | shipping | ANMR +12 dB | ANMR +20 dB | worst +20 at Hz |\n|---|---|---|---|---|");
    for r in rows {
        println!("{r}");
    }
}
