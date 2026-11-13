//! T1 of the characterisation matrix (DSP.md R17): the whole grid —
//! the multisine at every offset from the knee (C3), tones at 82 Hz to
//! 8 kHz across the level grid (C4–C6, C10), SMPTE and CCIF pairs (C7)
//! and bursts (C8) — oversampled and at 44.1, 48 and 96 kHz (C11: the
//! same budgets at every host rate). Ignored by default: the references
//! job runs it (`cargo test -p stave-test --test characterise_t1 --
//! --ignored`), beside the ngspice regeneration it is held to.

#[allow(dead_code)]
mod circuits;

use circuits::{budgets, par_map, piano_anmr, shipping, CIRCUITS};
use stave_measure::perceptual::nmr::AUDIBLE_ANMR_DB;
use stave_test::characterise::model::knee;
use stave_test::characterise::{deck, Characterise, Plan, Tier};

fn circuit(name: &str) {
    budgets(name, Tier::T1, Characterise::ngspice(name).tier(Tier::T1).circuit())
        .run()
        .assert();
}

/// The knees the plans were built at are still the circuits' knees,
/// to 0.2 dB (the plan rounds them to 0.1 dB).
#[test]
#[ignore = "T1: the references job runs it"]
fn knees_are_current() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    for c in CIRCUITS.iter().filter(|c| !c.supply) {
        let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{}.plan", c.name))).unwrap())
            .unwrap();
        let committed = plan.knee.unwrap();
        let now = knee(&deck(c.name), &plan.input, plan.settle_seconds);
        let moved = 20.0 * (now.amplitude / committed.amplitude).log10();
        println!("{}: knee {:.4e} V ({:?}), moved {moved:+.2} dB", c.name, now.amplitude, now.criterion);
        assert!(moved.abs() <= 0.2, "{}: the knee moved {moved:+.2} dB: re-bless the plan", c.name);
    }
}

macro_rules! t1 {
    ($($name:ident),*) => {$(
        #[test]
        #[ignore = "T1: the references job runs it"]
        fn $name() {
            circuit(stringify!($name));
        }
    )*};
}

t1!(
    diode_clipper,
    opamp_clipper,
    tube_screamer,
    common_cathode,
    common_cathode_grid_current,
    triode_stage,
    push_pull,
    push_pull_pentode,
    power_supply,
    fuzz_face,
    rangemaster,
    ce_stage,
    jfet_booster,
    mosfet_booster,
    ota_vca,
    fuzz_face_static,
    rangemaster_static,
    ce_stage_static,
    jfet_booster_static,
    mosfet_booster_static,
    boss_ce2_filters
);

/// The hand topologies' whole matrices.
mod hand {
    use super::circuits::hand;
    use stave_test::characterise::Tier;

    macro_rules! t1 {
        ($($name:ident: $case:literal),*) => {$(
            #[test]
            #[ignore = "T1: the references job runs it"]
            fn $name() {
                hand($case, Tier::T1);
            }
        )*};
    }

    t1!(
        diode_clipper: "diode_clipper/hand",
        opamp_clipper: "opamp_clipper/hand",
        common_cathode: "common_cathode/hand",
        common_cathode_grid_current: "common_cathode_grid_current/hand",
        power_supply: "power_supply/hand"
    );
}

/// C10 by audibility (DSP.md, "Aliasing budget by audibility"): each
/// circuit's `Circuit<Dk>` at 44.1 kHz — the worst host rate — as it
/// ships (`circuits::SHIPPING`, DSP.md R12: run at its oversampling and
/// read through an ideal decimator), with a tone on every semitone of the
/// piano (27.5 Hz to 4186 Hz; the 8192-point window merges neighbouring
/// keys in the lowest octave) at +12 and +20 dB re its knee: the worst
/// step's ANMR (Kahles et al. 2019) held to −10 dB, inaudible. A circuit
/// above it at its shipping factor is a finding, held to its recorded
/// worst + 3 dB: see [`PIANO`].
#[test]
#[ignore = "T1: the references job runs it"]
fn anmr_over_the_piano() {
    let rate = 44_100.0;
    // A thread a circuit, at most the slot's share of the cores.
    let circuits: Vec<_> = CIRCUITS.iter().filter(|c| !c.supply).collect();
    let results: Vec<(&str, f64, f64, f64)> = par_map(&circuits, |c| {
        let config = shipping(c.name);
        let worst = |offset: f64| piano_anmr(c.name, rate, config, offset);
        let (a12, hz12) = worst(12.0);
        let (a20, hz20) = worst(20.0);
        let (w, hz) = if a12 > a20 { (a12, hz12) } else { (a20, hz20) };
        println!(
            "{} ({config}): worst ANMR {a12:.1} dB at +12, {a20:.1} dB at +20 (at {hz:.0} Hz)",
            c.name
        );
        (c.name, a12, a20, w)
    });
    let mut over = Vec::new();
    for (name, a12, a20, worst) in results {
        let budget = PIANO.iter().find(|p| p.0 == name).map_or(AUDIBLE_ANMR_DB, |p| p.1);
        if worst > budget {
            over.push(format!("{name}: {a12:.1} / {a20:.1} dB at +12 / +20, budget {budget:.1} dB"));
        }
    }
    assert!(over.is_empty(), "ANMR over budget:\n  {}", over.join("\n  "));
}

/// The circuits above −10 dB at 44.1 kHz as they ship (findings, DSP.md
/// R12/R19): `(circuit, budget, recorded worst)`, the budget 3 dB above
/// the recording. Their floor does not fall with oversampling (the CE
/// stage reads +10.0 dB at 8× and 16×, the JFET booster −6.6 at 8× and
/// 16×, the MOSFET booster +13.5), so it is not aliasing: what remains is
/// R19's sweep to explain. Every other circuit is held to −10 dB; at 1×
/// (DSP.md R12's "before") 15 of 20 read above it, the diode clipper at
/// +51.0 dB, the MOSFET booster at +55.7, the CE-2's filters at +47.3.
/// Measured 2026-11-13.
const PIANO: &[(&str, f64, f64)] = &[
    ("ce_stage", 13.0, 10.0), // -11.6 / 10.0 dB at +12 / +20, 8×
    ("jfet_booster", -3.2, -6.2), // -6.7 / -6.2 dB at +12 / +20, 8×
    ("mosfet_booster", 17.2, 14.2), // 13.0 / 14.2 dB at +12 / +20, 8×
    ("ce_stage_static", 13.0, 10.0), // -11.5 / 10.0 dB at +12 / +20, 8×
    ("jfet_booster_static", -3.2, -6.2), // -6.7 / -6.2 dB at +12 / +20, 8×
    ("mosfet_booster_static", 16.9, 13.9), // 13.0 / 13.9 dB at +12 / +20, 8×
];

/// R16 on program material (DSP.md C13, R22's corpus): each circuit's
/// `Circuit<Dk>` at the corpus's own rate as it ships (its `SHIPPING`
/// oversampling, read through an ideal decimator) nulled against the
/// same deck at 16× (or twice the shipped factor), the solver ngspice
/// holds, on 2 s of R22's `guitar_di`,
/// `vocal` and `drums_loop` with their peaks 6 dB over the knee: the
/// residual's NMR under the oversampled output, level and latency
/// aligned (`stave_measure::perceptual::null`). A verdict per circuit
/// and clip, printed: findings, not a gate (the T0 matrix holds the
/// budgets). Skipped, saying so, without the corpus
/// (`stave_test::corpus`); the audio is never committed.
#[test]
#[ignore = "T1: the references job runs it"]
fn null_on_program_material() {
    use stave_measure::perceptual::null::{self, INAUDIBLE_DB};
    use stave_measure::perceptual::Playback;
    use stave_test::corpus::Corpus;
    // R22's roles: a guitar DI, a voice and a drum loop, 2 s of each.
    const ROLES: [&str; 3] = ["guitar_di", "vocal", "drums_loop"];
    const SECONDS: f64 = 2.0;
    let corpus = match Corpus::open() {
        Ok(c) => c,
        Err(skip) => return println!("null_on_program_material: {skip}"),
    };
    let clips: Vec<(String, Vec<f64>, f64)> = ROLES
        .iter()
        .filter_map(|role| {
            let item = match corpus.role(role) {
                Ok(items) => items.into_iter().next()?,
                Err(skip) => {
                    println!("{role}: {skip}");
                    return None;
                }
            };
            let m = corpus.load(&item).unwrap_or_else(|e| panic!("{role}: {e}"));
            let rate = f64::from(m.rate);
            let mono = m.mono();
            let n = mono.len().min((SECONDS * rate) as usize);
            let mut x: Vec<f64> = mono[..n].iter().map(|&v| f64::from(v)).collect();
            // 5 ms fades: no edge for a fractional delay to ring on.
            let fade = (0.005 * rate) as usize;
            for i in 0..fade.min(n / 2) {
                let g = 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / fade as f64).cos();
                x[i] *= g;
                x[n - 1 - i] *= g;
            }
            let peak = x.iter().fold(0.0f64, |p, v| p.max(v.abs()));
            (peak > 0.0).then(|| (format!("{role} ({})", item.name), x.iter().map(|v| v / peak).collect(), rate))
        })
        .collect();
    if clips.is_empty() {
        return println!("null_on_program_material skipped: none of {ROLES:?} has material");
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    let jobs: Vec<&circuits::Circuit> = CIRCUITS.iter().filter(|c| !c.supply).collect();
    let lines: Vec<String> = par_map(&jobs, |c| {
        let plan = Plan::parse(&std::fs::read_to_string(dir.join(format!("{}.plan", c.name))).unwrap()).unwrap();
        let deck = deck(c.name);
        let s = shipping(c.name);
        let ship = s.oversample.max(1) as usize;
        // The reference: the solver well past the shipped factor.
        let fine = 16.max(2 * ship);
        let peak = plan.knee.unwrap().at(6.0);
        clips
            .iter()
            .map(|(name, clip, rate)| {
                let settle = (plan.settle_seconds * rate).ceil() as usize;
                let mut input = vec![0.0; settle];
                input.extend(clip.iter().map(|v| v * peak));
                let run = |mut circuit: stave_dsp::circuit::Circuit<stave_dsp::circuit::methods::dk::Dk>, m: usize| -> Vec<f64> {
                    let up = band_limited_up(&input, m);
                    band_limited_down(&up.iter().map(|&v| circuit.tick_f64(v)).collect::<Vec<_>>(), m)
                };
                let model = run(circuits::shipped_circuit(&deck, &plan.input, *rate, s), ship);
                let reference = run(stave_test::characterise::model::circuit(&deck, &plan.input, rate * fine as f64), fine);
                let (r, m) = (&reference[settle..], &model[settle..]);
                let mean = r.iter().sum::<f64>() / r.len() as f64;
                let r: Vec<f64> = r.iter().map(|v| v - mean).collect();
                let m: Vec<f64> = m.iter().map(|v| v - mean).collect();
                let full = r.iter().fold(0.0f64, |p, v| p.max(v.abs())).max(1e-30);
                let n = null::between(&r, &m, *rate, Playback::full_scale(full), 1e-3);
                format!(
                    "{}: {name} at {:.1} kHz as it ships ({s}) vs {fine}×: {}: null NMR {:.1} dB total, {:.1} dB worst band ({:.0} Hz); aligned {:+.3} dB, {:+.2} µs",
                    c.name,
                    rate / 1e3,
                    null::verdict(n.nmr.total_db, n.nmr.max_db),
                    n.nmr.total_db,
                    n.nmr.max_db,
                    n.nmr.max_hz,
                    n.alignment.gain_db(),
                    n.alignment.delay * 1e6
                )
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    for l in &lines {
        println!("{l}");
    }
    let audible = lines.iter().filter(|l| l.contains("AUDIBLE")).count();
    println!("null_on_program_material: {audible} of {} audible (budget: ≤ {INAUDIBLE_DB} dB total and worst band)", lines.len());
}

/// `x` at `factor` times its rate, band-limited (its spectrum zero-padded).
fn band_limited_up(x: &[f64], factor: usize) -> Vec<f64> {
    use stave_dsp::primitives::spectral::Complex;
    if factor == 1 {
        return x.to_vec();
    }
    let n = x.len().next_power_of_two();
    let (small, big) = (stave_dsp::Fft::new(n), stave_dsp::Fft::new(n * factor));
    let mut buf = x.to_vec();
    buf.resize(n, 0.0);
    let mut spectrum = vec![Complex::new(0.0, 0.0); n / 2 + 1];
    small.forward_real(&buf, &mut spectrum);
    spectrum[n / 2] = spectrum[n / 2].scale(0.5);
    spectrum.resize(n * factor / 2 + 1, Complex::new(0.0, 0.0));
    let mut out = vec![0.0; n * factor];
    big.inverse_real(&mut spectrum, &mut out);
    out.truncate(x.len() * factor);
    out.iter().map(|v| v / n as f64).collect()
}

/// `x` at `1/factor` of its rate, everything above the new Nyquist
/// removed first.
fn band_limited_down(x: &[f64], factor: usize) -> Vec<f64> {
    use stave_dsp::primitives::spectral::Complex;
    if factor == 1 {
        return x.to_vec();
    }
    let m = x.len().next_power_of_two().max(2 * factor);
    let n = m / factor;
    let (big, small) = (stave_dsp::Fft::new(m), stave_dsp::Fft::new(n));
    let mut buf = x.to_vec();
    buf.resize(m, 0.0);
    let mut spectrum = vec![Complex::new(0.0, 0.0); m / 2 + 1];
    big.forward_real(&buf, &mut spectrum);
    spectrum.truncate(n / 2 + 1);
    spectrum[n / 2] = Complex::new(0.0, 0.0);
    let mut out = vec![0.0; n];
    small.inverse_real(&mut spectrum, &mut out);
    out.truncate(x.len() / factor);
    out.iter().map(|v| v / m as f64).collect()
}
