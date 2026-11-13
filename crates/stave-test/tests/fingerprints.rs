//! Fingerprints (DSP.md R18) of the catalogued stage models, and the
//! currency of every committed fingerprint.
//!
//! The circuits' and hand topologies' fingerprints are pinned by
//! `characterise.rs` (their T0 matrix, against ngspice). Here, each
//! catalogued stage that has no deck is pinned as a black box at 48 kHz
//! in its shipping configuration: its knee found at 1 kHz, then the same
//! T0 plan (multisine at −40, 0, +12 dB re knee; a 1 kHz tone at −20, 0,
//! +12), its own R19 metrics (Rnonlin, GedLee, ANMR). A model that changes
//! fails here until it is re-blessed on purpose:
//!
//! ```text
//! STAVE_BLESS=1 cargo test -p stave-test --test fingerprints --test characterise
//! ```

use stave_dsp::primitives::filters::korg35::Korg35;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::primitives::filters::sallen_key::SallenKey;
use stave_dsp::units::{db, hz};
use stave_dsp::{CurveStage, Oversampled2x, SmoothSat, Stage, Triode};
use stave_test::characterise::fingerprint::{blessing, path, pin_stage, pin_stage_with, stage_plan, Fingerprint};
use stave_test::characterise::kernels::KernelPlan;
use stave_test::characterise::model::Ticker;
use stave_test::characterise::{Plan, Table};

/// A `Stage` as a ticker: f64 in, f32 through, f64 out.
fn ticker(mut s: impl Stage + 'static) -> Ticker {
    Box::new(move |x| f64::from(s.tick(x as f32)))
}

/// With its nonlinear kernels (C12, DSP.md M6) beside the tables.
#[test]
fn triode() {
    pin_stage_with(
        "stages/triode",
        "Triode, grid drive 77 V, first-order ADAA, at the host rate",
        Some("stave_dsp::circuit::topologies::triode_stage::Triode"),
        |rate| {
            let mut t = Triode::new();
            t.prepare(rate as f32);
            t.set_grid_drive(77.0);
            ticker(t)
        },
        Some(&KernelPlan::default()),
    );
}

#[test]
fn smooth_sat() {
    pin_stage(
        "stages/smooth_sat",
        "SmoothSat, drive 1.6, in a 4x region (Oversampled2x::front(nested))",
        Some("stave_dsp::behavioral::smooth_sat::SmoothSat"),
        |rate| {
            let mut s = SmoothSat::new();
            s.prepare(rate as f32);
            s.set_drive(1.6);
            let mut os = Oversampled2x::front(Oversampled2x::nested(()));
            os.prepare(rate as f32);
            Box::new(move |x| {
                let pre = s.tick_pre(x as f32);
                let shaped = os.tick_with(pre, |inner, a| inner.tick_with(a, |(), b| s.shape(b)));
                f64::from(s.tick_post(shaped))
            })
        },
    );
}

#[test]
fn curve_stage() {
    pin_stage(
        "stages/curve_stage_tanh",
        "CurveStage of tanh over ±8, drive 6, first-order ADAA, at the host rate",
        Some("stave_dsp::behavioral::curve::CurveStage"),
        |rate| {
            let mut c = CurveStage::from_fn(-8.0, 8.0, 0.0, f64::tanh);
            c.pending_mut().drive = 6.0;
            c.prepare(rate as f32);
            ticker(c)
        },
    );
}

/// A nonlinear filter at cutoff 8 kHz, resonance 0.5, +12 dB of drive,
/// in its documented region (4x: `front(nested(..))`, 2x: `front`).
fn filter<F: Stage + 'static>(f: F, four: bool, rate: f64) -> Ticker {
    let mut region: Box<dyn Stage> = if four {
        Box::new(Oversampled2x::front(Oversampled2x::nested(f)))
    } else {
        Box::new(Oversampled2x::front(f))
    };
    region.prepare(rate as f32);
    Box::new(move |x| f64::from(region.tick(x as f32)))
}

#[test]
fn ladder_huovilainen() {
    pin_stage(
        "stages/ladder_huovilainen",
        "Huovilainen ladder, 8 kHz, resonance 0.5, +12 dB drive, in a 4x region",
        Some("stave_dsp::primitives::filters::ladder::Huovilainen"),
        |rate| {
            let mut f = Huovilainen::new(hz(8_000.0), 0.5);
            f.set_drive(db(12.0));
            filter(f, true, rate)
        },
    );
}

#[test]
fn ladder_stinchcombe() {
    pin_stage(
        "stages/ladder_stinchcombe",
        "Stinchcombe ladder, 8 kHz, resonance 0.5, +12 dB drive, in a 4x region",
        Some("stave_dsp::primitives::filters::ladder::Stinchcombe"),
        |rate| {
            let mut f = Stinchcombe::new(hz(8_000.0), 0.5);
            f.set_drive(db(12.0));
            filter(f, true, rate)
        },
    );
}

#[test]
fn sallen_key() {
    pin_stage(
        "stages/sallen_key",
        "Sallen-Key, 8 kHz, resonance 0.5, +12 dB drive, in a 4x region",
        Some("stave_dsp::primitives::filters::sallen_key::SallenKey"),
        |rate| {
            let mut f = SallenKey::new(hz(8_000.0), 0.5);
            f.set_drive(db(12.0));
            filter(f, true, rate)
        },
    );
}

#[test]
fn korg35() {
    pin_stage(
        "stages/korg35",
        "Korg35, 8 kHz, resonance 0.5, +12 dB drive, in a 2x region",
        Some("stave_dsp::primitives::filters::korg35::Korg35"),
        |rate| {
            let mut f = Korg35::new(hz(8_000.0), 0.5);
            f.set_drive(db(12.0));
            filter(f, false, rate)
        },
    );
}

/// Every committed fingerprint's metrics and plots are the ones its own
/// tables give with today's metric implementations (DSP.md R19's
/// versions): a metric that changes shows here as a stale fingerprint,
/// to re-bless — never as a model that moved. `STAVE_BLESS=1` rewrites
/// them from the committed tables.
#[test]
fn fingerprints_are_current() {
    let root = path("x").parent().unwrap().to_path_buf();
    let mut files = Vec::new();
    let mut dirs = vec![root];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "fingerprint") {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(files.len() >= 20, "{} fingerprints", files.len());
    let spice = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("spice");
    let stale: Vec<String> = std::thread::scope(|scope| {
        let jobs: Vec<_> = files
            .iter()
            .map(|file| {
                let spice = spice.clone();
                scope.spawn(move || {
                    let fp = Fingerprint::parse(&std::fs::read_to_string(file).unwrap())
                        .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
                    assert_eq!(path(&fp.case), *file, "{}: names case {}", file.display(), fp.case);
                    let (plan, reference) = if fp.reference == "none" {
                        let knee = fp.knee.expect("a black box records its knee");
                        (stage_plan(&fp.case, knee, fp.settle.expect("and its settling")), None)
                    } else {
                        let circuit = fp.case.split('/').next().unwrap();
                        let read = |ext: &str| std::fs::read_to_string(spice.join(format!("{circuit}.{ext}"))).unwrap();
                        (Plan::parse(&read("plan")).unwrap(), Some(Table::parse(&read("table")).unwrap()))
                    };
                    if blessing() {
                        // Re-derive the metrics and plots from the committed
                        // tables (the tables themselves stay as measured).
                        fp.derived(&plan, reference.as_ref()).write();
                        return Vec::new();
                    }
                    fp.stale(&plan, reference.as_ref())
                        .into_iter()
                        .map(|s| format!("{}: {s}", fp.case))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter().flat_map(|j| j.join().unwrap()).collect()
    });
    assert!(
        stale.is_empty(),
        "stale fingerprints (re-bless with STAVE_BLESS=1 cargo test -p stave-test --test characterise --test fingerprints):\n  {}",
        stale.join("\n  ")
    );
}
