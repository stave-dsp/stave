//! Anti-aliasing budgets (DSP.md R5): every nonlinear stage and every
//! oscillator, at 44.1 and 48 kHz with its default anti-aliasing, held to
//! a budget by audibility first: the A-weighted noise-to-mask ratio
//! (ANMR, Kahles et al. 2019; DSP.md R19) of a tone stepped over the
//! piano's range (27.5 Hz – 4186 Hz, 15 steps; 8 for the circuits) at or
//! under −10 dB, where aliasing becomes audible
//! ([`stave_measure::aliasing::anmr`]). A row above it is a finding
//! listed in `ANMR_FINDINGS` with its recorded ANMR and a budget 3 dB
//! above, so it cannot get worse unnoticed: mostly the comparison rows at
//! 1× and 2×, the polyBLEP oscillators, the ADAA curves at 1× and two of
//! the learned fixtures. Second, every row keeps its budget on the worst
//! inharmonic energy of a stepped tone (A4,
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
//!
//! Learned stages (M7) run as they ship: at their own rate, no
//! oversampler — a capture network's aliasing at its trained rate is
//! part of what it learned. The `.nam` fixtures (`tests/reference/nam/`)
//! and random networks of the standard sizes (Neural Amp Modeler's
//! 16 + 8-channel WaveNet with weights in ±0.125 — the datasheet's ±0.5
//! saturates all twenty layers into a fold generator no trained capture
//! resembles — and the datasheet's one-layer 16-unit LSTM and GRU) are measured at 44.1 and 48 kHz as if trained
//! there (the same weights, `RatePolicy::Any`), and each recurrent
//! network once more as a 48 kHz network at 44.1 kHz through
//! `RatePolicy::Interpolated`. The figures are not a quality ranking —
//! random weights alias as they happen to — but a drift detector; each
//! row also prints Sato & Smith's aliasing-to-signal ratio (ASR).
//!
//! The circuits (DSP.md A1) run every way instead, so each row compares:
//! 1×, 2× and 4× (`Oversampled::linear`), each without and with stateful
//! ADAA, one test per circuit (measured on an x86-64 Linux build host,
//! 2026-11-12; the same rule for budgets).

use stave_dsp::circuit::methods::adaa::Antialiasing;
use stave_dsp::circuit::methods::dk::Dk;
use stave_dsp::circuit::netlist::Netlist;
use stave_dsp::circuit::Circuit;
use stave_dsp::primitives::filters::korg35::Korg35;
use stave_dsp::primitives::filters::ladder::{Huovilainen, Stinchcombe};
use stave_dsp::primitives::filters::sallen_key::SallenKey;
use stave_dsp::units::{db, hz, Db, Hz};
use stave_dsp::{
    Adaa, AdaaOrder, CurveStage, DiodeClipper, DiodeClipperConfig, Osc, Oversampled,
    Oversampled2x, PairSolver, Shockley, SmoothSat, Stage, Triode, Waveform, Wavetable,
};
use stave_dsp::learned::{nam, Gru, LoadError, RatePolicy};
use stave_measure::aliasing::{Aliasing, AnmrSweep, SteppedTone};
use stave_measure::perceptual::nmr::AUDIBLE_ANMR_DB;
use stave_measure::system::System;

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

/// The circuits (DSP.md A1), the same stepped tone, at 1×, 2× and 4×
/// (`Oversampled::linear`), each without and with stateful ADAA
/// (`Antialiasing::Adaa`): the WDF clipper's root and the op-amp
/// clipper's one-port in closed form, the rest by Lobatto quadrature
/// (`Dk::adaa_mean`). Drives: the clippers ×10 and ×1, the cathode's
/// grid ×4 (2 V peak), the Fuzz Face 0.5 V, the Tube Screamer 0.5 V on
/// its 4.5 V reference. The quadrature rows gain far less than closed
/// form, and the cathode at 4× is at the floor either way: findings,
/// recorded as measured, not budgets to chase.
const CIRCUITS: &[Budget] = &[
    row("DiodeClipper x10 (WDF) 1x", 44_100.0, -7.0, -9.9, -9.8),
    row("DiodeClipper x10 (WDF) 1x", 48_000.0, -7.0, -9.0, -9.0),
    row("DiodeClipper x10 (WDF) 1x ADAA", 44_100.0, -26.0, -28.4, -27.6),
    row("DiodeClipper x10 (WDF) 1x ADAA", 48_000.0, -22.0, -24.9, -24.9),
    row("DiodeClipper x10 (WDF) 2x", 44_100.0, -22.0, -24.4, -24.4),
    row("DiodeClipper x10 (WDF) 2x", 48_000.0, -21.0, -23.4, -23.4),
    row("DiodeClipper x10 (WDF) 2x ADAA", 44_100.0, -37.0, -39.3, -39.3),
    row("DiodeClipper x10 (WDF) 2x ADAA", 48_000.0, -36.0, -38.1, -38.1),
    row("DiodeClipper x10 (WDF) 4x", 44_100.0, -41.0, -43.3, -39.2),
    row("DiodeClipper x10 (WDF) 4x", 48_000.0, -36.0, -38.0, -38.0),
    row("DiodeClipper x10 (WDF) 4x ADAA", 44_100.0, -62.0, -64.6, -60.8),
    row("DiodeClipper x10 (WDF) 4x ADAA", 48_000.0, -67.0, -69.5, -69.5),
    row("Circuit<Dk> diode clipper x10 (RS) 1x", 44_100.0, -7.0, -9.9, -9.9),
    row("Circuit<Dk> diode clipper x10 (RS) 1x", 48_000.0, -7.0, -9.0, -9.0),
    row("Circuit<Dk> diode clipper x10 (RS) 1x ADAA", 44_100.0, -22.0, -24.6, -24.6),
    row("Circuit<Dk> diode clipper x10 (RS) 1x ADAA", 48_000.0, -18.0, -20.7, -20.7),
    row("Circuit<Dk> diode clipper x10 (RS) 2x", 44_100.0, -22.0, -24.4, -24.4),
    row("Circuit<Dk> diode clipper x10 (RS) 2x", 48_000.0, -21.0, -23.4, -23.4),
    row("Circuit<Dk> diode clipper x10 (RS) 2x ADAA", 44_100.0, -40.0, -42.6, -40.2),
    row("Circuit<Dk> diode clipper x10 (RS) 2x ADAA", 48_000.0, -37.0, -39.0, -39.0),
    row("Circuit<Dk> diode clipper x10 (RS) 4x", 44_100.0, -41.0, -43.6, -39.4),
    row("Circuit<Dk> diode clipper x10 (RS) 4x", 48_000.0, -36.0, -38.2, -38.2),
    row("Circuit<Dk> diode clipper x10 (RS) 4x ADAA", 44_100.0, -55.0, -57.5, -51.9),
    row("Circuit<Dk> diode clipper x10 (RS) 4x ADAA", 48_000.0, -60.0, -62.0, -62.0),
    row("Circuit<Dk> op-amp clipper 1x", 44_100.0, -12.0, -14.4, -14.4),
    row("Circuit<Dk> op-amp clipper 1x", 48_000.0, -12.0, -14.4, -14.4),
    row("Circuit<Dk> op-amp clipper 1x ADAA", 44_100.0, -26.0, -28.4, -28.4),
    row("Circuit<Dk> op-amp clipper 1x ADAA", 48_000.0, -24.0, -26.7, -26.7),
    row("Circuit<Dk> op-amp clipper 2x", 44_100.0, -24.0, -26.8, -26.6),
    row("Circuit<Dk> op-amp clipper 2x", 48_000.0, -20.0, -22.9, -22.9),
    row("Circuit<Dk> op-amp clipper 2x ADAA", 44_100.0, -39.0, -41.2, -40.8),
    row("Circuit<Dk> op-amp clipper 2x ADAA", 48_000.0, -40.0, -42.0, -42.0),
    row("Circuit<Dk> op-amp clipper 4x", 44_100.0, -33.0, -35.7, -31.5),
    row("Circuit<Dk> op-amp clipper 4x", 48_000.0, -31.0, -33.0, -33.0),
    row("Circuit<Dk> op-amp clipper 4x ADAA", 44_100.0, -67.0, -69.2, -52.8),
    row("Circuit<Dk> op-amp clipper 4x ADAA", 48_000.0, -59.0, -61.1, -61.1),
    row("Circuit<Dk> common cathode x4 1x", 44_100.0, -30.0, -32.2, -18.1),
    row("Circuit<Dk> common cathode x4 1x", 48_000.0, -30.0, -32.2, -18.1),
    row("Circuit<Dk> common cathode x4 1x ADAA", 44_100.0, -46.0, -48.9, -27.5),
    row("Circuit<Dk> common cathode x4 1x ADAA", 48_000.0, -66.0, -68.7, -25.3),
    row("Circuit<Dk> common cathode x4 2x", 44_100.0, -66.0, -68.7, -68.7),
    row("Circuit<Dk> common cathode x4 2x", 48_000.0, -79.0, -81.4, -24.4),
    row("Circuit<Dk> common cathode x4 2x ADAA", 44_100.0, -96.0, -98.9, -94.0),
    row("Circuit<Dk> common cathode x4 2x ADAA", 48_000.0, -96.0, -98.0, -25.7),
    row("Circuit<Dk> common cathode x4 4x", 44_100.0, -130.0, -132.2, -94.3),
    row("Circuit<Dk> common cathode x4 4x", 48_000.0, -130.0, -132.7, -24.4),
    row("Circuit<Dk> common cathode x4 4x ADAA", 44_100.0, -131.0, -133.6, -94.6),
    row("Circuit<Dk> common cathode x4 4x ADAA", 48_000.0, -132.0, -134.0, -24.7),
    row("Circuit<Dk> fuzz face 1x", 44_100.0, -3.0, -5.1, -1.0),
    row("Circuit<Dk> fuzz face 1x", 48_000.0, -4.0, -6.7, -1.5),
    row("Circuit<Dk> fuzz face 1x ADAA", 44_100.0, -3.0, -5.9, -2.9),
    row("Circuit<Dk> fuzz face 1x ADAA", 48_000.0, -6.0, -8.3, -3.5),
    row("Circuit<Dk> fuzz face 2x", 44_100.0, -7.0, -9.3, -9.0),
    row("Circuit<Dk> fuzz face 2x", 48_000.0, -10.0, -12.7, -7.1),
    row("Circuit<Dk> fuzz face 2x ADAA", 44_100.0, -12.0, -14.1, -13.4),
    row("Circuit<Dk> fuzz face 2x ADAA", 48_000.0, -15.0, -17.0, -9.0),
    row("Circuit<Dk> fuzz face 4x", 44_100.0, -15.0, -17.0, -15.4),
    row("Circuit<Dk> fuzz face 4x", 48_000.0, -16.0, -18.5, -8.7),
    row("Circuit<Dk> fuzz face 4x ADAA", 44_100.0, -19.0, -21.1, -20.6),
    row("Circuit<Dk> fuzz face 4x ADAA", 48_000.0, -21.0, -23.0, -9.3),
    row("Circuit<Dk> tube screamer 1x", 44_100.0, -11.0, -13.7, -13.6),
    row("Circuit<Dk> tube screamer 1x", 48_000.0, -11.0, -13.5, -13.5),
    row("Circuit<Dk> tube screamer 1x ADAA", 44_100.0, -15.0, -17.6, -17.4),
    row("Circuit<Dk> tube screamer 1x ADAA", 48_000.0, -15.0, -17.7, -17.7),
    row("Circuit<Dk> tube screamer 2x", 44_100.0, -23.0, -25.7, -25.5),
    row("Circuit<Dk> tube screamer 2x", 48_000.0, -19.0, -21.0, -21.0),
    row("Circuit<Dk> tube screamer 2x ADAA", 44_100.0, -26.0, -28.1, -26.5),
    row("Circuit<Dk> tube screamer 2x ADAA", 48_000.0, -25.0, -27.4, -27.4),
    row("Circuit<Dk> tube screamer 4x", 44_100.0, -30.0, -32.1, -28.6),
    row("Circuit<Dk> tube screamer 4x", 48_000.0, -26.0, -28.9, -28.9),
    row("Circuit<Dk> tube screamer 4x ADAA", 44_100.0, -37.0, -39.4, -37.1),
    row("Circuit<Dk> tube screamer 4x ADAA", 48_000.0, -38.0, -40.0, -40.0),
];

/// The rows whose ANMR over the piano is above −10 dB — aliasing a
/// listener hears with the stage's defaults: `(stage, rate, budget,
/// recorded)`, the budget 3 dB above the recording. Every other row is
/// held to −10 dB. Most worst steps are the top of the piano, where a
/// hot tone's upper harmonics fold back below 20 kHz; the second-order
/// ADAA's is its lowest (27–29 Hz): broadband error that an A-weighted
/// low tone cannot mask. Measured 2026-11-13.
const ANMR_FINDINGS: &[(&str, f64, f64, f64)] = &[
    ("Triode (ADAA 1st order, 1x)", 44_100.0, 36.9, 33.9), // at 4194 Hz
    ("Triode (ADAA 1st order, 1x)", 48_000.0, 29.0, 26.0), // at 4189 Hz
    ("DiodeClipper x10 (Newton, 4x)", 44_100.0, -6.5, -9.5), // at 2923 Hz
    ("CurveStage tanh(6x) (ADAA 1st order, 1x)", 44_100.0, 14.3, 11.3), // at 4194 Hz
    ("Adaa 2nd order tanh(6x) (1x)", 44_100.0, 3.0, 0.0), // at 27 Hz
    ("Adaa 2nd order tanh(6x) (1x)", 48_000.0, 0.6, -2.4), // at 29 Hz
    ("DiodeClipper x10 (WDF) 1x", 44_100.0, 55.6, 52.6), // at 4194 Hz
    ("DiodeClipper x10 (WDF) 1x", 48_000.0, 52.9, 49.9), // at 4189 Hz
    ("DiodeClipper x10 (WDF) 1x ADAA", 44_100.0, 32.5, 29.5), // at 4194 Hz
    ("DiodeClipper x10 (WDF) 1x ADAA", 48_000.0, 26.9, 23.9), // at 4189 Hz
    ("DiodeClipper x10 (WDF) 2x", 44_100.0, 9.1, 6.1), // at 2040 Hz
    ("DiodeClipper x10 (WDF) 2x", 48_000.0, 17.6, 14.6), // at 4189 Hz
    ("DiodeClipper x10 (WDF) 2x ADAA", 48_000.0, -1.1, -4.1), // at 4189 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 1x", 44_100.0, 55.4, 52.4), // at 4194 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 1x", 48_000.0, 52.9, 49.9), // at 4189 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 1x ADAA", 44_100.0, 22.7, 19.7), // at 4194 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 1x ADAA", 48_000.0, 36.0, 33.0), // at 4189 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 2x", 44_100.0, 8.9, 5.9), // at 2040 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 2x", 48_000.0, 16.4, 13.4), // at 4189 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 2x ADAA", 44_100.0, -0.8, -3.8), // at 2040 Hz
    ("Circuit<Dk> diode clipper x10 (RS) 2x ADAA", 48_000.0, 5.9, 2.9), // at 4189 Hz
    ("Circuit<Dk> op-amp clipper 1x", 44_100.0, 51.8, 48.8), // at 4194 Hz
    ("Circuit<Dk> op-amp clipper 1x", 48_000.0, 51.9, 48.9), // at 4189 Hz
    ("Circuit<Dk> op-amp clipper 1x ADAA", 44_100.0, 28.8, 25.8), // at 4194 Hz
    ("Circuit<Dk> op-amp clipper 1x ADAA", 48_000.0, 18.5, 15.5), // at 4189 Hz
    ("Circuit<Dk> op-amp clipper 2x", 44_100.0, 10.6, 7.6), // at 2040 Hz
    ("Circuit<Dk> op-amp clipper 2x", 48_000.0, 24.9, 21.9), // at 4189 Hz
    ("Circuit<Dk> op-amp clipper 4x", 44_100.0, -5.1, -8.1), // at 2040 Hz
    ("Circuit<Dk> op-amp clipper 4x", 48_000.0, 2.0, -1.0), // at 4189 Hz
    ("Circuit<Dk> fuzz face 1x", 44_100.0, 61.7, 58.7), // at 4194 Hz
    ("Circuit<Dk> fuzz face 1x", 48_000.0, 56.2, 53.2), // at 4189 Hz
    ("Circuit<Dk> fuzz face 1x ADAA", 44_100.0, 55.6, 52.6), // at 4194 Hz
    ("Circuit<Dk> fuzz face 1x ADAA", 48_000.0, 52.1, 49.1), // at 4189 Hz
    ("Circuit<Dk> fuzz face 2x", 44_100.0, 37.1, 34.1), // at 4194 Hz
    ("Circuit<Dk> fuzz face 2x", 48_000.0, 44.8, 41.8), // at 4189 Hz
    ("Circuit<Dk> fuzz face 2x ADAA", 44_100.0, 35.2, 32.2), // at 4194 Hz
    ("Circuit<Dk> fuzz face 2x ADAA", 48_000.0, 42.3, 39.3), // at 4189 Hz
    ("Circuit<Dk> fuzz face 4x", 44_100.0, 34.4, 31.4), // at 4194 Hz
    ("Circuit<Dk> fuzz face 4x", 48_000.0, 42.5, 39.5), // at 4189 Hz
    ("Circuit<Dk> fuzz face 4x ADAA", 44_100.0, 32.1, 29.1), // at 4194 Hz
    ("Circuit<Dk> fuzz face 4x ADAA", 48_000.0, 40.5, 37.5), // at 4189 Hz
    ("Circuit<Dk> tube screamer 1x", 44_100.0, 54.1, 51.1), // at 4194 Hz
    ("Circuit<Dk> tube screamer 1x", 48_000.0, 53.8, 50.8), // at 4189 Hz
    ("Circuit<Dk> tube screamer 1x ADAA", 44_100.0, 44.5, 41.5), // at 4194 Hz
    ("Circuit<Dk> tube screamer 1x ADAA", 48_000.0, 44.7, 41.7), // at 4189 Hz
    ("Circuit<Dk> tube screamer 2x", 44_100.0, 24.7, 21.7), // at 2040 Hz
    ("Circuit<Dk> tube screamer 2x", 48_000.0, 31.8, 28.8), // at 4189 Hz
    ("Circuit<Dk> tube screamer 2x ADAA", 44_100.0, 18.3, 15.3), // at 2040 Hz
    ("Circuit<Dk> tube screamer 2x ADAA", 48_000.0, 21.7, 18.7), // at 4189 Hz
    ("Circuit<Dk> tube screamer 4x", 44_100.0, 13.2, 10.2), // at 2040 Hz
    ("Circuit<Dk> tube screamer 4x", 48_000.0, 13.9, 10.9), // at 4189 Hz
    ("Circuit<Dk> tube screamer 4x ADAA", 44_100.0, -4.3, -7.3), // at 2040 Hz
    ("Circuit<Dk> tube screamer 4x ADAA", 48_000.0, 2.2, -0.8), // at 4189 Hz
    ("Osc saw (polyBLEP)", 44_100.0, 9.9, 6.9), // at 4194 Hz
    ("Osc saw (polyBLEP)", 48_000.0, 7.6, 4.6), // at 4189 Hz
    ("Osc square (polyBLEP)", 44_100.0, 8.5, 5.5), // at 4194 Hz
    ("Osc square (polyBLEP)", 48_000.0, 7.8, 4.8), // at 4189 Hz
    ("Osc saw hard sync (polyBLEP)", 44_100.0, 12.6, 9.6), // at 4194 Hz
    ("Osc saw hard sync (polyBLEP)", 48_000.0, 10.1, 7.1), // at 4189 Hz
    ("WaveNet packed (fixture)", 44_100.0, 33.3, 30.3), // at 4194 Hz
    ("WaveNet packed (fixture)", 48_000.0, 28.3, 25.3), // at 4189 Hz
    ("WaveNet mixed (fixture)", 44_100.0, 49.5, 46.5), // at 4194 Hz
    ("WaveNet mixed (fixture)", 48_000.0, 46.2, 43.2), // at 4189 Hz
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

/// The learned stages, the same stepped tone as the nonlinear stages.
const LEARNED: &[Budget] = &[
    row("WaveNet standard (fixture)", 44_100.0, -30.0, -32.2, -14.3),
    row("WaveNet standard (fixture)", 48_000.0, -29.0, -31.7, -13.6),
    row("WaveNet packed (fixture)", 44_100.0, -16.0, -18.6, -17.0),
    row("WaveNet packed (fixture)", 48_000.0, -17.0, -19.5, -16.1),
    row("WaveNet mixed (fixture)", 44_100.0, -3.0, -5.2, -5.2),
    row("WaveNet mixed (fixture)", 48_000.0, -3.0, -5.4, -5.0),
    row("Slimmable WaveNet, 1 of 4 channels (fixture)", 44_100.0, -48.0, -50.8, -17.3),
    row("Slimmable WaveNet, 1 of 4 channels (fixture)", 48_000.0, -49.0, -51.1, -17.8),
    row("Slimmable WaveNet, 4 of 4 channels (fixture)", 44_100.0, -37.0, -39.3, -19.2),
    row("Slimmable WaveNet, 4 of 4 channels (fixture)", 48_000.0, -32.0, -34.7, -20.8),
    row("LSTM 2 x 5 (fixture)", 44_100.0, -41.0, -43.0, -29.6),
    row("LSTM 2 x 5 (fixture)", 48_000.0, -41.0, -43.4, -32.5),
    row("GRU 2 x 4 (fixture)", 44_100.0, -30.0, -32.4, -18.8),
    row("GRU 2 x 4 (fixture)", 48_000.0, -33.0, -35.3, -19.9),
    row("WaveNet NAM standard (random, ±0.125)", 44_100.0, -70.0, -72.4, -59.0),
    row("WaveNet NAM standard (random, ±0.125)", 48_000.0, -71.0, -73.4, -66.8),
    row("LSTM 1 x 16 (random)", 44_100.0, -42.0, -44.7, -16.7),
    row("LSTM 1 x 16 (random)", 48_000.0, -45.0, -47.0, -17.1),
    row("GRU 1 x 16 (random)", 44_100.0, -44.0, -46.9, -23.5),
    row("GRU 1 x 16 (random)", 48_000.0, -46.0, -48.9, -24.3),
    row("LSTM 2 x 5 (fixture), 48 kHz, Interpolated", 44_100.0, -39.0, -41.5, -25.7),
    row("GRU 2 x 4 (fixture), 48 kHz, Interpolated", 44_100.0, -28.0, -30.9, -23.1),
    row("LSTM 1 x 16 (random), 48 kHz, Interpolated", 44_100.0, -44.0, -46.3, -18.4),
    row("GRU 1 x 16 (random), 48 kHz, Interpolated", 44_100.0, -44.0, -46.9, -24.8),
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

fn measure_stage(name: &str, rate: f64, mode: Mode) -> Measured {
    let sweep = sweep_for(mode, stage_sweep(rate), ANMR_STEPS);
    let fs = rate as f32;
    match name {
        "Triode (ADAA 1st order, 1x)" => through(&sweep, mode, || {
            let mut t = Triode::new();
            t.prepare(fs);
            t.set_grid_drive(77.0);
            move |x: f32| t.tick(x)
        }),
        "DiodeClipper x10 (Newton, 4x)" => through(&sweep, mode, || {
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
        "SmoothSat (drive 1.6, 4x)" => through(&sweep, mode, || {
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
        "CurveStage tanh(6x) (ADAA 1st order, 1x)" => through(&sweep, mode, || {
            let mut c = tanh_curve();
            c.prepare(fs);
            move |x: f32| c.tick(x)
        }),
        "Adaa 2nd order tanh(6x) (1x)" => through(&sweep, mode, || {
            let c = tanh_curve();
            let mut adaa = Adaa::new(AdaaOrder::Second);
            move |x: f32| {
                let framed = c.curve().framed(0.0, 6.0, 1.0);
                adaa.tick(&framed, f64::from(x)) as f32
            }
        }),
        "Ladder (Huovilainen, +12 dB, 4x)" => {
            filter(sweep, mode, fs, 4, Huovilainen::new(hz(8_000.0), 0.5))
        }
        "Ladder (Stinchcombe, +12 dB, 4x)" => {
            filter(sweep, mode, fs, 4, Stinchcombe::new(hz(8_000.0), 0.5))
        }
        "SallenKey (+12 dB, 4x)" => filter(sweep, mode, fs, 4, SallenKey::new(hz(8_000.0), 0.5)),
        "Korg35 (+12 dB, 2x)" => filter(sweep, mode, fs, 2, Korg35::new(hz(8_000.0), 0.5)),
        other => panic!("no stage {other}"),
    }
}

/// A nonlinear filter (S1) at cutoff 8 kHz, resonance 0.5, +12 dB of
/// drive, in its documented default region: `Oversampled2x::front` at
/// `factor` 2, `front(nested(..))` at 4.
fn filter<F: Stage + Clone + Drive + 'static>(
    sweep: SteppedTone,
    mode: Mode,
    fs: f32,
    factor: u32,
    f: F,
) -> Measured {
    through(&sweep, mode, move || {
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

fn measure_oscillator(name: &str, rate: f64, mode: Mode) -> Measured {
    let sweep = sweep_for(mode, oscillator_sweep(rate), ANMR_STEPS);
    let fs = rate as f32;
    let osc = |make: fn(Waveform) -> Osc, shape: Waveform, sync: bool| {
        generated(&sweep, mode, move |hz| {
            let mut o = make(shape);
            o.prepare(fs);
            if sync {
                // The output repeats at the master: the measured tone.
                o.set_frequency(Hz::new((hz * 1.7) as f32));
                o.set_sync(Some(Hz::new(hz as f32)));
            } else {
                o.set_frequency(Hz::new(hz as f32));
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
            generated(&sweep, mode, move |hz| {
                let mut w = table.clone();
                w.prepare(fs);
                w.set_frequency(Hz::new(hz as f32));
                move |_: f32| w.tick(0.0)
            })
        }
        other => panic!("no oscillator {other}"),
    }
}

/// A circuit row's name: `"<circuit> <factor>x[ ADAA]"`.
fn measure_circuit(name: &str, rate: f64, mode: Mode) -> Measured {
    let (circuit, adaa) = match name.strip_suffix(" ADAA") {
        Some(c) => (c, true),
        None => (name, false),
    };
    let (circuit, factor) = circuit.rsplit_once(' ').expect("a factor");
    let factor: u32 = factor.trim_end_matches('x').parse().expect("a factor");
    let antialiasing = if adaa {
        Antialiasing::Adaa
    } else {
        Antialiasing::None
    };
    let sweep = sweep_for(mode, stage_sweep(rate), CIRCUIT_ANMR_STEPS);
    let fs = rate as f32;
    if circuit == "DiodeClipper x10 (WDF)" {
        return through(&sweep, mode, || {
            let mut clipper = DiodeClipper::new(DiodeClipperConfig {
                input_ohms: 2.2e3,
                capacitor_farads: 10e-9,
                diode: Shockley::N4148,
                solver: PairSolver::default(),
            });
            clipper.set_antialiasing(antialiasing);
            let mut os = Oversampled::linear(clipper, factor);
            os.prepare(fs);
            move |x: f32| os.tick(10.0 * x)
        });
    }
    // (deck, driven source, gain, the input's DC offset)
    let (deck, source, gain, offset) = match circuit {
        "Circuit<Dk> diode clipper x10 (RS)" => (
            include_str!("../../stave-test/spice/diode_clipper.cir"),
            "v1",
            10.0,
            0.0,
        ),
        "Circuit<Dk> op-amp clipper" => (
            include_str!("../../stave-test/spice/opamp_clipper.cir"),
            "v1",
            1.0,
            0.0,
        ),
        "Circuit<Dk> common cathode x4" => (
            include_str!("../../stave-test/spice/common_cathode.cir"),
            "v1",
            4.0,
            0.0,
        ),
        "Circuit<Dk> fuzz face" => (
            // The static deck: these budgets predate device charge (DSP.md R11).
            include_str!("../../stave-test/spice/fuzz_face_static.cir"),
            "vin",
            1.0,
            0.0,
        ),
        "Circuit<Dk> tube screamer" => (
            include_str!("../../stave-test/spice/tube_screamer.cir"),
            "v1",
            1.0,
            4.5,
        ),
        other => panic!("no circuit {other}"),
    };
    let netlist = Netlist::parse_spice(deck).unwrap();
    through(&sweep, mode, || {
        let mut c = Circuit::<Dk>::new(&netlist).unwrap();
        c.drive(source).unwrap();
        c.set_antialiasing(antialiasing);
        let mut os = Oversampled::linear(c, factor);
        os.prepare(fs);
        // Settle on the input's offset, then measure about where it rests
        // (a plate at 200 V is subtracted in f64, not lost in f32).
        for _ in 0..(rate as usize / 10) {
            os.tick(offset as f32);
        }
        let rest = f64::from(os.tick(offset as f32));
        move |x: f32| (f64::from(os.tick((offset + gain * f64::from(x)) as f32)) - rest) as f32
    })
}

/// The circuit rows whose name starts with `prefix`, at `rate`, with
/// ADAA or without.
fn circuit_rows(prefix: &str, rate: f64, adaa: bool) -> Vec<&'static Budget> {
    let rows: Vec<_> = CIRCUITS
        .iter()
        .filter(|b| b.stage.starts_with(prefix) && b.rate == rate && b.stage.ends_with(" ADAA") == adaa)
        .collect();
    assert!(!rows.is_empty(), "no rows for {prefix}");
    rows
}

/// Weights in ±0.5·`scale` from an LCG, rounded so the JSON stays short
/// (the datasheet's generator: at scale 1 the random networks are its
/// networks).
fn weights(n: usize, scale: f32) -> String {
    let mut lcg: u32 = 11;
    let w: Vec<String> = (0..n)
        .map(|_| {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            format!("{:.4}", scale * ((lcg >> 8) as f32 / (1u32 << 24) as f32 - 0.5))
        })
        .collect();
    format!("[{}]", w.join(","))
}

/// A `.nam` network with `{w}` standing for as many weights as the
/// loader says the architecture needs.
fn random_network(template: &str, scale: f32) -> nam::Network {
    let expected = match nam::parse(&template.replace("{w}", "[]")) {
        Err(LoadError::Weights { expected, .. }) => expected,
        other => panic!("{other:?}"),
    };
    nam::parse(&template.replace("{w}", &weights(expected, scale)))
        .unwrap()
        .network
}

/// A learned stage, as `nam::Network` or `Gru`, under a rate policy.
#[derive(Clone)]
enum Learned {
    Nam(nam::Network),
    Gru(Gru),
}

impl Learned {
    fn with(&self, policy: RatePolicy) -> Box<dyn Stage> {
        match self.clone() {
            Learned::Nam(mut n) => {
                n.set_rate_policy(policy);
                Box::new(n)
            }
            Learned::Gru(mut g) => {
                g.set_rate_policy(policy);
                Box::new(g)
            }
        }
    }
}

/// A learned stage by name (a row's name up to ", 48 kHz").
fn learned(name: &str) -> Learned {
    let file = |text: &str, size: usize| Learned::Nam(nam::parse(text).unwrap().network_at(size));
    match name.split(", 48 kHz").next().unwrap() {
        "WaveNet standard (fixture)" => file(include_str!("reference/nam/wavenet_standard.nam"), 0),
        "WaveNet packed (fixture)" => file(include_str!("reference/nam/wavenet_packed.nam"), 0),
        "WaveNet mixed (fixture)" => file(include_str!("reference/nam/wavenet_mixed.nam"), 0),
        "Slimmable WaveNet, 1 of 4 channels (fixture)" => {
            file(include_str!("reference/nam/slimmable_gated.nam"), 0)
        }
        "Slimmable WaveNet, 4 of 4 channels (fixture)" => {
            file(include_str!("reference/nam/slimmable_gated.nam"), 3)
        }
        "LSTM 2 x 5 (fixture)" => file(include_str!("reference/nam/lstm.nam"), 0),
        "GRU 2 x 4 (fixture)" => {
            Learned::Gru(Gru::parse(include_str!("reference/nam/gru.json")).unwrap())
        }
        "WaveNet NAM standard (random, ±0.125)" => {
            let array = |input: u32, channels: u32, head: u32, bias: bool| {
                format!(
                    r#"{{"input_size":{input},"condition_size":1,"channels":{channels},"head_size":{head},
                    "kernel_size":3,"dilations":[1,2,4,8,16,32,64,128,256,512],"activation":"Tanh",
                    "gated":false,"head_bias":{bias}}}"#
                )
            };
            Learned::Nam(random_network(&format!(
                r#"{{"version":"0.5.4","architecture":"WaveNet","sample_rate":48000,
                "config":{{"layers":[{},{}],"head":null,"head_scale":0.02}},"weights":{{w}}}}"#,
                array(1, 16, 8, false),
                array(16, 8, 1, true)
            ), 0.25))
        }
        "LSTM 1 x 16 (random)" => Learned::Nam(random_network(
            r#"{"version":"0.5.4","architecture":"LSTM","sample_rate":48000,
            "config":{"input_size":1,"hidden_size":16,"num_layers":1},"weights":{w}}"#,
            1.0,
        )),
        "GRU 1 x 16 (random)" => {
            let w = weights(3 * 16 * 17 + 6 * 16 + 17, 1.0);
            Learned::Gru(
                Gru::parse(&format!(
                    r#"{{"input_size":1,"hidden_size":16,"num_layers":1,"sample_rate":48000,"weights":{w}}}"#
                ))
                .unwrap(),
            )
        }
        other => panic!("no learned stage {other}"),
    }
}

/// A row ending "Interpolated" runs a 48 kHz network at the row's rate
/// through `RatePolicy::Interpolated`; any other runs the network as if
/// trained at the row's rate (`RatePolicy::Any`).
fn measure_learned(name: &str, rate: f64, mode: Mode) -> Measured {
    let policy = if name.ends_with("Interpolated") {
        RatePolicy::Interpolated
    } else {
        RatePolicy::Any
    };
    let network = learned(name);
    let fs = rate as f32;
    through(&sweep_for(mode, stage_sweep(rate), ANMR_STEPS), mode, move || {
        let mut net = network.with(policy);
        net.prepare(fs);
        move |x: f32| net.tick(x)
    })
}

fn check(rows: &[Budget], measure: impl Fn(&str, f64, Mode) -> Measured) {
    check_rows(rows.iter(), measure);
}

/// What a row measures: the stepped tone's figures in dB (the recorded
/// rows), or its ANMR over the piano (the audibility budget).
#[derive(Copy, Clone, Debug, PartialEq)]
enum Mode {
    Db,
    Anmr,
}

enum Measured {
    Db(Aliasing),
    Anmr(AnmrSweep),
}

impl Measured {
    fn db(self) -> Aliasing {
        match self {
            Measured::Db(a) => a,
            Measured::Anmr(_) => unreachable!("measured the ANMR"),
        }
    }

    fn anmr(self) -> AnmrSweep {
        match self {
            Measured::Anmr(a) => a,
            Measured::Db(_) => unreachable!("measured in dB"),
        }
    }
}

/// The ANMR's steps over the piano's range: about one every half
/// octave (every semitone is the characterisation's T1 sweep), and
/// about one an octave for the circuits, whose Newton solves at 4×
/// cost the most.
const ANMR_STEPS: usize = 15;
const CIRCUIT_ANMR_STEPS: usize = 8;

/// The row's sweep for `mode`: its own, or the piano's range in `steps`
/// with the row's amplitude, window and warm-up.
fn sweep_for(mode: Mode, base: SteppedTone, steps: usize) -> SteppedTone {
    match mode {
        Mode::Db => base,
        Mode::Anmr => SteppedTone {
            low_hz: 27.5,
            high_hz: 4_186.0,
            steps,
            ..base
        },
    }
}

fn through<S: System>(sweep: &SteppedTone, mode: Mode, make: impl FnMut() -> S) -> Measured {
    match mode {
        Mode::Db => Measured::Db(sweep.through(make)),
        Mode::Anmr => Measured::Anmr(sweep.anmr_through(make)),
    }
}

fn generated<G: System>(sweep: &SteppedTone, mode: Mode, make: impl FnMut(f64) -> G) -> Measured {
    match mode {
        Mode::Db => Measured::Db(sweep.generated(make)),
        Mode::Anmr => Measured::Anmr(sweep.anmr_generated(make)),
    }
}

/// The ANMR a row is held to: −10 dB (inaudible, Kahles et al. 2019),
/// or its finding's budget.
fn anmr_budget(stage: &str, rate: f64) -> (f64, Option<f64>) {
    ANMR_FINDINGS
        .iter()
        .find(|f| f.0 == stage && f.1 == rate)
        .map_or((AUDIBLE_ANMR_DB, None), |f| (f.2, Some(f.3)))
}

fn check_rows<'a>(
    rows: impl Iterator<Item = &'a Budget>,
    measure: impl Fn(&str, f64, Mode) -> Measured,
) {
    let mut failures = Vec::new();
    for b in rows {
        let anmr = measure(b.stage, b.rate, Mode::Anmr).anmr();
        let worst = anmr.worst();
        let (budget, recorded) = anmr_budget(b.stage, b.rate);
        println!(
            "{:<42} {:>4.1} kHz  ANMR {:>6.1} dB at {:>5.0} Hz (budget {:>6.1}{})",
            b.stage,
            b.rate / 1e3,
            worst.anmr_db,
            worst.hz,
            budget,
            recorded.map_or(String::new(), |r| format!(", FINDING recorded {r:.1}"))
        );
        if worst.anmr_db > budget {
            failures.push(format!(
                "{} at {} Hz: ANMR {:.1} dB at {:.0} Hz over its {:.1} dB budget",
                b.stage, b.rate, worst.anmr_db, worst.hz, budget
            ));
        }
        let report = measure(b.stage, b.rate, Mode::Db).db();
        let (audible, full) = (report.worst_audible(), report.worst());
        println!(
            "{:<42} {:>4.1} kHz  audible {:>7.1} dB at {:>5.0} Hz (budget {:>6.1}, recorded {:>6.1})  full band {:>7.1} dB (recorded {:>6.1}), loudest bin {:>6.1} dB at {:>5.0} Hz, ASR {:.2e} at {:>5.0} Hz",
            b.stage,
            b.rate / 1e3,
            audible.audible_db,
            audible.hz,
            b.budget_db,
            b.audible_db,
            full.inharmonic_db,
            b.full_db,
            full.worst_db,
            full.worst_hz,
            report.worst_asr().asr,
            report.worst_asr().hz
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

// The circuits (DSP.md A1): a test per circuit, rate and setting, so
// they run in parallel within the harness's time limit.

#[test]
#[cfg_attr(miri, ignore)]
fn wdf_diode_clipper_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("DiodeClipper x10 (WDF)", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn wdf_diode_clipper_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("DiodeClipper x10 (WDF)", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn wdf_diode_clipper_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("DiodeClipper x10 (WDF)", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn wdf_diode_clipper_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("DiodeClipper x10 (WDF)", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_diode_clipper_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> diode clipper", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_diode_clipper_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> diode clipper", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_diode_clipper_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> diode clipper", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_diode_clipper_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> diode clipper", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_opamp_clipper_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> op-amp clipper", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_opamp_clipper_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> op-amp clipper", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_opamp_clipper_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> op-amp clipper", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_opamp_clipper_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> op-amp clipper", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_common_cathode_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> common cathode", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_common_cathode_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> common cathode", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_common_cathode_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> common cathode", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_common_cathode_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> common cathode", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_fuzz_face_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> fuzz face", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_fuzz_face_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> fuzz face", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_fuzz_face_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> fuzz face", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_fuzz_face_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> fuzz face", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_tube_screamer_plain_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> tube screamer", 44_100.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_tube_screamer_adaa_44k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> tube screamer", 44_100.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_tube_screamer_plain_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> tube screamer", 48_000.0, false).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn dk_tube_screamer_adaa_48k_holds_its_alias_budgets() {
    check_rows(circuit_rows("Circuit<Dk> tube screamer", 48_000.0, true).into_iter(), measure_circuit);
}

#[test]
#[cfg_attr(miri, ignore)]
fn learned_stages_hold_their_alias_budgets() {
    check(LEARNED, measure_learned);
}
