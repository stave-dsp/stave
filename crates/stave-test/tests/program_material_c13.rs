//! C13 on program material (DSP.md R22): the reference corpus's roles
//! through a circuit, each run judged as a null test against a reference
//! run by the residual's NMR under it (`stave_test::corpus::c13`). The
//! audio is never committed; without the library (or a role's material)
//! each case prints why it skipped and passes.
//!
//! Two comparisons on the diode clipper driven 6 dB past its knee:
//!
//! - `Circuit<Wdf>` against `Circuit<Dk>` at the material's rate: the
//!   same equations, so on real material too the residual must sit far
//!   under the mask (held: NMR ≤ −20 dB).
//! - `Circuit<Dk>` at the material's rate against itself at 4×,
//!   band-limited: what the shipping rate costs on real material
//!   (bilinear warping, aliasing), printed as a finding.

use std::path::Path;

use stave_measure::identify::periodic::resample;
use stave_test::characterise::{self, model, Plan};
use stave_test::corpus::c13::NullNmr;
use stave_test::corpus::{Corpus, Material};

const ROLES: [&str; 8] = ["guitar_di", "bass_di", "bass_clean", "guitar_amped", "drums_loop", "mix_bus", "vocal", "piano"];
const DECK: &str = "diode_clipper";
/// Samples played: a power of two (the band-limited resampling treats it
/// as one period), about 2.7 s at 48 kHz.
const LEN: usize = 1 << 17;

/// The material, mono, `LEN` samples with 10 ms fades (so the period
/// wraps without a step), scaled to peak at `peak`.
fn excerpt(m: &Material, peak: f64) -> Vec<f64> {
    let mono = m.mono();
    let mut x: Vec<f64> = mono.iter().take(LEN).map(|&v| f64::from(v)).collect();
    x.resize(LEN, 0.0);
    let fade = (0.01 * f64::from(m.rate)) as usize;
    for i in 0..fade {
        let g = 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / fade as f64).cos();
        x[i] *= g;
        x[LEN - 1 - i] *= g;
    }
    let top = x.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-12);
    x.iter_mut().for_each(|v| *v *= peak / top);
    x
}

fn first(corpus: &Corpus, role: &str) -> Option<Material> {
    let item = match corpus.role(role) {
        Ok(items) => items.into_iter().next()?,
        Err(skip) => {
            println!("{role}: {skip}");
            return None;
        }
    };
    Some(corpus.load(&item).unwrap_or_else(|e| panic!("{role}: {e}")))
}

#[test]
fn program_material_through_the_diode_clipper() {
    let corpus = match Corpus::open() {
        Ok(c) => c,
        Err(skip) => return println!("{skip}"),
    };
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("spice/{DECK}.plan"))).unwrap();
    let plan = Plan::parse(&text).unwrap();
    let knee = plan.knee.expect("the plan's knee").amplitude;
    let deck = characterise::deck(DECK).without_charge();
    for role in ROLES {
        let Some(m) = first(&corpus, role) else { continue };
        let rate = f64::from(m.rate);
        let x = excerpt(&m, 2.0 * knee);
        let mut dk = model::circuit(&deck, &plan.input, rate);
        let mut wdf = model::wdf_circuit(&deck, &plan.input, rate);
        let y_dk: Vec<f64> = x.iter().map(|&v| dk.tick_f64(v)).collect();
        let y_wdf: Vec<f64> = x.iter().map(|&v| wdf.tick_f64(v)).collect();
        assert!(y_dk.iter().chain(&y_wdf).all(|y| y.is_finite()), "{role}: not finite");
        let methods = NullNmr::of(&y_dk, &y_wdf, rate);
        let mut fine = model::circuit(&deck, &plan.input, 4.0 * rate);
        let up: Vec<f64> = resample(&x, 4 * LEN).iter().map(|&v| fine.tick_f64(v)).collect();
        let shipping = NullNmr::of(&resample(&up, LEN), &y_dk, rate);
        println!("{role}: {}", m.describe());
        println!("  WDF vs DK at {rate} Hz: {}", methods.describe());
        println!("  FINDING DK at {rate} Hz vs 4x: {}", shipping.describe());
        assert!(methods.masked(-20.0), "{role}: WDF and DK part on program material: {}", methods.describe());
    }
}
