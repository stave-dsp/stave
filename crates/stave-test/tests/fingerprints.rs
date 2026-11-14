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
use stave_test::characterise::fingerprint::{blessing, path, pin_stage, pin_stage_from, pin_stage_with, stage_plan, Fingerprint};
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

// ---- R9: the nonlinear, level-dependent learned and physical stages ----
//
// Each runs as it ships at 48 kHz (no oversampling region of its own:
// the networks and block models run at their trained rate, the tape
// machine and the driver oversample inside where they need to).

/// A Neural Amp Modeler fixture exported by the trainer (the same files
/// `stave-dsp/tests/nam_parity.rs` holds to the trainer's outputs).
fn nam_fixture(text: &str) -> Ticker {
    let mut net = stave_dsp::learned::nam::parse(text).unwrap().network;
    net.prepare(48_000.0);
    ticker(net)
}

/// Where a network's knee search starts: −60 dBFS. Each network's output
/// sits on a DC offset whose `f32` rounding reads, under a 10 µ tone,
/// as 1 % THD — a "knee" at the search's first level.
const NETWORK_KNEE_FROM: f64 = 1e-3;

#[test]
fn nam_wavenet_standard() {
    pin_stage_from(
        "stages/nam_wavenet_standard",
        "nam::Network (WaveNet, NAM standard) from the trainer's wavenet_standard.nam fixture, knee searched from -60 dBFS",
        Some("stave_dsp::learned::WaveNet"),
        |_| nam_fixture(include_str!("../../stave-dsp/tests/reference/nam/wavenet_standard.nam")),
        NETWORK_KNEE_FROM,
    );
}

#[test]
fn nam_lstm() {
    pin_stage_from(
        "stages/nam_lstm",
        "nam::Network (LSTM) from the trainer's lstm.nam fixture, knee searched from -60 dBFS",
        Some("stave_dsp::learned::Lstm"),
        |_| nam_fixture(include_str!("../../stave-dsp/tests/reference/nam/lstm.nam")),
        NETWORK_KNEE_FROM,
    );
}

#[test]
fn gru() {
    pin_stage_from(
        "stages/gru",
        "Gru from the torch-exported gru.json fixture, knee searched from -60 dBFS",
        Some("stave_dsp::learned::Gru"),
        |rate| {
            let mut net = stave_dsp::learned::Gru::parse(include_str!("../../stave-dsp/tests/reference/nam/gru.json")).unwrap();
            net.prepare(rate as f32);
            ticker(net)
        },
        NETWORK_KNEE_FROM,
    );
}

/// A lowpass biquad section at `hz` (Q 0.71) at 48 kHz, the datasheet's.
fn section(hz: f64) -> stave_dsp::learned::block::Section {
    let w = std::f64::consts::TAU * hz / 48_000.0;
    let alpha = w.sin() / 1.4;
    let a0 = 1.0 + alpha;
    stave_dsp::learned::block::Section {
        b: [(1.0 - w.cos()) / 2.0 / a0, (1.0 - w.cos()) / a0, (1.0 - w.cos()) / 2.0 / a0],
        a: [-2.0 * w.cos() / a0, (1.0 - alpha) / a0],
    }
}

#[test]
fn wiener_hammerstein() {
    use stave_dsp::learned::block::{Linear, Nonlinearity, WienerHammerstein};
    pin_stage(
        "stages/wiener_hammerstein",
        "WienerHammerstein, lowpasses at 700 Hz and 5 kHz, a 24-knot tanh spline over ±3, lowpasses at 9 and 12 kHz",
        Some("stave_dsp::learned::block::WienerHammerstein"),
        |rate| {
            let knots: Vec<f64> = (0..24).map(|k| (f64::from(k) / 23.0 * 2.0 - 1.0).powi(3) * 3.0).collect();
            let spline = Nonlinearity::Spline { values: knots.iter().map(|&u| u.tanh()).collect(), knots };
            let mut wh = WienerHammerstein::new(
                48_000.0,
                Linear::Rational(vec![section(700.0), section(5_000.0)]),
                spline,
                Linear::Rational(vec![section(9_000.0), section(12_000.0)]),
            );
            wh.prepare(rate as f32);
            ticker(wh)
        },
    );
}

/// Through the `Block` a model file reads into, so its dispatch is pinned too.
#[test]
fn parallel_hammerstein() {
    use stave_dsp::learned::block::{Block, Linear, ParallelHammerstein};
    pin_stage(
        "stages/parallel_hammerstein",
        "Block::ParallelHammerstein, order 5, two lowpass sections a branch at 2–6 kHz",
        Some("stave_dsp::learned::block::ParallelHammerstein"),
        |rate| {
            let branches = (0..5).map(|m| Linear::Rational(vec![section(2_000.0 + 1_000.0 * f64::from(m)); 2])).collect();
            let mut block = Block::from(ParallelHammerstein::new(48_000.0, branches));
            block.prepare(rate as f32);
            ticker(block)
        },
    );
}

/// The tape machine with its flutter off: the record side's hysteresis
/// (15× inside), the play head's losses, time-invariant. The machine
/// with its flutter is a golden digest (`golden.rs`).
#[test]
fn tape_record_and_play() {
    use stave_dsp::physical::tape::{Chowdhury, Machine};
    pin_stage(
        "stages/tape_tc260",
        "Chowdhury, TC-260 at 7.5 ips, flutter off",
        Some("stave_dsp::physical::tape::Chowdhury"),
        |rate| {
            let mut deck = Chowdhury::new(Machine::TC260);
            deck.set_flutter(0.0);
            deck.prepare(rate as f32);
            ticker(deck)
        },
    );
}

#[test]
fn driver_large_signal() {
    use stave_dsp::physical::speaker::{Driver, LargeSignal, ThieleSmall};
    pin_stage(
        "stages/driver_large_signal",
        "Driver, Jensen C12N (LR-2 coil), Klippel curves from limits 1.0/1.2/6 mm, 20 V a unit",
        Some("stave_dsp::physical::speaker::Driver"),
        |rate| {
            let mut d = Driver::new(ThieleSmall::JENSEN_C12N_LR2, LargeSignal::from_limits(1.0e-3, 1.2e-3, 6e-3));
            d.set_volts(20.0);
            d.prepare(rate as f32);
            ticker(d)
        },
    );
}
