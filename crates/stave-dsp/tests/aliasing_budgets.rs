//! Anti-aliasing budgets (DSP.md R5): every nonlinear stage and every
//! oscillator, at 44.1 and 48 kHz with its default anti-aliasing, held to
//! a budget on the worst inharmonic energy of a stepped tone (A4,
//! [`stave_measure::aliasing`]), in dB below the fundamental.
//!
//! The budget binds the inharmonic energy at or below 20 kHz, the
//! enforceable claim (stave-measure's crate docs say why: a half-band
//! decimator's shoulder folds onto the band above 20 kHz by design).
//! Each row records its budget beside the values measured when it was
//! set (the datasheet's reference machine, 2026-11-12), the audible figure and the full band's;
//! a budget sits 2 dB above its measurement, rounded up to a whole dB:
//! the margin absorbs platform `libm` differences, not regressions. The
//! test prints every row's current measurement.
//!
//! Defaults, as each stage documents them: the triode stage and the
//! curves run first-order ADAA (or second, as named) at the host rate,
//! with no oversampler; the diode clipper and the smooth saturator run
//! in a 4x region (`Oversampled2x::front(nested(..))`); the nonlinear
//! filters (S1) in the region each documents: the ladders and the
//! Sallen–Key at 4x, the Korg35 at 2x (`Oversampled2x::front`).

use stave_dsp::primitives::filters::korg35::Korg35;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::primitives::filters::sallen_key::SallenKey;
use stave_dsp::units::{db, hz, Db, Hz};
use stave_dsp::{
    Adaa, AdaaOrder, CurveStage, DiodeClipper, DiodeClipperConfig, Osc, Oversampled2x, PairSolver,
    Shockley, SmoothSat, Stage, Triode, Waveform, Wavetable,
};
use stave_measure::aliasing::{Aliasing, SteppedTone};

/// One stage's budget at one rate: the worst step's inharmonic energy
/// at or below 20 kHz must stay at or below `budget_db`. The record:
/// `audible_db`, that figure when the budget was set, and `full_db`, the
/// whole band's (which also counts the oversamplers' shoulder folding
/// onto itself above 20 kHz, and polyBLEP's folds next to Nyquist).
struct Budget {
    stage: &'static str,
    rate: f64,
    budget_db: f64,
    audible_db: f64,
    full_db: f64,
}

const fn row(
    stage: &'static str,
    rate: f64,
    budget_db: f64,
    audible_db: f64,
    full_db: f64,
) -> Budget {
    Budget {
        stage,
        rate,
        budget_db,
        audible_db,
        full_db,
    }
}

/// The nonlinear stages, a stepped tone from 500 Hz to 12 kHz.
const STAGES: &[Budget] = &[
    row("Triode (ADAA 1st order, 1x)", 44_100.0, -19.0, -21.8, -21.3),
    row("Triode (ADAA 1st order, 1x)", 48_000.0, -18.0, -20.2, -20.1),
    row(
        "DiodeClipper x10 (Newton, 4x)",
        44_100.0,
        -41.0,
        -43.3,
        -39.2,
    ),
    row(
        "DiodeClipper x10 (Newton, 4x)",
        48_000.0,
        -36.0,
        -38.0,
        -38.0,
    ),
    row(
        "SmoothSat (drive 1.6, 4x)",
        44_100.0,
        -101.0,
        -103.3,
        -103.3,
    ),
    row("SmoothSat (drive 1.6, 4x)", 48_000.0, -97.0, -99.9, -38.3),
    row(
        "CurveStage tanh(6x) (ADAA 1st order, 1x)",
        44_100.0,
        -28.0,
        -30.4,
        -30.4,
    ),
    row(
        "CurveStage tanh(6x) (ADAA 1st order, 1x)",
        48_000.0,
        -26.0,
        -28.0,
        -28.0,
    ),
    row(
        "Adaa 2nd order tanh(6x) (1x)",
        44_100.0,
        -30.0,
        -32.1,
        -32.1,
    ),
    row(
        "Adaa 2nd order tanh(6x) (1x)",
        48_000.0,
        -30.0,
        -32.9,
        -32.9,
    ),
    row("Ladder (Huovilainen, +12 dB, 4x)", 44_100.0, -66.0, -68.9, -68.9),
    row("Ladder (Huovilainen, +12 dB, 4x)", 48_000.0, -85.0, -87.5, -87.5),
    row("Ladder (Stinchcombe, +12 dB, 4x)", 44_100.0, -62.0, -64.4, -64.3),
    row("Ladder (Stinchcombe, +12 dB, 4x)", 48_000.0, -82.0, -84.9, -84.9),
    row("SallenKey (+12 dB, 4x)", 44_100.0, -93.0, -95.9, -61.9),
    row("SallenKey (+12 dB, 4x)", 48_000.0, -98.0, -100.6, -89.3),
    row("Korg35 (+12 dB, 2x)", 44_100.0, -77.0, -79.6, -79.6),
    row("Korg35 (+12 dB, 2x)", 48_000.0, -80.0, -82.9, -82.9),
];

/// The oscillators, stepped from 110 Hz to 5 kHz.
const OSCILLATORS: &[Budget] = &[
    row("Osc saw (polyBLEP)", 44_100.0, -21.0, -23.2, -23.2),
    row("Osc saw (polyBLEP)", 48_000.0, -25.0, -27.5, -21.1),
    row("Osc square (polyBLEP)", 44_100.0, -21.0, -23.9, -23.9),
    row("Osc square (polyBLEP)", 48_000.0, -32.0, -34.8, -22.1),
    row("Osc triangle (polyBLAMP)", 44_100.0, -36.0, -38.0, -38.0),
    row("Osc triangle (polyBLAMP)", 48_000.0, -50.0, -52.7, -36.1),
    row(
        "Osc saw hard sync (polyBLEP)",
        44_100.0,
        -12.0,
        -14.2,
        -14.2,
    ),
    row(
        "Osc saw hard sync (polyBLEP)",
        48_000.0,
        -19.0,
        -21.7,
        -12.3,
    ),
    row("Osc saw (minBLEP)", 44_100.0, -91.0, -93.5, -93.5),
    row("Osc saw (minBLEP)", 48_000.0, -92.0, -94.9, -92.6),
    row("Osc square (minBLEP)", 44_100.0, -94.0, -96.5, -96.5),
    row("Osc square (minBLEP)", 48_000.0, -95.0, -97.8, -96.4),
    row("Osc triangle (minBLAMP)", 44_100.0, -101.0, -103.7, -103.7),
    row("Osc triangle (minBLAMP)", 48_000.0, -103.0, -105.2, -104.7),
    row("Osc saw hard sync (minBLEP)", 44_100.0, -83.0, -85.5, -85.5),
    row("Osc saw hard sync (minBLEP)", 48_000.0, -87.0, -89.4, -84.3),
    row("Wavetable saw", 44_100.0, -122.0, -124.6, -124.6),
    row("Wavetable saw", 48_000.0, -122.0, -124.6, -124.6),
];

fn stage_sweep(rate: f64) -> SteppedTone {
    SteppedTone {
        sample_rate: rate,
        amplitude: 0.5,
        low_hz: 500.0,
        high_hz: 12_000.0,
        steps: 6,
        ..SteppedTone::default()
    }
}

fn oscillator_sweep(rate: f64) -> SteppedTone {
    SteppedTone {
        sample_rate: rate,
        low_hz: 110.0,
        high_hz: 5_000.0,
        steps: 6,
        ..SteppedTone::default()
    }
}

/// `tanh`, tabulated over ±8 and run at drive 6.
fn tanh_curve() -> CurveStage {
    let mut c = CurveStage::from_fn(-8.0, 8.0, 0.0, f64::tanh);
    c.pending_mut().drive = 6.0;
    c
}

fn measure_stage(name: &str, rate: f64) -> Aliasing {
    let sweep = stage_sweep(rate);
    let fs = rate as f32;
    match name {
        "Triode (ADAA 1st order, 1x)" => sweep.through(|| {
            let mut t = Triode::new();
            t.prepare(fs);
            t.set_grid_drive(77.0);
            move |x: f32| t.tick(x)
        }),
        "DiodeClipper x10 (Newton, 4x)" => sweep.through(|| {
            let clipper = DiodeClipper::new(DiodeClipperConfig {
                input_ohms: 2.2e3,
                capacitor_farads: 10e-9,
                diode: Shockley::N4148,
                solver: PairSolver::default(),
            });
            let mut os = Oversampled2x::front(Oversampled2x::nested(clipper));
            os.prepare(fs);
            move |x: f32| os.tick(10.0 * x)
        }),
        "SmoothSat (drive 1.6, 4x)" => sweep.through(|| {
            let mut s = SmoothSat::new();
            s.prepare(fs);
            s.set_drive(1.6);
            let mut os = Oversampled2x::front(Oversampled2x::nested(()));
            os.prepare(fs);
            move |x: f32| {
                let pre = s.tick_pre(x);
                let shaped = os.tick_with(pre, |inner, a| inner.tick_with(a, |(), b| s.shape(b)));
                s.tick_post(shaped)
            }
        }),
        "CurveStage tanh(6x) (ADAA 1st order, 1x)" => sweep.through(|| {
            let mut c = tanh_curve();
            c.prepare(fs);
            move |x: f32| c.tick(x)
        }),
        "Adaa 2nd order tanh(6x) (1x)" => sweep.through(|| {
            let c = tanh_curve();
            let mut adaa = Adaa::new(AdaaOrder::Second);
            move |x: f32| {
                let framed = c.curve().framed(0.0, 6.0, 1.0);
                adaa.tick(&framed, f64::from(x)) as f32
            }
        }),
        "Ladder (Huovilainen, +12 dB, 4x)" => {
            filter(sweep, fs, 4, Huovilainen::new(hz(8_000.0), 0.5))
        }
        "Ladder (Stinchcombe, +12 dB, 4x)" => {
            filter(sweep, fs, 4, Stinchcombe::new(hz(8_000.0), 0.5))
        }
        "SallenKey (+12 dB, 4x)" => filter(sweep, fs, 4, SallenKey::new(hz(8_000.0), 0.5)),
        "Korg35 (+12 dB, 2x)" => filter(sweep, fs, 2, Korg35::new(hz(8_000.0), 0.5)),
        other => panic!("no stage {other}"),
    }
}

/// A nonlinear filter (S1) at cutoff 8 kHz, resonance 0.5, +12 dB of
/// drive, in its documented default region: `Oversampled2x::front` at
/// `factor` 2, `front(nested(..))` at 4.
fn filter<F: Stage + Clone + Drive + 'static>(
    sweep: SteppedTone,
    fs: f32,
    factor: u32,
    f: F,
) -> Aliasing {
    sweep.through(move || {
        let mut f = f.clone();
        f.drive(db(12.0));
        let mut region: Box<dyn Stage> = if factor == 4 {
            Box::new(Oversampled2x::front(Oversampled2x::nested(f)))
        } else {
            Box::new(Oversampled2x::front(f))
        };
        region.prepare(fs);
        move |x: f32| region.tick(x)
    })
}

/// The filters' drive setter, for [`filter`].
trait Drive {
    fn drive(&mut self, drive: Db);
}

macro_rules! drive {
    ($($t:ty),*) => {$(
        impl Drive for $t {
            fn drive(&mut self, drive: Db) {
                self.set_drive(drive);
            }
        }
    )*};
}
drive!(Huovilainen, Stinchcombe, SallenKey, Korg35);

fn measure_oscillator(name: &str, rate: f64) -> Aliasing {
    let sweep = oscillator_sweep(rate);
    let fs = rate as f32;
    let osc = |make: fn(Waveform) -> Osc, shape: Waveform, sync: bool| {
        sweep.generated(move |hz| {
            let mut o = make(shape);
            o.prepare(fs);
            if sync {
                // The output repeats at the master: the measured tone.
                o.set_hz((hz * 1.7) as f32);
                o.set_sync(Some(Hz::new(hz as f32)));
            } else {
                o.set_hz(hz as f32);
            }
            move |_: f32| o.tick(0.0)
        })
    };
    match name {
        "Osc saw (polyBLEP)" => osc(Osc::new, Waveform::Saw, false),
        "Osc square (polyBLEP)" => osc(Osc::new, Waveform::Square, false),
        "Osc triangle (polyBLAMP)" => osc(Osc::new, Waveform::Triangle, false),
        "Osc saw hard sync (polyBLEP)" => osc(Osc::new, Waveform::Saw, true),
        "Osc saw (minBLEP)" => osc(Osc::min_blep, Waveform::Saw, false),
        "Osc square (minBLEP)" => osc(Osc::min_blep, Waveform::Square, false),
        "Osc triangle (minBLAMP)" => osc(Osc::min_blep, Waveform::Triangle, false),
        "Osc saw hard sync (minBLEP)" => osc(Osc::min_blep, Waveform::Saw, true),
        "Wavetable saw" => {
            let table = Wavetable::new(Waveform::Saw);
            sweep.generated(move |hz| {
                let mut w = table.clone();
                w.prepare(fs);
                w.set_hz(hz as f32);
                move |_: f32| w.tick(0.0)
            })
        }
        other => panic!("no oscillator {other}"),
    }
}

fn check(rows: &[Budget], measure: impl Fn(&str, f64) -> Aliasing) {
    let mut failures = Vec::new();
    for b in rows {
        let report = measure(b.stage, b.rate);
        let (audible, full) = (report.worst_audible(), report.worst());
        println!(
            "{:<42} {:>4.1} kHz  audible {:>7.1} dB at {:>5.0} Hz (budget {:>6.1}, recorded {:>6.1})  full band {:>7.1} dB (recorded {:>6.1}), loudest bin {:>6.1} dB at {:>5.0} Hz",
            b.stage,
            b.rate / 1e3,
            audible.audible_db,
            audible.hz,
            b.budget_db,
            b.audible_db,
            full.inharmonic_db,
            b.full_db,
            full.worst_db,
            full.worst_hz
        );
        if audible.audible_db > b.budget_db {
            failures.push(format!(
                "{} at {} Hz: {:.1} dB over its {:.1} dB budget",
                b.stage, b.rate, audible.audible_db, b.budget_db
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn nonlinear_stages_hold_their_alias_budgets() {
    check(STAGES, measure_stage);
}

#[test]
#[cfg_attr(miri, ignore)]
fn oscillators_hold_their_alias_budgets() {
    check(OSCILLATORS, measure_oscillator);
}
