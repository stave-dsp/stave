//! Every case's budgets, per tier. The primary one is audibility
//! (DSP.md R16, `Audibility`): the null residual — reference minus the
//! model, level and latency aligned — under the reference as its
//! masker, at most −10 dB NMR in total and in its worst band (`(−10,
//! −10)`: the residual at least 10 dB under BS.1387's masked threshold,
//! the margin `stave_measure::perceptual::null` argues). A case over it
//! is a finding, held at the measured NMR plus 5 dB rounded up to 5. The
//! numeric budgets beside it are the regression guards: the measured
//! error with a margin
//! (three times, rounded up to 1-2-5; aliasing ceilings 2 dB above the
//! measured level; group delay at the audibility budget, 0.2 ms from
//! 500 Hz to 4 kHz; ANMR at Kahles et al.'s audibility threshold, −10 dB,
//! where the model is under it, else 3 dB above the measured level), as
//! `(check, rates, first, second)` — the second is degrees (dB of probe
//! level for dynamics), `NAN` where the check has none. Measured against
//! the ngspice 42 tables; rows marked "R11" were re-measured when the
//! transistor decks gained their cards' charge (their `*_static` twins
//! keep the static rows). A row that holds a finding says so beside it:
//! its budget is the measured value with the same margin, so the finding
//! cannot get worse unnoticed; DSP.md R17 lists them. Errors at shipping
//! rates are the model as it ships (DSP.md R12, `SHIPPING` in
//! `circuits/mod.rs`) at 44.1, 48 and 96 kHz: run at its oversampling
//! and read through an ideal decimator — what the plugin will have.
//! Re-measured for R12 and tightened; two checks were raised (on four
//! rows: each deck and its `_static` twin), each marked
//! "RAISED" with why.

use stave_test::characterise::Check::{self, *};
use stave_test::characterise::Rates::{self, *};
use stave_test::characterise::Tier;

const NAN: f64 = f64::NAN;

pub type Row = (Check, Rates, f64, f64);

pub fn table(case: &str, tier: Tier) -> &'static [Row] {
    match (case, tier) {
        ("diode_clipper", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -38.6 / -20.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -51.0 / -33.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 1.0, 2.0), // 0.173 / 0.512
            (Bla, Shipping, 0.2, 0.5), // 0.0414 / 0.126 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 5e-4, 0.02), // 0.000109 / 0.0054
            (Compression, Shipping, 5e-4, 0.02), // 0.000118 / 0.00585
            (Detection, Oversampled, 0.05, NAN), // 0.0122
            (Detection, Shipping, 0.01, NAN), // 0.00297
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.73e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 1.69e-08
            (Harmonics, Oversampled, 0.2, 2.0), // 0.0788 / 0.402
            (Harmonics, Shipping, 0.1, 0.5), // 0.0189 / 0.111
            (OperatingPoint, Oversampled, 1e-9, NAN), // 1.06e-25
            (OperatingPoint, Shipping, 1e-9, NAN), // 1.06e-25
            (Response, Oversampled, 0.02, 0.1), // 0.0058 / 0.0228 — FINDING: the deck's 4 pF Cjo, which DK leaves out (R11)
            (Response, Shipping, 0.2, 0.5), // 0.043 / 0.126 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.005, NAN), // 0.00129
            (ThdN, Shipping, 0.005, NAN), // 0.00141 — aliasing as it ships
        ],
        ("ce_stage", Tier::T0) => &[
            (Aliasing, Oversampled, -104.0, NAN), // -106
            (Aliasing, Shipping, -102.0, NAN), // -104 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -18.5 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -82.2 / -64.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -79.6 / -62.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.000967 / 0.0102
            (Bla, Shipping, 0.01, 0.05), // 0.00221 / 0.0102
            (Compression, Oversampled, 1e-5, 1e-4), // 2.88e-06 / 1.73e-05
            (Compression, Shipping, 1e-5, 2e-4), // 3.01e-06 / 6.66e-05
            (Detection, Oversampled, 50.0, NAN), // 9.28
            (Detection, Shipping, 50.0, NAN), // 9.28
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.56e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 2.21e-10
            (Harmonics, Oversampled, 0.05, 0.01), // 0.0135 / 0.0023
            (Harmonics, Shipping, 0.05, 0.02), // 0.0135 / 0.006
            (OperatingPoint, Oversampled, 5e-6, NAN), // 9.02e-07
            (OperatingPoint, Shipping, 5e-6, NAN), // 9.02e-07
            (Response, Oversampled, 5e-6, 1e-5), // 7.76e-07 / 1.77e-06
            (Response, Shipping, 2e-5, 0.02), // 3.68e-06 / 0.00376 (R11: the whole card)
            (ThdN, Oversampled, 2e-4, NAN), // 3.98e-05
            (ThdN, Shipping, 2e-4, NAN), // 4.6e-05 — aliasing as it ships
        ],
        ("ce_stage_static", Tier::T0) => &[
            (Aliasing, Oversampled, -99.0, NAN), // -101
            (Aliasing, Shipping, -102.0, NAN), // -104 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -18.5 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -68.7 / -54.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -80.4 / -63.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.1), // 0.00143 / 0.0271
            (Bla, Shipping, 0.005, 0.05), // 0.00141 / 0.0102
            (Compression, Oversampled, 2e-5, 0.001), // 4.01e-06 / 0.000253
            (Compression, Shipping, 1e-5, 2e-4), // 2.99e-06 / 6.6e-05
            (Detection, Oversampled, 50.0, NAN), // 9.28
            (Detection, Shipping, 50.0, NAN), // 9.28
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.58e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 1.78e-10
            (Harmonics, Oversampled, 0.05, 0.05), // 0.0135 / 0.0118
            (Harmonics, Shipping, 0.05, 0.02), // 0.0135 / 0.00584
            (OperatingPoint, Oversampled, 5e-6, NAN), // 9.02e-07
            (OperatingPoint, Shipping, 5e-6, NAN), // 9.02e-07
            (Response, Oversampled, 5e-6, 1e-5), // 7.76e-07 / 1.77e-06
            (Response, Shipping, 2e-6, 0.005), // 4.33e-07 / 0.00104
            (ThdN, Oversampled, 2e-4, NAN), // 5.99e-05
            (ThdN, Shipping, 2e-4, NAN), // 4.62e-05 — aliasing as it ships
        ],
        ("common_cathode", Tier::T0) => &[
            (Aliasing, Oversampled, -110.0, NAN), // -112
            (Aliasing, Shipping, -102.0, NAN), // -104 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -25.5 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -63.5 / -51.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -50.9 / -38.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000969 / 0.000373
            (Bla, Shipping, 0.005, 0.02), // 0.00097 / 0.00649
            (Compression, Oversampled, 1e-5, 1e-4), // 3.03e-06 / 2.14e-05
            (Compression, Shipping, 2e-5, 0.002), // 3.76e-06 / 0.000348
            (Detection, Oversampled, 0.02, NAN), // 0.00421
            (Detection, Shipping, 0.05, NAN), // 0.0119
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.43e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 1.03e-09
            (Harmonics, Oversampled, 1.0, 0.02), // 0.239 / 0.00588
            (Harmonics, Shipping, 5.0, 0.1), // 1.13 / 0.0245
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.21e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.21e-09
            (Response, Oversampled, 5e-9, 1e-8), // 7.57e-10 / 2.25e-09
            (Response, Shipping, 2e-6, 0.02), // 7.87e-07 / 0.00648
            (ThdN, Oversampled, 2e-4, NAN), // 3.98e-05
            (ThdN, Shipping, 5e-4, NAN), // 9.32e-05 — aliasing as it ships
        ],
        ("fuzz_face", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -116.0, NAN), // -118 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -37.1 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -62.1 / -44.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -36.7 / -18.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.05, 0.2), // 0.00992 / 0.0356
            (Bla, Shipping, 1.0, 2.0), // 0.188 / 0.613 (R11: the whole card)
            (Compression, Oversampled, 2e-6, 5e-4), // 6.47e-07 / 6.88e-05 — R12: closed with the operating point (was 0.12 dB)
            (Compression, Shipping, 1e-4, 0.005), // 1.87e-05 / 0.00118
            (Detection, Oversampled, 0.001, NAN), // 0.000299
            (Detection, Shipping, 0.02, NAN), // 0.00508
            (GroupDelay, Oversampled, 2e-4, NAN), // 5.68e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 4.64e-08
            (Harmonics, Oversampled, 0.02, 0.001), // 0.00507 / 0.000226 (R11: the whole card)
            (Harmonics, Shipping, 0.1, 0.02), // 0.0283 / 0.0054 (R11: the whole card)
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.23e-07 — R12: closed (was 9.4 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.23e-07
            (Response, Oversampled, 1e-5, 2e-5), // 2.92e-06 / 5.48e-06 — R12: closed with the operating point (was 0.12 dB)
            (Response, Shipping, 1.0, 2.0), // 0.189 / 0.614 (R11: the whole card)
            (ThdN, Oversampled, 5e-5, NAN), // 9.68e-06
            (ThdN, Shipping, 0.001, NAN), // 0.000267 — aliasing as it ships
        ],
        ("fuzz_face_static", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -94.3 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -82.2 / -64.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -82.2 / -64.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.000966 / 0.00669
            (Bla, Shipping, 0.005, 0.05), // 0.000966 / 0.00669
            (Compression, Oversampled, 5e-6, 0.002), // 1.24e-06 / 0.00039 — R12: closed with the operating point (was 0.12 dB)
            (Compression, Shipping, 5e-6, 0.002), // 1.24e-06 / 0.00039
            (Detection, Oversampled, 5e-4, NAN), // 7.58e-05
            (Detection, Shipping, 5e-4, NAN), // 7.58e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.7e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 1.14e-09
            (Harmonics, Oversampled, 5e-4, 0.001), // 7.25e-05 / 0.000263
            (Harmonics, Shipping, 5e-4, 0.001), // 7.25e-05 / 0.000263
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.23e-07 — R12: closed (was 9.4 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.23e-07
            (Response, Oversampled, 1e-5, 2e-5), // 2.92e-06 / 5.49e-06 — R12: closed with the operating point (was 0.12 dB)
            (Response, Shipping, 1e-5, 0.05), // 2.14e-06 / 0.00672
            (ThdN, Oversampled, 5e-5, NAN), // 6.96e-06
            (ThdN, Shipping, 5e-5, NAN), // 6.96e-06 — aliasing as it ships
        ],
        ("common_cathode/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -110.0, NAN), // -112
            (Aliasing, Shipping, -102.0, NAN), // -104 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -25.5 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -63.5 / -51.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -50.9 / -38.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000969 / 0.000373
            (Bla, Shipping, 0.005, 0.02), // 0.00097 / 0.00649
            (Compression, Oversampled, 1e-5, 1e-4), // 3.03e-06 / 2.14e-05
            (Compression, Shipping, 2e-5, 0.002), // 3.76e-06 / 0.000348
            (Detection, Oversampled, 0.02, NAN), // 0.00421
            (Detection, Shipping, 0.05, NAN), // 0.0119
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.01e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 3.13e-08
            (Harmonics, Oversampled, 1.0, 0.02), // 0.239 / 0.00588
            (Harmonics, Shipping, 5.0, 0.1), // 1.13 / 0.0245
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.21e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.21e-09
            (Response, Oversampled, 0.01, 0.01), // 0.00225 / 0.00309
            (Response, Shipping, 0.01, 0.02), // 0.00225 / 0.0065
            (ThdN, Oversampled, 2e-4, NAN), // 3.98e-05
            (ThdN, Shipping, 5e-4, NAN), // 9.32e-05 — aliasing as it ships
        ],
        ("common_cathode_grid_current/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -108.0, NAN), // -111
            (Aliasing, Shipping, -112.0, NAN), // -114 — aliasing as it ships
            (Audibility, Oversampled, -10.0, -10.0), // -67.1 / -53.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -73.4 / -60.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000411
            (Bla, Shipping, 0.005, 5e-4), // 0.000968 / 0.000145
            (Compression, Oversampled, 2e-5, 1e-4), // 3.49e-06 / 2.36e-05
            (Compression, Shipping, 2e-5, 2e-5), // 3.44e-06 / 5.66e-06
            (Detection, Oversampled, 0.005, NAN), // 0.00125
            (Detection, Shipping, 0.002, NAN), // 0.000557
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.62e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 2.61e-08
            (Harmonics, Oversampled, 0.1, 0.2), // 0.0279 / 0.0536
            (Harmonics, Shipping, 0.05, 0.1), // 0.0143 / 0.0249
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 0.01, 0.01), // 0.00196 / 0.0027
            (Response, Shipping, 0.01, 0.01), // 0.00196 / 0.0027
            (ThdN, Oversampled, 2e-4, NAN), // 5.3e-05
            (ThdN, Shipping, 2e-4, NAN), // 4.46e-05 — aliasing as it ships
        ],
        ("common_cathode_grid_current", Tier::T0) => &[
            (Aliasing, Oversampled, -108.0, NAN), // -111
            (Aliasing, Shipping, -112.0, NAN), // -114 — aliasing as it ships
            (Audibility, Oversampled, -10.0, -10.0), // -67.1 / -53.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -73.4 / -60.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000411
            (Bla, Shipping, 0.005, 5e-4), // 0.000968 / 0.000145
            (Compression, Oversampled, 2e-5, 1e-4), // 3.49e-06 / 2.36e-05
            (Compression, Shipping, 2e-5, 2e-5), // 3.44e-06 / 5.66e-06
            (Detection, Oversampled, 0.005, NAN), // 0.00125
            (Detection, Shipping, 0.002, NAN), // 0.000557
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.28e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 1.76e-11
            (Harmonics, Oversampled, 0.1, 0.2), // 0.0279 / 0.0536
            (Harmonics, Shipping, 0.05, 0.1), // 0.0143 / 0.0249
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 2e-9, 5e-9), // 6.39e-10 / 1.41e-09
            (Response, Shipping, 5e-8, 5e-4), // 1.46e-08 / 0.000103
            (ThdN, Oversampled, 2e-4, NAN), // 5.3e-05
            (ThdN, Shipping, 2e-4, NAN), // 4.46e-05 — aliasing as it ships
        ],
        ("diode_clipper/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -93.3 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -39.1 / -21.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -52.8 / -34.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.49
            (Bla, Shipping, 0.2, 0.5), // 0.0355 / 0.107 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.01, 0.02), // 0.00257 / 0.00541
            (Compression, Shipping, 0.01, 0.02), // 0.0025 / 0.00585
            (Detection, Oversampled, 0.05, NAN), // 0.0161
            (Detection, Shipping, 0.05, NAN), // 0.00689
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.69e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 1.7e-08
            (Harmonics, Oversampled, 0.5, 2.0), // 0.0757 / 0.376
            (Harmonics, Shipping, 0.05, 0.5), // 0.00959 / 0.0853
            (OperatingPoint, Oversampled, 1e-9, NAN), // 1.06e-25
            (OperatingPoint, Shipping, 1e-9, NAN), // 1.06e-25
            (Response, Oversampled, 1.0, 2.0), // 0.169 / 0.489
            (Response, Shipping, 0.2, 0.5), // 0.0372 / 0.109 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.02, NAN), // 0.00402
            (ThdN, Shipping, 0.02, NAN), // 0.00502 — aliasing as it ships
        ],
        ("power_supply/hand", Tier::T0) => &[
            (Dynamics, Oversampled, 5e-4, 0.02), // 0.000113 / 0.00488
            (Dynamics, Shipping, 0.01, 0.1), // 0.00236 / 0.0319
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0507 / 1.28
            (OperatingPoint, Oversampled, 1e-4, NAN), // 2.45e-05
            (OperatingPoint, Shipping, 0.005, NAN), // 0.000679
        ],
        ("jfet_booster", Tier::T0) => &[
            (Aliasing, Oversampled, -99.0, NAN), // -101
            (Aliasing, Shipping, -103.0, NAN), // -105 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -10.7 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -48.1 / -35.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -54.8 / -42.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.00113 / 0.00491
            (Bla, Shipping, 0.005, 0.005), // 0.000978 / 0.00133
            (Compression, Oversampled, 2e-5, 2e-4), // 3.39e-06 / 3.82e-05
            (Compression, Shipping, 2e-5, 5e-5), // 3.34e-06 / 9.15e-06
            (Detection, Oversampled, 0.05, NAN), // 0.00789
            (Detection, Shipping, 0.005, NAN), // 0.000811
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.42e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 3.29e-11
            (Harmonics, Oversampled, 1.0, 0.1), // 0.245 / 0.0227
            (Harmonics, Shipping, 0.5, 0.05), // 0.115 / 0.0106
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.65e-10 — R12: closed (was 3.2e-5: SPICE's junction gmin, 4.3 pA in 1 MΩ)
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.65e-10
            (Response, Oversampled, 2e-8, 2e-8), // 3.44e-09 / 5.03e-09
            (Response, Shipping, 1e-7, 0.002), // 1.89e-08 / 0.000448
            (ThdN, Oversampled, 2e-4, NAN), // 8.4e-05 — RAISED from 5e-6 (R12): DK now carries the gate junction's gmin (its bias to 3.7e-10 V, was 3.2e-5), which moves THD+N at −20 dB by 1e-5 of itself; 5e-6 dB sat below the reference's own resolution (ngspice's deck tolerances move this figure 1.5e-4 dB)
            (ThdN, Shipping, 2e-4, NAN), // 3.4e-05 — aliasing as it ships
        ],
        ("jfet_booster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -99.0, NAN), // -101
            (Aliasing, Shipping, -103.0, NAN), // -105 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -10.7 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -48.1 / -35.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -54.8 / -42.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.00113 / 0.00492
            (Bla, Shipping, 0.005, 0.005), // 0.000977 / 0.00104
            (Compression, Oversampled, 2e-5, 2e-4), // 3.34e-06 / 3.79e-05
            (Compression, Shipping, 1e-5, 5e-5), // 3.3e-06 / 9.09e-06
            (Detection, Oversampled, 0.05, NAN), // 0.00797
            (Detection, Shipping, 0.005, NAN), // 0.000792
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.27e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 2.85e-11
            (Harmonics, Oversampled, 1.0, 0.1), // 0.245 / 0.0227
            (Harmonics, Shipping, 0.5, 0.05), // 0.115 / 0.0106
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.65e-10 — R12: closed (was 3.2e-5: SPICE's junction gmin, 4.3 pA in 1 MΩ)
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.65e-10
            (Response, Oversampled, 1e-8, 2e-8), // 2.92e-09 / 5.01e-09
            (Response, Shipping, 1e-7, 0.001), // 1.83e-08 / 0.000167
            (ThdN, Oversampled, 2e-4, NAN), // 8.4e-05 — RAISED from 5e-6 (R12): DK now carries the gate junction's gmin (its bias to 3.7e-10 V, was 3.2e-5), which moves THD+N at −20 dB by 1e-5 of itself; 5e-6 dB sat below the reference's own resolution (ngspice's deck tolerances move this figure 1.5e-4 dB)
            (ThdN, Shipping, 2e-4, NAN), // 3.4e-05 — aliasing as it ships
        ],
        ("opamp_clipper/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -72.7 / -54.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -67.8 / -52.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.001, 0.1), // 0.000316 / 0.021
            (Bla, Shipping, 0.02, 0.5), // 0.00361 / 0.0887 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 1e-4, 5e-4), // 1.81e-05 / 0.000119
            (Compression, Shipping, 2e-4, 0.002), // 6.54e-05 / 0.0005
            (Detection, Oversampled, 5e-4, NAN), // 0.000106
            (Detection, Shipping, 0.005, NAN), // 0.000685
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.68e-09
            (GroupDelay, Shipping, 2e-4, NAN), // 3.91e-09
            (Harmonics, Oversampled, 0.5, 0.05), // 0.0717 / 0.0106
            (Harmonics, Shipping, 0.5, 0.2), // 0.0706 / 0.0445
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 0.005, 0.1), // 0.000827 / 0.0224
            (Response, Shipping, 0.01, 0.5), // 0.00327 / 0.0901 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.002, NAN), // 0.000608
            (ThdN, Shipping, 0.005, NAN), // 0.000764 — aliasing as it ships
        ],
        ("mosfet_booster", Tier::T0) => &[
            (Aliasing, Oversampled, -111.0, NAN), // -114
            (Aliasing, Shipping, -82.0, NAN), // -84.6 — aliasing as it ships
            (Anmr, Shipping, 11.0, NAN), // 7.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -79.8 / -64.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -69.1 / -56.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.01), // 0.000964 / 0.00231
            (Bla, Shipping, 0.005, 0.05), // 0.00164 / 0.00765
            (Compression, Oversampled, 2e-5, 2e-4), // 3.39e-06 / 4.27e-05
            (Compression, Shipping, 2e-5, 5e-4), // 3.38e-06 / 0.000141 (R11: the whole card)
            (Detection, Oversampled, 1.0, NAN), // 0.223
            (Detection, Shipping, 1.0, NAN), // 0.224
            (GroupDelay, Oversampled, 2e-4, NAN), // 5.25e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 7.88e-11
            (Harmonics, Oversampled, 0.2, 0.05), // 0.0404 / 0.0162
            (Harmonics, Shipping, 0.2, 0.2), // 0.0403 / 0.0538 (R11: the whole card)
            (OperatingPoint, Oversampled, 5e-7, NAN), // 8.84e-08
            (OperatingPoint, Shipping, 5e-7, NAN), // 8.84e-08
            (Response, Oversampled, 5e-7, 2e-8), // 7.44e-08 / 5.59e-09
            (Response, Shipping, 5e-5, 0.02), // 1.66e-05 / 0.00368 (R11: the whole card)
            (ThdN, Oversampled, 0.01, NAN), // 0.0017
            (ThdN, Shipping, 0.01, NAN), // 0.00175 — aliasing as it ships
        ],
        ("mosfet_booster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -101.0, NAN), // -104
            (Aliasing, Shipping, -77.0, NAN), // -79.3 — aliasing as it ships
            (Anmr, Shipping, 11.0, NAN), // 7.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -55.0 / -40.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -68.8 / -55.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.05, 0.2), // 0.00683 / 0.0433
            (Bla, Shipping, 0.01, 0.05), // 0.00167 / 0.0106
            (Compression, Oversampled, 2e-5, 0.001), // 3.34e-06 / 0.000228
            (Compression, Shipping, 1e-5, 5e-4), // 3.32e-06 / 0.000105
            (Detection, Oversampled, 1.0, NAN), // 0.224
            (Detection, Shipping, 1.0, NAN), // 0.224
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.67e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 2.26e-11
            (Harmonics, Oversampled, 0.2, 0.5), // 0.0406 / 0.148
            (Harmonics, Shipping, 0.2, 0.2), // 0.0406 / 0.0643
            (OperatingPoint, Oversampled, 5e-7, NAN), // 8.84e-08
            (OperatingPoint, Shipping, 5e-7, NAN), // 8.84e-08
            (Response, Oversampled, 5e-7, 5e-8), // 7.45e-08 / 7.34e-09
            (Response, Shipping, 5e-7, 5e-4), // 7.97e-08 / 0.000132
            (ThdN, Oversampled, 0.01, NAN), // 0.00197
            (ThdN, Shipping, 0.01, NAN), // 0.00186 — aliasing as it ships
        ],
        ("ota_vca", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -81.3 / -64.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -81.2 / -64.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.00105
            (Bla, Shipping, 0.005, 0.005), // 0.000968 / 0.00143
            (Compression, Oversampled, 2e-5, 5e-7), // 5.2e-06 / 1.36e-07
            (Compression, Shipping, 2e-5, 5e-7), // 5.2e-06 / 1.36e-07
            (Detection, Oversampled, 0.01, NAN), // 0.00257
            (Detection, Shipping, 0.01, NAN), // 0.00257
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.01, 5e-4), // 0.00206 / 8.53e-05
            (Harmonics, Shipping, 0.01, 5e-4), // 0.00206 / 8.53e-05
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.9e-10
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.9e-10
            (Response, Oversampled, 2e-9, NAN), // 5.86e-10
            (Response, Shipping, 2e-9, NAN), // 5.86e-10
            (ThdN, Oversampled, 5e-4, NAN), // 8.6e-05
            (ThdN, Shipping, 5e-4, NAN), // 8.6e-05 — aliasing as it ships
        ],
        ("opamp_clipper", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -72.7 / -54.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -67.8 / -52.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.001, 0.1), // 0.000316 / 0.021
            (Bla, Shipping, 0.02, 0.5), // 0.00361 / 0.0887 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 1e-4, 5e-4), // 1.81e-05 / 0.000119
            (Compression, Shipping, 2e-4, 0.002), // 6.54e-05 / 0.0005
            (Detection, Oversampled, 5e-4, NAN), // 0.000106
            (Detection, Shipping, 0.005, NAN), // 0.000685
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.41e-13
            (GroupDelay, Shipping, 2e-4, NAN), // 3.78e-09
            (Harmonics, Oversampled, 0.5, 0.05), // 0.0717 / 0.0106
            (Harmonics, Shipping, 0.5, 0.2), // 0.0706 / 0.0445
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 1e-7, 1e-7), // 1.85e-08 / 3.25e-08
            (Response, Shipping, 0.01, 0.5), // 0.00325 / 0.0901 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.002, NAN), // 0.000608
            (ThdN, Shipping, 0.005, NAN), // 0.000764 — aliasing as it ships
        ],
        ("power_supply", Tier::T0) => &[
            (Dynamics, Oversampled, 5e-4, 0.02), // 0.000113 / 0.00489
            (Dynamics, Shipping, 0.01, 0.1), // 0.00236 / 0.0319
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0507 / 1.28
            (OperatingPoint, Oversampled, 1e-4, NAN), // 2.49e-05
            (OperatingPoint, Shipping, 0.005, NAN), // 0.000679
        ],
        ("push_pull", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -41.4 / -27.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -82.1 / -64.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.2, 2.0), // 0.0517 / 0.367
            (Bla, Shipping, 0.005, 0.002), // 0.00102 / 0.000538
            (Compression, Oversampled, 5e-5, 1e-6), // 8.39e-06 / 3.25e-07
            (Compression, Shipping, 5e-5, 1e-6), // 8.39e-06 / 3.25e-07
            (Detection, Oversampled, 0.2, NAN), // 0.0642
            (Detection, Shipping, 0.005, NAN), // 0.00164
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.01, 0.001), // 0.00286 / 0.000208
            (Harmonics, Shipping, 0.01, 0.001), // 0.00286 / 0.000208
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.5e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.5e-09
            (Response, Oversampled, 2e-9, NAN), // 4.68e-10
            (Response, Shipping, 2e-9, NAN), // 4.68e-10
            (ThdN, Oversampled, 5e-4, NAN), // 0.000101
            (ThdN, Shipping, 5e-4, NAN), // 0.000101 — aliasing as it ships
        ],
        ("push_pull_pentode", Tier::T0) => &[
            (Aliasing, Oversampled, -89.0, NAN), // -91.4
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -75.6 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -46.5 / -28.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -81.8 / -64.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.2, 0.5), // 0.0426 / 0.149
            (Bla, Shipping, 0.005, 0.005), // 0.001 / 0.000775
            (Compression, Oversampled, 5e-5, 2e-7), // 9.12e-06 / 4.32e-08
            (Compression, Shipping, 5e-5, 2e-7), // 9.11e-06 / 4.32e-08
            (Detection, Oversampled, 0.2, NAN), // 0.0376
            (Detection, Shipping, 0.005, NAN), // 0.0013
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.05, 2e-4), // 0.00953 / 4.61e-05
            (Harmonics, Shipping, 0.02, 2e-4), // 0.00418 / 4.61e-05
            (OperatingPoint, Oversampled, 1e-7, NAN), // 2.97e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 2.97e-08
            (Response, Oversampled, 2e-9, NAN), // 3.87e-10
            (Response, Shipping, 2e-9, NAN), // 3.87e-10
            (ThdN, Oversampled, 5e-4, NAN), // 9.66e-05
            (ThdN, Shipping, 5e-4, NAN), // 9.69e-05 — aliasing as it ships
        ],
        ("triode_stage", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -57.5 / -37.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -81.9 / -64.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.02, 0.1), // 0.0057 / 0.0244
            (Bla, Shipping, 0.005, 2e-4), // 0.000968 / 6.51e-05
            (Compression, Oversampled, 2e-5, 1e-7), // 3.34e-06 / 2.54e-08
            (Compression, Shipping, 2e-5, 1e-7), // 3.34e-06 / 2.54e-08
            (Detection, Oversampled, 0.1, NAN), // 0.018
            (Detection, Shipping, 0.005, NAN), // 0.000774
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.002, 5e-6), // 0.000635 / 1.46e-06
            (Harmonics, Shipping, 0.002, 5e-6), // 0.000635 / 1.46e-06
            (OperatingPoint, Oversampled, 5e-9, NAN), // 7.6e-10
            (OperatingPoint, Shipping, 5e-9, NAN), // 7.6e-10
            (Response, Oversampled, 2e-9, NAN), // 5.68e-10
            (Response, Shipping, 2e-9, NAN), // 5.68e-10
            (ThdN, Oversampled, 1e-4, NAN), // 2.29e-05
            (ThdN, Shipping, 1e-4, NAN), // 2.29e-05 — aliasing as it ships
        ],
        ("rangemaster", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -52.1 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -68.8 / -56.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -43.8 / -31.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.02, 1.0), // 0.00421 / 0.186
            (Bla, Shipping, 0.2, 20.0), // 0.0479 / 3.35
            (Compression, Oversampled, 0.002, 0.01), // 0.00058 / 0.00255
            (Compression, Shipping, 0.05, 0.2), // 0.00929 / 0.041
            (Detection, Oversampled, 0.01, NAN), // 0.00327
            (Detection, Shipping, 0.2, NAN), // 0.0611
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.47e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 5.33e-07
            (Harmonics, Oversampled, 0.01, 0.2), // 0.00311 / 0.0461
            (Harmonics, Shipping, 0.2, 2.0), // 0.0404 / 0.755
            (OperatingPoint, Oversampled, 5e-7, NAN), // 9.19e-08 — R12: closed (was 3.9 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 5e-7, NAN), // 9.19e-08
            (Response, Oversampled, 1e-5, 2e-5), // 2.84e-06 / 6.54e-06 — R12: closed with the operating point (was 0.13 dB)
            (Response, Shipping, 0.2, 10.0), // 0.0432 / 3.24
            (ThdN, Oversampled, 0.01, NAN), // 0.00194
            (ThdN, Shipping, 0.05, NAN), // 0.031 — aliasing as it ships
        ],
        ("rangemaster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -52.9 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -67.2 / -54.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -43.2 / -31.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.02, 1.0), // 0.00522 / 0.198
            (Bla, Shipping, 0.2, 20.0), // 0.0617 / 3.48
            (Compression, Oversampled, 0.002, 0.01), // 0.000603 / 0.00243
            (Compression, Shipping, 0.05, 0.2), // 0.00965 / 0.0389
            (Detection, Oversampled, 0.02, NAN), // 0.00405
            (Detection, Shipping, 0.5, NAN), // 0.0714
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.62e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 5.8e-07
            (Harmonics, Oversampled, 0.02, 0.2), // 0.00354 / 0.0523
            (Harmonics, Shipping, 0.2, 2.0), // 0.0435 / 0.853
            (OperatingPoint, Oversampled, 5e-7, NAN), // 9.19e-08 — R12: closed (was 3.9 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 5e-7, NAN), // 9.19e-08
            (Response, Oversampled, 1e-5, 5e-5), // 3.17e-06 / 7.64e-06 — R12: closed with the operating point (was 0.13 dB)
            (Response, Shipping, 0.2, 10.0), // 0.0534 / 3.3
            (ThdN, Oversampled, 0.01, NAN), // 0.00212
            (ThdN, Shipping, 0.05, NAN), // 0.0339 — aliasing as it ships
        ],
        ("tube_screamer", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -49.5 / -31.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -37.2 / -19.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.2, 0.5), // 0.0456 / 0.109
            (Bla, Shipping, 1.0, 2.0), // 0.19 / 0.442 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 2e-4, 0.005), // 6.21e-05 / 0.000774
            (Compression, Shipping, 0.001, 0.01), // 0.000241 / 0.0032
            (Detection, Oversampled, 0.01, NAN), // 0.00273
            (Detection, Shipping, 0.05, NAN), // 0.0112
            (GroupDelay, Oversampled, 2e-4, NAN), // 8.79e-13
            (GroupDelay, Shipping, 2e-4, NAN), // 5.48e-08
            (Harmonics, Oversampled, 0.1, 1.0), // 0.0208 / 0.231
            (Harmonics, Shipping, 0.5, 5.0), // 0.108 / 0.945
            (OperatingPoint, Oversampled, 2e-9, NAN), // 5.31e-10
            (OperatingPoint, Shipping, 2e-9, NAN), // 5.31e-10
            (Response, Oversampled, 1e-6, 2e-6), // 1.69e-07 / 5.78e-07
            (Response, Shipping, 1.0, 2.0), // 0.192 / 0.402 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.001, NAN), // 0.000298
            (ThdN, Shipping, 0.005, NAN), // 0.00116 — aliasing as it ships
        ],
        ("diode_clipper", Tier::T1) => &[
            (Aliasing, Oversampled, -59.0, NAN), // -61.7
            (Aliasing, Shipping, -76.0, NAN), // -78.3 — aliasing as it ships
            (Anmr, Shipping, 1.0, NAN), // -2.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -37.5 / -19.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -48.4 / -30.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 1.0, 2.0), // 0.173 / 0.515
            (Bla, Shipping, 0.2, 0.5), // 0.0493 / 0.15 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.1, 0.5), // 0.0287 / 0.194
            (Compression, Shipping, 0.05, 0.2), // 0.00819 / 0.0567
            (Detection, Oversampled, 0.1, NAN), // 0.0234
            (Detection, Shipping, 0.05, NAN), // 0.00686
            (Dynamics, Oversampled, 5e-5, 0.05), // 1.18e-05 / 0.00788
            (Dynamics, Shipping, 5e-5, 0.01), // 6.8e-06 / 0.00246
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.5e-13
            (GroupDelay, Shipping, 2e-4, NAN), // 1.12e-08
            (Harmonics, Oversampled, 100.0, 200.0), // 17.7 / 42.3
            (Harmonics, Shipping, 50.0, 2.0), // 9.02 / 0.453
            (Intermodulation, Oversampled, 20.0, 20.0), // 5.23 / 3.81
            (Intermodulation, Shipping, 2.0, 2.0), // 0.516 / 0.528
            (OperatingPoint, Oversampled, 1e-9, NAN), // 1.06e-25
            (OperatingPoint, Shipping, 1e-9, NAN), // 1.06e-25
            (Response, Oversampled, 5e-9, 2e-8), // 1.12e-09 / 4.08e-09 — FINDING: the deck's 4 pF Cjo, which DK leaves out (R11)
            (Response, Shipping, 0.2, 0.5), // 0.051 / 0.149 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 200.0, NAN), // 49.1
            (ThdN, Shipping, 100.0, NAN), // 32.5 — aliasing as it ships
        ],
        ("ce_stage", Tier::T1) => &[
            (Aliasing, Oversampled, -72.0, NAN), // -74.7
            (Aliasing, Shipping, -57.0, NAN), // -59.8 — aliasing as it ships
            (Anmr, Shipping, 17.0, NAN), // 13.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -78.9 / -63.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -70.3 / -54.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.00101 / 0.00518
            (Bla, Shipping, 0.005, 0.05), // 0.00139 / 0.0129
            (Compression, Oversampled, 0.001, 0.001), // 0.00028 / 0.000209
            (Compression, Shipping, 0.002, 0.005), // 0.000392 / 0.00102 (R11: the whole card)
            (Detection, Oversampled, 50.0, NAN), // 13.8
            (Detection, Shipping, 50.0, NAN), // 13.8
            (Dynamics, Oversampled, 2e-4, 0.02), // 5e-05 / 0.0038
            (Dynamics, Shipping, 2e-4, 0.02), // 5e-05 / 0.0038
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.56e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 2.62e-10
            (Harmonics, Oversampled, 0.5, 0.5), // 0.0857 / 0.149
            (Harmonics, Shipping, 0.2, 0.5), // 0.0495 / 0.135 (R11: the whole card)
            (Intermodulation, Oversampled, 0.1, 2.0), // 0.0265 / 0.345 (R11: the whole card)
            (Intermodulation, Shipping, 0.1, 0.2), // 0.0266 / 0.0469 (R11: the whole card)
            (OperatingPoint, Oversampled, 5e-6, NAN), // 9.02e-07
            (OperatingPoint, Shipping, 5e-6, NAN), // 9.02e-07
            (Response, Oversampled, 5e-6, 1e-5), // 7.76e-07 / 1.77e-06
            (Response, Shipping, 2e-5, 0.02), // 4.22e-06 / 0.00446 (R11: the whole card)
            (ThdN, Oversampled, 0.1, NAN), // 0.0277
            (ThdN, Shipping, 0.1, NAN), // 0.0277 — aliasing as it ships
        ],
        ("ce_stage_static", Tier::T1) => &[
            (Aliasing, Oversampled, -46.0, NAN), // -48.6
            (Aliasing, Shipping, -57.0, NAN), // -59.6 — aliasing as it ships
            (Anmr, Shipping, 17.0, NAN), // 13.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -58.8 / -42.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -70.3 / -54.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.02, 0.05), // 0.00389 / 0.0141
            (Bla, Shipping, 0.005, 0.05), // 0.00149 / 0.00914
            (Compression, Oversampled, 0.001, 0.01), // 0.000244 / 0.0022
            (Compression, Shipping, 0.001, 0.005), // 0.000245 / 0.00068
            (Detection, Oversampled, 50.0, NAN), // 13.8
            (Detection, Shipping, 50.0, NAN), // 13.8
            (Dynamics, Oversampled, 5e-4, 0.02), // 8.78e-05 / 0.00601
            (Dynamics, Shipping, 2e-4, 0.02), // 4.96e-05 / 0.00382
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.58e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 2.11e-10
            (Harmonics, Oversampled, 1.0, 10.0), // 0.17 / 2.04
            (Harmonics, Shipping, 0.2, 0.5), // 0.0495 / 0.13
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.0512 / 0.03
            (Intermodulation, Shipping, 0.1, 0.01), // 0.0266 / 0.00321
            (OperatingPoint, Oversampled, 5e-6, NAN), // 9.02e-07
            (OperatingPoint, Shipping, 5e-6, NAN), // 9.02e-07
            (Response, Oversampled, 5e-6, 1e-5), // 7.76e-07 / 1.77e-06
            (Response, Shipping, 5e-6, 0.005), // 6.9e-07 / 0.00124
            (ThdN, Oversampled, 0.1, NAN), // 0.0277
            (ThdN, Shipping, 0.1, NAN), // 0.0277 — aliasing as it ships
        ],
        ("common_cathode", Tier::T1) => &[
            (Aliasing, Oversampled, -94.0, NAN), // -96.5
            (Aliasing, Shipping, -30.0, NAN), // -32.7 — aliasing as it ships
            (Anmr, Shipping, 16.0, NAN), // 12.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -44.5 / -31.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -29.7 / -17.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000382
            (Bla, Shipping, 0.05, 0.2), // 0.012 / 0.0553
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000192
            (Compression, Shipping, 0.001, 0.02), // 0.000244 / 0.00363
            (Detection, Oversampled, 0.05, NAN), // 0.0125
            (Detection, Shipping, 0.2, NAN), // 0.0617
            (Dynamics, Oversampled, 0.001, 5e-4), // 0.000319 / 9.51e-05
            (Dynamics, Shipping, 0.01, 0.005), // 0.00181 / 0.00154
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.43e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 1.22e-09
            (Harmonics, Oversampled, 2.0, 0.2), // 0.598 / 0.0663
            (Harmonics, Shipping, 10.0, 5.0), // 2.07 / 0.738
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0493 / 0.016
            (Intermodulation, Shipping, 0.5, 0.05), // 0.0828 / 0.0126
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.21e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.21e-09
            (Response, Oversampled, 5e-9, 1e-8), // 7.57e-10 / 2.25e-09
            (Response, Shipping, 5e-6, 0.05), // 9.32e-07 / 0.0078
            (ThdN, Oversampled, 0.01, NAN), // 0.0017
            (ThdN, Shipping, 0.5, NAN), // 0.163 — aliasing as it ships
        ],
        ("common_cathode/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -94.0, NAN), // -96.5
            (Aliasing, Shipping, -30.0, NAN), // -32.7 — aliasing as it ships
            (Anmr, Shipping, 16.0, NAN), // 12.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -44.5 / -31.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -29.7 / -17.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000382
            (Bla, Shipping, 0.05, 0.2), // 0.012 / 0.0553
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000192
            (Compression, Shipping, 0.001, 0.02), // 0.000244 / 0.00363
            (Detection, Oversampled, 0.05, NAN), // 0.0125
            (Detection, Shipping, 0.2, NAN), // 0.0617
            (Dynamics, Oversampled, 0.001, 5e-4), // 0.000319 / 9.51e-05
            (Dynamics, Shipping, 0.01, 0.005), // 0.00181 / 0.00154
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.39e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 1.48e-08
            (Harmonics, Oversampled, 2.0, 0.2), // 0.598 / 0.0663
            (Harmonics, Shipping, 10.0, 5.0), // 2.07 / 0.738
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0493 / 0.016
            (Intermodulation, Shipping, 0.5, 0.05), // 0.0828 / 0.0126
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.21e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.21e-09
            (Response, Oversampled, 0.01, 0.01), // 0.00182 / 0.0033
            (Response, Shipping, 0.01, 0.05), // 0.00183 / 0.00779
            (ThdN, Oversampled, 0.01, NAN), // 0.0017
            (ThdN, Shipping, 0.5, NAN), // 0.163 — aliasing as it ships
        ],
        ("fuzz_face", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -117
            (Aliasing, Shipping, -107.0, NAN), // -110 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -19.9 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -54.6 / -36.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -28.8 / -11.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.05, 0.2), // 0.00992 / 0.0357
            (Bla, Shipping, 1.0, 5.0), // 0.223 / 0.731 (R11: the whole card)
            (Compression, Oversampled, 0.01, 0.05), // 0.0018 / 0.0109 — R12: closed with the operating point (was 0.12 dB)
            (Compression, Shipping, 0.2, 1.0), // 0.0412 / 0.221 (R11: the whole card)
            (Detection, Oversampled, 0.005, NAN), // 0.00113
            (Detection, Shipping, 0.05, NAN), // 0.00825
            (Dynamics, Oversampled, 5e-5, 0.1), // 1.52e-05 / 0.0345
            (Dynamics, Shipping, 2e-4, 0.5), // 3.59e-05 / 0.0858 (R11: the whole card)
            (GroupDelay, Oversampled, 2e-4, NAN), // 5.68e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 5.5e-08
            (Harmonics, Oversampled, 0.1, 0.05), // 0.0311 / 0.0113
            (Harmonics, Shipping, 1.0, 1.0), // 0.21 / 0.175 (R11: the whole card)
            (Intermodulation, Oversampled, 0.5, 0.5), // 0.0717 / 0.0965
            (Intermodulation, Shipping, 5.0, 2.0), // 0.985 / 0.424 (R11: the whole card)
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.23e-07 — R12: closed (was 9.4 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.23e-07
            (Response, Oversampled, 1e-5, 2e-5), // 2.92e-06 / 5.48e-06 — R12: closed with the operating point (was 0.12 dB)
            (Response, Shipping, 1.0, 5.0), // 0.225 / 0.73 (R11: the whole card)
            (ThdN, Oversampled, 0.02, NAN), // 0.00644
            (ThdN, Shipping, 0.5, NAN), // 0.145 (R11: the whole card)
        ],
        ("fuzz_face_static", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -78.9 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -82.2 / -64.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -82.2 / -64.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.000966 / 0.00669
            (Bla, Shipping, 0.005, 0.05), // 0.000967 / 0.00794
            (Compression, Oversampled, 0.001, 0.02), // 0.000242 / 0.00335 — R12: closed with the operating point (was 0.12 dB)
            (Compression, Shipping, 0.001, 0.02), // 0.000242 / 0.00397
            (Detection, Oversampled, 5e-4, NAN), // 0.000158
            (Detection, Shipping, 5e-4, NAN), // 0.000158
            (Dynamics, Oversampled, 2e-4, 0.005), // 3.4e-05 / 0.00128
            (Dynamics, Shipping, 2e-4, 0.005), // 4e-05 / 0.00129
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.7e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 1.35e-09
            (Harmonics, Oversampled, 0.05, 0.02), // 0.0121 / 0.00432
            (Harmonics, Shipping, 0.02, 0.01), // 0.0061 / 0.00243
            (Intermodulation, Oversampled, 0.1, 0.1), // 0.0231 / 0.0211
            (Intermodulation, Shipping, 0.02, 0.05), // 0.0063 / 0.00808
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.23e-07 — R12: closed (was 9.4 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.23e-07
            (Response, Oversampled, 1e-5, 2e-5), // 2.92e-06 / 5.49e-06 — R12: closed with the operating point (was 0.12 dB)
            (Response, Shipping, 1e-5, 0.05), // 2.73e-06 / 0.00797
            (ThdN, Oversampled, 0.005, NAN), // 0.000728
            (ThdN, Shipping, 0.005, NAN), // 0.000728 — aliasing as it ships
        ],
        ("diode_clipper/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -60.0, NAN), // -62.1
            (Aliasing, Shipping, -76.0, NAN), // -78.5 — aliasing as it ships
            (Anmr, Shipping, 0.0, NAN), // -3 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -38.1 / -19.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -50.0 / -31.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.493
            (Bla, Shipping, 0.2, 0.5), // 0.0446 / 0.131 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.1, 0.5), // 0.027 / 0.159
            (Compression, Shipping, 0.05, 0.1), // 0.013 / 0.0266
            (Detection, Oversampled, 0.1, NAN), // 0.0303
            (Detection, Shipping, 0.05, NAN), // 0.0156
            (Dynamics, Oversampled, 5e-5, 0.1), // 1.15e-05 / 0.026
            (Dynamics, Shipping, 5e-5, 0.1), // 7.12e-06 / 0.0193
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.69e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 1.72e-08
            (Harmonics, Oversampled, 100.0, 200.0), // 17.6 / 45.5
            (Harmonics, Shipping, 50.0, 2.0), // 9.02 / 0.664
            (Intermodulation, Oversampled, 20.0, 20.0), // 5.19 / 5.1
            (Intermodulation, Shipping, 2.0, 5.0), // 0.475 / 0.692
            (OperatingPoint, Oversampled, 1e-9, NAN), // 1.06e-25
            (OperatingPoint, Shipping, 1e-9, NAN), // 1.06e-25
            (Response, Oversampled, 1.0, 2.0), // 0.169 / 0.489
            (Response, Shipping, 0.2, 0.5), // 0.0452 / 0.132 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 200.0, NAN), // 48.7
            (ThdN, Shipping, 100.0, NAN), // 32.3 — aliasing as it ships
        ],
        ("power_supply/hand", Tier::T1) => &[
            (Dynamics, Oversampled, 0.001, 0.02), // 0.000177 / 0.00521
            (Dynamics, Shipping, 0.01, 0.2), // 0.00272 / 0.0374
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0571 / 1.57
            (OperatingPoint, Oversampled, 5e-4, NAN), // 0.00014
            (OperatingPoint, Shipping, 0.005, NAN), // 0.00114
        ],
        ("common_cathode_grid_current", Tier::T1) => &[
            (Aliasing, Oversampled, -58.0, NAN), // -60.3
            (Aliasing, Shipping, -79.0, NAN), // -81.4 — aliasing as it ships
            (Anmr, Shipping, -1.0, NAN), // -4.1 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -59.9 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -59.9 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.000968 / 0.00432
            (Bla, Shipping, 0.005, 0.002), // 0.000968 / 0.000434
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000211
            (Compression, Shipping, 0.001, 2e-4), // 0.000243 / 6.59e-05
            (Detection, Oversampled, 0.05, NAN), // 0.00981
            (Detection, Shipping, 0.01, NAN), // 0.00196
            (Dynamics, Oversampled, 0.001, 1e-4), // 0.000246 / 2.44e-05
            (Dynamics, Shipping, 0.001, 1e-4), // 0.000193 / 2.64e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.28e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 2.08e-11
            (Harmonics, Oversampled, 1.0, 0.5), // 0.235 / 0.122
            (Harmonics, Shipping, 0.5, 0.1), // 0.107 / 0.0274
            (Intermodulation, Oversampled, 0.2, 0.2), // 0.0617 / 0.0344
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0283 / 0.00356
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 2e-9, 5e-9), // 6.39e-10 / 1.41e-09
            (Response, Shipping, 1e-7, 5e-4), // 1.72e-08 / 0.000122
            (ThdN, Oversampled, 0.02, NAN), // 0.00386
            (ThdN, Shipping, 0.02, NAN), // 0.00655 — aliasing as it ships
        ],
        ("common_cathode_grid_current/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -58.0, NAN), // -60.3
            (Aliasing, Shipping, -79.0, NAN), // -81.4 — aliasing as it ships
            (Anmr, Shipping, -1.0, NAN), // -4.1 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -59.9 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -59.9 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.000968 / 0.00432
            (Bla, Shipping, 0.005, 0.002), // 0.000968 / 0.000434
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000211
            (Compression, Shipping, 0.001, 2e-4), // 0.000243 / 6.59e-05
            (Detection, Oversampled, 0.05, NAN), // 0.00981
            (Detection, Shipping, 0.01, NAN), // 0.00196
            (Dynamics, Oversampled, 0.001, 1e-4), // 0.000246 / 2.44e-05
            (Dynamics, Shipping, 0.001, 1e-4), // 0.000193 / 2.64e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.22e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 1.21e-08
            (Harmonics, Oversampled, 1.0, 0.5), // 0.235 / 0.122
            (Harmonics, Shipping, 0.5, 0.1), // 0.107 / 0.0274
            (Intermodulation, Oversampled, 0.2, 0.2), // 0.0617 / 0.0344
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0283 / 0.00356
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 0.005, 0.01), // 0.00159 / 0.0028
            (Response, Shipping, 0.005, 0.01), // 0.00159 / 0.00281
            (ThdN, Oversampled, 0.02, NAN), // 0.00386
            (ThdN, Shipping, 0.02, NAN), // 0.00655 — aliasing as it ships
        ],
        ("jfet_booster", Tier::T1) => &[
            (Aliasing, Oversampled, -52.0, NAN), // -54.8
            (Aliasing, Shipping, -64.0, NAN), // -66.4 — aliasing as it ships
            (Anmr, Shipping, 14.0, NAN), // 10.4 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -48.1 / -35.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -53.9 / -41.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.0013 / 0.00973
            (Bla, Shipping, 0.005, 0.005), // 0.00098 / 0.00164
            (Compression, Oversampled, 0.001, 0.002), // 0.000243 / 0.000545
            (Compression, Shipping, 0.001, 0.001), // 0.00025 / 0.000303 (R11: the whole card)
            (Detection, Oversampled, 0.05, NAN), // 0.0112
            (Detection, Shipping, 0.01, NAN), // 0.00314
            (Dynamics, Oversampled, 2e-5, 0.002), // 1.32e-05 / 0.000393
            (Dynamics, Shipping, 2e-5, 5e-4), // 5.27e-06 / 0.000121
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.42e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 3.9e-11
            (Harmonics, Oversampled, 2.0, 20.0), // 0.572 / 4.3 (R11: the whole card)
            (Harmonics, Shipping, 1.0, 0.2), // 0.292 / 0.0462
            (Intermodulation, Oversampled, 5.0, 0.2), // 1.13 / 0.192
            (Intermodulation, Shipping, 1.0, 0.1), // 0.21 / 0.0247
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.65e-10 — R12: closed (was 3.2e-5: SPICE's junction gmin, 4.3 pA in 1 MΩ)
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.65e-10
            (Response, Oversampled, 2e-8, 2e-8), // 3.44e-09 / 5.03e-09
            (Response, Shipping, 1e-7, 0.002), // 2.17e-08 / 0.000531 (R11: the whole card)
            (ThdN, Oversampled, 0.01, NAN), // 0.00378
            (ThdN, Shipping, 0.005, NAN), // 0.000891 — aliasing as it ships
        ],
        ("jfet_booster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -52.0, NAN), // -54.8
            (Aliasing, Shipping, -64.0, NAN), // -66.3 — aliasing as it ships
            (Anmr, Shipping, 14.0, NAN), // 10.4 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -48.1 / -35.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -53.9 / -41.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.0013 / 0.0101
            (Bla, Shipping, 0.005, 0.01), // 0.000987 / 0.00176
            (Compression, Oversampled, 0.001, 0.002), // 0.000243 / 0.000358
            (Compression, Shipping, 0.001, 5e-4), // 0.000243 / 0.000123
            (Detection, Oversampled, 0.05, NAN), // 0.0114
            (Detection, Shipping, 0.01, NAN), // 0.00309
            (Dynamics, Oversampled, 2e-5, 0.002), // 1.32e-05 / 0.000389
            (Dynamics, Shipping, 2e-5, 5e-4), // 5.26e-06 / 0.000125
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.27e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 3.37e-11
            (Harmonics, Oversampled, 2.0, 1.0), // 0.575 / 0.295
            (Harmonics, Shipping, 1.0, 0.2), // 0.294 / 0.0511
            (Intermodulation, Oversampled, 5.0, 0.2), // 0.775 / 0.0482
            (Intermodulation, Shipping, 1.0, 0.1), // 0.211 / 0.0247
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.65e-10 — R12: closed (was 3.2e-5: SPICE's junction gmin, 4.3 pA in 1 MΩ)
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.65e-10
            (Response, Oversampled, 1e-8, 2e-8), // 2.92e-09 / 5.01e-09
            (Response, Shipping, 1e-7, 0.001), // 2.12e-08 / 0.000198
            (ThdN, Oversampled, 0.01, NAN), // 0.00367
            (ThdN, Shipping, 0.005, NAN), // 0.000824 — aliasing as it ships
        ],
        ("opamp_clipper/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -108.0, NAN), // -110
            (Aliasing, Shipping, -68.0, NAN), // -70.4 — aliasing as it ships
            (Anmr, Shipping, 8.0, NAN), // 5 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -67.4 / -48.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -59.4 / -39.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.1), // 0.000793 / 0.0233
            (Bla, Shipping, 0.05, 0.5), // 0.00839 / 0.106 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.001, 0.05), // 0.000215 / 0.0137
            (Compression, Shipping, 0.01, 0.2), // 0.00188 / 0.0565
            (Detection, Oversampled, 0.001, NAN), // 0.000269
            (Detection, Shipping, 0.005, NAN), // 0.00128
            (Dynamics, Oversampled, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (Dynamics, Shipping, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.5e-09
            (GroupDelay, Shipping, 2e-4, NAN), // 4.43e-09
            (Harmonics, Oversampled, 2.0, 20.0), // 0.511 / 3.62
            (Harmonics, Shipping, 1.0, 1.0), // 0.269 / 0.288
            (Intermodulation, Oversampled, 0.5, 5.0), // 0.074 / 0.669
            (Intermodulation, Shipping, 0.2, 2.0), // 0.035 / 0.431
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 0.005, 0.1), // 0.000825 / 0.0224
            (Response, Shipping, 0.02, 0.5), // 0.00388 / 0.107 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 50.0, NAN), // 9.53
            (ThdN, Shipping, 200.0, NAN), // 49.6 — aliasing as it ships
        ],
        ("mosfet_booster", Tier::T1) => &[
            (Aliasing, Oversampled, -63.0, NAN), // -65.8
            (Aliasing, Shipping, -49.0, NAN), // -51.4 — aliasing as it ships
            (Anmr, Shipping, 26.0, NAN), // 22.5 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -50.5 / -34.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -31.8 / -12.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.01), // 0.000979 / 0.00328
            (Bla, Shipping, 0.01, 0.1), // 0.00297 / 0.0179
            (Compression, Oversampled, 0.002, 0.05), // 0.000406 / 0.00731 (R11: the whole card)
            (Compression, Shipping, 0.005, 0.1), // 0.000894 / 0.0197 (R11: the whole card)
            (Detection, Oversampled, 50.0, NAN), // 10
            (Detection, Shipping, 50.0, NAN), // 10
            (Dynamics, Oversampled, 5e-5, 0.005), // 1.1e-05 / 0.00132
            (Dynamics, Shipping, 5e-5, 0.01), // 1.1e-05 / 0.00252
            (GroupDelay, Oversampled, 2e-4, NAN), // 5.25e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 9.33e-11
            (Harmonics, Oversampled, 1.0, 2.0), // 0.309 / 1.13
            (Harmonics, Shipping, 1.0, 2.0), // 0.275 / 0.413 (R11: the whole card)
            (Intermodulation, Oversampled, 1.0, 1.0), // 0.266 / 0.563
            (Intermodulation, Shipping, 2.0, 2.0), // 0.395 / 0.554 (R11: the whole card)
            (OperatingPoint, Oversampled, 5e-7, NAN), // 8.84e-08
            (OperatingPoint, Shipping, 5e-7, NAN), // 8.84e-08
            (Response, Oversampled, 5e-7, 2e-8), // 7.44e-08 / 5.59e-09
            (Response, Shipping, 1e-4, 0.02), // 1.97e-05 / 0.00436 (R11: the whole card)
            (ThdN, Oversampled, 5.0, NAN), // 1.13
            (ThdN, Shipping, 5.0, NAN), // 1.13 — aliasing as it ships
        ],
        ("mosfet_booster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -39.0, NAN), // -41.7
            (Aliasing, Shipping, -49.0, NAN), // -51 — aliasing as it ships
            (Anmr, Shipping, 26.0, NAN), // 22.6 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -47.6 / -30.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -47.8 / -31.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.05, 0.1), // 0.00834 / 0.0303
            (Bla, Shipping, 0.01, 0.05), // 0.00274 / 0.0161
            (Compression, Oversampled, 0.001, 0.002), // 0.000243 / 0.000652
            (Compression, Shipping, 0.001, 0.002), // 0.000243 / 0.000355
            (Detection, Oversampled, 50.0, NAN), // 9.93
            (Detection, Shipping, 50.0, NAN), // 9.93
            (Dynamics, Oversampled, 2e-4, 0.02), // 4.33e-05 / 0.00452
            (Dynamics, Shipping, 5e-5, 0.01), // 1.09e-05 / 0.00216
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.67e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 2.68e-11
            (Harmonics, Oversampled, 1.0, 2.0), // 0.275 / 0.638
            (Harmonics, Shipping, 1.0, 1.0), // 0.275 / 0.234
            (Intermodulation, Oversampled, 2.0, 1.0), // 0.639 / 0.245
            (Intermodulation, Shipping, 2.0, 0.5), // 0.624 / 0.131
            (OperatingPoint, Oversampled, 5e-7, NAN), // 8.84e-08
            (OperatingPoint, Shipping, 5e-7, NAN), // 8.84e-08
            (Response, Oversampled, 5e-7, 5e-8), // 7.45e-08 / 7.32e-09
            (Response, Shipping, 5e-7, 5e-4), // 8.06e-08 / 0.000157
            (ThdN, Oversampled, 5.0, NAN), // 1.14
            (ThdN, Shipping, 5.0, NAN), // 1.14 — aliasing as it ships
        ],
        ("power_supply", Tier::T1) => &[
            (Dynamics, Oversampled, 0.001, 0.02), // 0.000176 / 0.00521
            (Dynamics, Shipping, 0.01, 0.2), // 0.00272 / 0.0374
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0571 / 1.57
            (OperatingPoint, Oversampled, 5e-4, NAN), // 0.00014
            (OperatingPoint, Shipping, 0.005, NAN), // 0.00114
        ],
        ("ota_vca", Tier::T1) => &[
            (Aliasing, Oversampled, -86.0, NAN), // -88
            (Aliasing, Shipping, -32.0, NAN), // -34.7 — aliasing as it ships
            (Anmr, Shipping, 36.0, NAN), // 32.7 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -60.6 / -43.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -45.8 / -31.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.00123
            (Bla, Shipping, 0.05, 0.2), // 0.00817 / 0.0651
            (Compression, Oversampled, 0.001, 0.002), // 0.000332 / 0.000346
            (Compression, Shipping, 0.001, 0.002), // 0.000332 / 0.000346
            (Detection, Oversampled, 0.01, NAN), // 0.00257
            (Detection, Shipping, 0.2, NAN), // 0.048
            (Dynamics, Oversampled, 5e-6, 2e-4), // 1.47e-06 / 4.73e-05
            (Dynamics, Shipping, 5e-6, 2e-4), // 1.47e-06 / 4.73e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.2), // 0.162 / 0.0608
            (Harmonics, Shipping, 0.1, 0.05), // 0.0293 / 0.0133
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.065 / 0.0254
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0265 / 0.00433
            (OperatingPoint, Oversampled, 2e-9, NAN), // 3.9e-10
            (OperatingPoint, Shipping, 2e-9, NAN), // 3.9e-10
            (Response, Oversampled, 2e-9, NAN), // 5.86e-10
            (Response, Shipping, 2e-9, NAN), // 5.86e-10
            (ThdN, Oversampled, 0.01, NAN), // 0.0024
            (ThdN, Shipping, 100.0, NAN), // 19.2 — aliasing as it ships
        ],
        ("opamp_clipper", Tier::T1) => &[
            (Aliasing, Oversampled, -108.0, NAN), // -110
            (Aliasing, Shipping, -68.0, NAN), // -70.4 — aliasing as it ships
            (Anmr, Shipping, 8.0, NAN), // 5 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -67.4 / -48.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -59.4 / -39.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.1), // 0.000793 / 0.0233
            (Bla, Shipping, 0.05, 0.5), // 0.00839 / 0.106 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.001, 0.05), // 0.000215 / 0.0137
            (Compression, Shipping, 0.01, 0.2), // 0.00188 / 0.0565
            (Detection, Oversampled, 0.001, NAN), // 0.000269
            (Detection, Shipping, 0.005, NAN), // 0.00128
            (Dynamics, Oversampled, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (Dynamics, Shipping, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.41e-13
            (GroupDelay, Shipping, 2e-4, NAN), // 4.47e-09
            (Harmonics, Oversampled, 2.0, 20.0), // 0.511 / 3.62
            (Harmonics, Shipping, 1.0, 1.0), // 0.269 / 0.288
            (Intermodulation, Oversampled, 0.5, 5.0), // 0.074 / 0.669
            (Intermodulation, Shipping, 0.2, 2.0), // 0.035 / 0.431
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 1e-7, 1e-7), // 1.85e-08 / 3.25e-08
            (Response, Shipping, 0.02, 0.5), // 0.00386 / 0.107 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 50.0, NAN), // 9.53
            (ThdN, Shipping, 200.0, NAN), // 49.6 — aliasing as it ships
        ],
        ("rangemaster", Tier::T1) => &[
            (Aliasing, Oversampled, -91.0, NAN), // -94 — RAISED from -115 (R12): not aliasing at 4×; the 8 kHz +20 dB tone's inharmonic floor is the circuit's own (-64.9 dBc in both), and the check reads the model's excess over the reference — 0.005 dB of agreement on that floor reads -94; the temp-corrected reference moved it from just above the model's to just below
            (Aliasing, Shipping, -27.0, NAN), // -29.7 — aliasing as it ships
            (Anmr, Shipping, 38.0, NAN), // 34.6 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -49.2 / -32.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 0.0), // -23.1 / -6.7 — FINDING: audible as it ships, unoversampled: worst at 115 Hz on the +20 dB multisines, where products folded past Nyquist land on its lines (oversampled it is inaudible)
            (Bla, Oversampled, 0.5, 2.0), // 0.0877 / 0.404
            (Bla, Shipping, 2.0, 100.0), // 0.426 / 28.7
            (Compression, Oversampled, 0.01, 0.5), // 0.00299 / 0.0944
            (Compression, Shipping, 0.2, 10.0), // 0.0511 / 1.9
            (Detection, Oversampled, 0.5, NAN), // 0.135
            (Detection, Shipping, 10.0, NAN), // 1.69
            (Dynamics, Oversampled, 2e-4, 0.02), // 3.39e-05 / 0.0147
            (Dynamics, Shipping, 5e-4, 0.5), // 0.000262 / 0.107
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.47e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 6.32e-07
            (Harmonics, Oversampled, 5.0, 5.0), // 1.04 / 1.01
            (Harmonics, Shipping, 10.0, 50.0), // 1.85 / 11.2
            (Intermodulation, Oversampled, 2.0, 20.0), // 1.04 / 10.4
            (Intermodulation, Shipping, 10.0, 20.0), // 5.17 / 5.56
            (OperatingPoint, Oversampled, 5e-7, NAN), // 9.19e-08 — R12: closed (was 3.9 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 5e-7, NAN), // 9.19e-08
            (Response, Oversampled, 1e-5, 2e-5), // 2.84e-06 / 6.54e-06 — R12: closed with the operating point (was 0.13 dB)
            (Response, Shipping, 0.2, 20.0), // 0.0508 / 3.95
            (ThdN, Oversampled, 0.1, NAN), // 0.0187
            (ThdN, Shipping, 0.5, NAN), // 0.149 — aliasing as it ships
        ],
        ("rangemaster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -91.0, NAN), // -93.7 — RAISED from -115 (R12): not aliasing at 4×; the 8 kHz +20 dB tone's inharmonic floor is the circuit's own (-64.9 dBc in both), and the check reads the model's excess over the reference — 0.005 dB of agreement on that floor reads -94; the temp-corrected reference moved it from just above the model's to just below
            (Aliasing, Shipping, -23.0, NAN), // -25.3 — aliasing as it ships
            (Anmr, Shipping, 41.0, NAN), // 37.3 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -48.0 / -32.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 5.0), // -21.3 / -4.9 — FINDING: audible as it ships, unoversampled: worst at 115 Hz on the +20 dB multisines, where products folded past Nyquist land on its lines (oversampled it is inaudible)
            (Bla, Oversampled, 0.1, 5.0), // 0.0317 / 1.39
            (Bla, Shipping, 5.0, 200.0), // 1.65 / 34.5
            (Compression, Oversampled, 0.02, 0.5), // 0.00362 / 0.107
            (Compression, Shipping, 0.2, 10.0), // 0.0628 / 2.17
            (Detection, Oversampled, 0.5, NAN), // 0.143
            (Detection, Shipping, 10.0, NAN), // 2.14
            (Dynamics, Oversampled, 1e-4, 0.02), // 3.11e-05 / 0.0144
            (Dynamics, Shipping, 5e-4, 0.5), // 0.00024 / 0.106
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.62e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 6.88e-07
            (Harmonics, Oversampled, 5.0, 10.0), // 1.14 / 1.96
            (Harmonics, Shipping, 10.0, 50.0), // 2.92 / 11.7
            (Intermodulation, Oversampled, 2.0, 20.0), // 0.393 / 4.47
            (Intermodulation, Shipping, 10.0, 50.0), // 2.16 / 13.2
            (OperatingPoint, Oversampled, 5e-7, NAN), // 9.19e-08 — R12: closed (was 3.9 mV: the reference had dropped the deck's temp)
            (OperatingPoint, Shipping, 5e-7, NAN), // 9.19e-08
            (Response, Oversampled, 1e-5, 5e-5), // 3.17e-06 / 7.64e-06 — R12: closed with the operating point (was 0.13 dB)
            (Response, Shipping, 0.2, 20.0), // 0.0627 / 3.98
            (ThdN, Oversampled, 0.05, NAN), // 0.00924
            (ThdN, Shipping, 1.0, NAN), // 0.248 — aliasing as it ships
        ],
        ("triode_stage", Tier::T1) => &[
            (Aliasing, Oversampled, -76.0, NAN), // -78.6
            (Aliasing, Shipping, -72.0, NAN), // -74.7 — aliasing as it ships
            (Anmr, Shipping, 6.0, NAN), // 2.8 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -62.1 / -44.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -62.1 / -44.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.000992
            (Bla, Shipping, 0.005, 0.005), // 0.000968 / 0.00131
            (Compression, Oversampled, 0.001, 5e-5), // 0.000243 / 1.03e-05
            (Compression, Shipping, 0.001, 5e-5), // 0.000243 / 1.03e-05
            (Detection, Oversampled, 0.01, NAN), // 0.00327
            (Detection, Shipping, 0.01, NAN), // 0.00327
            (Dynamics, Oversampled, 1e-4, 2e-4), // 2.27e-05 / 6.38e-05
            (Dynamics, Shipping, 1e-4, 2e-4), // 2.27e-05 / 6.38e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.1), // 0.0949 / 0.0281
            (Harmonics, Shipping, 0.5, 0.05), // 0.0949 / 0.00675
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0655 / 0.0131
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0273 / 0.00353
            (OperatingPoint, Oversampled, 5e-9, NAN), // 7.6e-10
            (OperatingPoint, Shipping, 5e-9, NAN), // 7.6e-10
            (Response, Oversampled, 2e-9, NAN), // 5.68e-10
            (Response, Shipping, 2e-9, NAN), // 5.68e-10
            (ThdN, Oversampled, 0.05, NAN), // 0.0101
            (ThdN, Shipping, 0.05, NAN), // 0.0102 — aliasing as it ships
        ],
        ("push_pull", Tier::T1) => &[
            (Aliasing, Oversampled, -50.0, NAN), // -52.8
            (Aliasing, Shipping, -63.0, NAN), // -65.1 — aliasing as it ships
            (Anmr, Shipping, 14.0, NAN), // 10.5 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -60.0 / -43.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -70.9 / -51.0 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.05), // 0.00166 / 0.0114
            (Bla, Shipping, 0.005, 0.01), // 0.00106 / 0.0024
            (Compression, Oversampled, 0.002, 5e-4), // 0.000455 / 0.000105
            (Compression, Shipping, 0.002, 5e-4), // 0.000455 / 0.000105
            (Detection, Oversampled, 0.02, NAN), // 0.00342
            (Detection, Shipping, 0.005, NAN), // 0.00164
            (Dynamics, Oversampled, 2e-4, 5e-4), // 4.54e-05 / 7.9e-05
            (Dynamics, Shipping, 2e-4, 5e-4), // 4.54e-05 / 7.9e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.1), // 0.109 / 0.0175
            (Harmonics, Shipping, 0.1, 0.05), // 0.0283 / 0.0161
            (Intermodulation, Oversampled, 0.1, 0.1), // 0.0307 / 0.0332
            (Intermodulation, Shipping, 0.05, 0.05), // 0.0156 / 0.0157
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.5e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.5e-09
            (Response, Oversampled, 2e-9, NAN), // 4.68e-10
            (Response, Shipping, 2e-9, NAN), // 4.68e-10
            (ThdN, Oversampled, 200.0, NAN), // 64.9
            (ThdN, Shipping, 200.0, NAN), // 52.5 — aliasing as it ships
        ],
        ("push_pull_pentode", Tier::T1) => &[
            (Aliasing, Oversampled, -61.0, NAN), // -63.7
            (Aliasing, Shipping, -70.0, NAN), // -72.1 — aliasing as it ships
            (Anmr, Shipping, 6.0, NAN), // 2.6 — FINDING: above −10 dB as it ships, on the grid's hottest tones (to 8 kHz at +20 dB; R12 chose its factor on the piano, to 4.2 kHz)
            (Audibility, Oversampled, -10.0, -10.0), // -66.4 / -49.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -66.4 / -49.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.02), // 0.00112 / 0.0061
            (Bla, Shipping, 0.005, 0.005), // 0.00105 / 0.00165
            (Compression, Oversampled, 0.002, 5e-4), // 0.000426 / 0.000146
            (Compression, Shipping, 0.002, 5e-4), // 0.000426 / 0.000146
            (Detection, Oversampled, 0.01, NAN), // 0.00217
            (Detection, Shipping, 0.005, NAN), // 0.00146
            (Dynamics, Oversampled, 2e-4, 2e-4), // 5.19e-05 / 3.92e-05
            (Dynamics, Shipping, 2e-4, 1e-4), // 5.19e-05 / 3.25e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 2.0, 1.0), // 0.346 / 0.171
            (Harmonics, Shipping, 0.5, 0.1), // 0.0808 / 0.0322
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.0372 / 0.0235
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0283 / 0.00381
            (OperatingPoint, Oversampled, 1e-7, NAN), // 2.97e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 2.97e-08
            (Response, Oversampled, 2e-9, NAN), // 3.87e-10
            (Response, Shipping, 2e-9, NAN), // 3.87e-10
            (ThdN, Oversampled, 200.0, NAN), // 52.6
            (ThdN, Shipping, 200.0, NAN), // 44.2 — aliasing as it ships
        ],
        ("tube_screamer", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -88.0, NAN), // -90.6 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -17.4 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -62.2 / -44.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -35.3 / -17.6 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.05, 0.1), // 0.0101 / 0.0265
            (Bla, Shipping, 1.0, 2.0), // 0.226 / 0.54 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (Compression, Oversampled, 0.01, 0.05), // 0.00198 / 0.0115
            (Compression, Shipping, 0.2, 1.0), // 0.0448 / 0.232
            (Detection, Oversampled, 0.005, NAN), // 0.000942
            (Detection, Shipping, 0.1, NAN), // 0.0205
            (Dynamics, Oversampled, 5e-5, 0.1), // 1.33e-05 / 0.0224
            (Dynamics, Shipping, 1e-4, 0.2), // 3.13e-05 / 0.0593
            (GroupDelay, Oversampled, 2e-4, NAN), // 8.79e-13
            (GroupDelay, Shipping, 2e-4, NAN), // 6.5e-08
            (Harmonics, Oversampled, 5.0, 20.0), // 0.699 / 3.53
            (Harmonics, Shipping, 2.0, 10.0), // 0.511 / 1.85
            (Intermodulation, Oversampled, 1.0, 1.0), // 0.207 / 0.203
            (Intermodulation, Shipping, 5.0, 5.0), // 1.48 / 1.33
            (OperatingPoint, Oversampled, 2e-9, NAN), // 5.31e-10
            (OperatingPoint, Shipping, 2e-9, NAN), // 5.31e-10
            (Response, Oversampled, 1e-6, 2e-6), // 1.69e-07 / 5.78e-07
            (Response, Shipping, 1.0, 2.0), // 0.228 / 0.476 — R12: shipped oversampled (bilinear warping at 1× read 0.7–5.6 dB at 16 kHz)
            (ThdN, Oversampled, 0.1, NAN), // 0.0249
            (ThdN, Shipping, 100.0, NAN), // 29.4 — aliasing as it ships
        ],
        ("boss_ce2_filters", Tier::T0) => &[
            (Aliasing, Oversampled, -82.0, NAN), // -84.2
            (Aliasing, Shipping, -111.0, NAN), // -113 — aliasing as it ships
            (Audibility, Oversampled, -10.0, -10.0), // -27.5 / -11.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -52.0 / -35.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 5.0, 50.0), // 1.25 / 10.8 — at 16 kHz, 66 dB down in the stopband
            (Bla, Shipping, 0.5, 5.0), // 0.0712 / 0.69 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (Compression, Oversampled, 0.005, 0.05), // 0.00158 / 0.0139
            (Compression, Shipping, 5e-4, 0.005), // 9.49e-05 / 0.000846
            (Detection, Oversampled, 0.1, NAN), // 0.0238
            (Detection, Shipping, 0.005, NAN), // 0.00139
            (GroupDelay, Oversampled, 2e-4, NAN), // 6.48e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 1.5e-08
            (Harmonics, Oversampled, 2.0, 20.0), // 0.54 / 5.41
            (Harmonics, Shipping, 0.1, 1.0), // 0.0322 / 0.33 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.12e-07
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.12e-07
            (Response, Oversampled, 2e-4, 0.001), // 4.45e-05 / 0.000321
            (Response, Shipping, 0.5, 0.5), // 0.077 / 0.153 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (ThdN, Oversampled, 0.01, NAN), // 0.00264
            (ThdN, Shipping, 0.001, NAN), // 0.000172 — aliasing as it ships
        ],
        ("boss_ce2_filters", Tier::T1) => &[
            (Aliasing, Oversampled, -37.0, NAN), // -39.7
            (Aliasing, Shipping, -70.0, NAN), // -72.5 — aliasing as it ships
            (Audibility, Oversampled, -10.0, 0.0), // -27.1 / -9.6 — FINDING: audible, just: the 8 kHz tone at the knee, its harmonics in the sixth-order stopband at 4× (T1's factor; it ships at 16×)
            (Audibility, Shipping, -10.0, -10.0), // -50.2 / -32.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 5.0, 20.0), // 1.44 / 5.86 — at 16 kHz, 66 dB down in the stopband
            (Bla, Shipping, 0.5, 2.0), // 0.101 / 0.374 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (Compression, Oversampled, 1.0, 5.0), // 0.217 / 1.52
            (Compression, Shipping, 0.05, 0.5), // 0.0149 / 0.107 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (Detection, Oversampled, 0.2, NAN), // 0.0558
            (Detection, Shipping, 0.02, NAN), // 0.00386
            (Dynamics, Oversampled, 0.001, 0.05), // 0.00022 / 0.00818
            (Dynamics, Shipping, 5e-4, 0.02), // 9.85e-05 / 0.00579
            (GroupDelay, Oversampled, 2e-4, NAN), // 6.48e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 1.77e-08
            (Harmonics, Oversampled, 50.0, 100.0), // 16.2 / 17 — an 8 kHz tone's H-products near −105 dBc: aliasing at 4×
            (Harmonics, Shipping, 0.5, 2.0), // 0.13 / 0.395 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (Intermodulation, Oversampled, 100.0, 50.0), // 18.9 / 10.3 — CCIF products near −94 dBc: aliasing at 4×
            (Intermodulation, Shipping, 1.0, 2.0), // 0.183 / 0.341 — aliasing as it ships
            (OperatingPoint, Oversampled, 1e-6, NAN), // 2.12e-07
            (OperatingPoint, Shipping, 1e-6, NAN), // 2.12e-07
            (Response, Oversampled, 2e-4, 0.001), // 4.45e-05 / 0.000321
            (Response, Shipping, 0.5, 1.0), // 0.0913 / 0.181 — R12: shipped oversampled at 16× (at 1× the bilinear map left its stopband 28 dB shallow)
            (ThdN, Oversampled, 5.0, NAN), // 0.679
            (ThdN, Shipping, 0.2, NAN), // 0.047 — aliasing as it ships
        ],
        ("tube_screamer_ts808", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -69 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -56.2 / -39.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -7.7 / 8.9 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.1, 0.2), // 0.022 / 0.0616
            (Bla, Shipping, 50.0, 100.0), // 8.9 / 19.3 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.0002, 0.001), // 4.73e-05 / 0.00033
            (Compression, Shipping, 0.02, 0.5), // 0.00582 / 0.0979
            (Detection, Oversampled, 2.0, NAN), // 0.484
            (Detection, Shipping, 2.0, NAN), // 0.455
            (GroupDelay, Oversampled, 0.0002, NAN), // 7.99e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 1.12e-06
            (Harmonics, Oversampled, 0.05, 0.1), // 0.0123 / 0.0292
            (Harmonics, Shipping, 20.0, 50.0), // 5.1 / 10.1 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 1e-06, NAN), // 2.01e-07
            (OperatingPoint, Shipping, 1e-06, NAN), // 2.01e-07
            (Response, Oversampled, 5e-07, 2e-06), // 8.95e-08 / 5.65e-07
            (Response, Shipping, 50.0, 50.0), // 8.9 / 8.42 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.002, NAN), // 0.000422
            (ThdN, Shipping, 0.5, NAN), // 0.11
        ],
        ("tube_screamer_ts808", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -10.0, NAN), // -12.5 — aliasing as it ships
            (Anmr, Shipping, 73.0, NAN), // 69.9 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -56.2 / -38.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 20.0), // -6.3 / 11.3 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.1, 0.1), // 0.022 / 0.0322
            (Bla, Shipping, 50.0, 50.0), // 11.5 / 12.4 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.02, 0.05), // 0.00495 / 0.0153
            (Compression, Shipping, 10.0, 50.0), // 1.85 / 9.4 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 2.0, NAN), // 0.484
            (Detection, Shipping, 200.0, NAN), // 35.3 — aliasing as it ships
            (Dynamics, Oversampled, 5e-05, 0.2), // 1.37e-05 / 0.0486
            (Dynamics, Shipping, 0.001, 5.0), // 0.000211 / 0.781
            (GroupDelay, Oversampled, 0.0002, NAN), // 7.99e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 1.33e-06
            (Harmonics, Oversampled, 5.0, 20.0), // 1.27 / 3.63
            (Harmonics, Shipping, 200.0, 500.0), // 43.9 / 75 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 2.0, 1.0), // 0.377 / 0.249
            (Intermodulation, Shipping, 200.0, 100.0), // 63.4 / 25 — aliasing as it ships
            (OperatingPoint, Oversampled, 1e-06, NAN), // 2.01e-07
            (OperatingPoint, Shipping, 1e-06, NAN), // 2.01e-07
            (Response, Oversampled, 5e-07, 2e-06), // 8.95e-08 / 5.65e-07
            (Response, Shipping, 50.0, 50.0), // 11.5 / 10.1 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 1.0, NAN), // 0.321
            (ThdN, Shipping, 500.0, NAN), // 69.6 — aliasing as it ships
        ],
        ("big_muff_rams_head", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -117.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -49.5 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -54.4 / -38.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -7.1 / 9.4 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.2, 0.2), // 0.0341 / 0.0463
            (Bla, Shipping, 50.0, 50.0), // 13.6 / 13.8 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.0001, 0.0005), // 2.49e-05 / 0.000166
            (Compression, Shipping, 0.05, 0.2), // 0.00715 / 0.0458
            (Detection, Oversampled, 0.02, NAN), // 0.00637
            (Detection, Shipping, 0.05, NAN), // 0.00874
            (GroupDelay, Oversampled, 0.0002, NAN), // 2.14e-11
            (GroupDelay, Shipping, 0.0002, NAN), // 1.85e-06
            (Harmonics, Oversampled, 0.05, 0.2), // 0.00859 / 0.0607
            (Harmonics, Shipping, 10.0, 100.0), // 2.97 / 17.2 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 5e-06, NAN), // 8.52e-07
            (OperatingPoint, Shipping, 5e-06, NAN), // 8.52e-07
            (Response, Oversampled, 1e-05, 2e-05), // 2.33e-06 / 5.37e-06
            (Response, Shipping, 50.0, 50.0), // 13.5 / 12.5 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.005, NAN), // 0.000669
            (ThdN, Shipping, 0.2, NAN), // 0.0546
        ],
        ("big_muff_rams_head", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -64.0, NAN), // -66.3 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -36 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -54.2 / -37.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 20.0), // -5.6 / 11.2 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.2, 0.2), // 0.0341 / 0.0478
            (Bla, Shipping, 100.0, 100.0), // 17.4 / 19.4 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.05, 0.1), // 0.0079 / 0.0178
            (Compression, Shipping, 10.0, 20.0), // 2.89 / 5.98 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 0.5, NAN), // 0.132
            (Detection, Shipping, 0.5, NAN), // 0.116
            (Dynamics, Oversampled, 0.0001, 0.2), // 1.95e-05 / 0.0663
            (Dynamics, Shipping, 0.001, 5.0), // 0.000302 / 1.15
            (GroupDelay, Oversampled, 0.0002, NAN), // 2.14e-11
            (GroupDelay, Shipping, 0.0002, NAN), // 2.2e-06
            (Harmonics, Oversampled, 0.5, 0.5), // 0.0755 / 0.0729
            (Harmonics, Shipping, 100.0, 100.0), // 24.8 / 29.5 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 0.1, 0.002), // 0.0262 / 0.000464
            (Intermodulation, Shipping, 50.0, 0.02), // 15.4 / 0.00351 — aliasing as it ships
            (OperatingPoint, Oversampled, 5e-06, NAN), // 8.52e-07
            (OperatingPoint, Shipping, 5e-06, NAN), // 8.52e-07
            (Response, Oversampled, 1e-05, 2e-05), // 2.33e-06 / 5.37e-06
            (Response, Shipping, 100.0, 50.0), // 17.4 / 15.2 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 100.0, NAN), // 17.5
            (ThdN, Shipping, 100.0, NAN), // 17.5 — aliasing as it ships
        ],
        ("proco_rat", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -71.0, NAN), // -73.6 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -26.9 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -74.1 / -56.3 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -8.5 / 9.1 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.01, 0.05), // 0.00246 / 0.0108
            (Bla, Shipping, 50.0, 100.0), // 7.14 / 19 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 5e-05, 0.0002), // 9.45e-06 / 4.08e-05
            (Compression, Shipping, 0.1, 0.2), // 0.0171 / 0.0549
            (Detection, Oversampled, 20.0, NAN), // 6.52
            (Detection, Shipping, 20.0, NAN), // 6.53 — aliasing as it ships
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.11e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 3.6e-07
            (Harmonics, Oversampled, 0.2, 0.02), // 0.0404 / 0.00526
            (Harmonics, Shipping, 5.0, 50.0), // 1.16 / 9.39 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 1e-09, NAN), // 3.33e-10
            (OperatingPoint, Shipping, 1e-09, NAN), // 3.33e-10
            (Response, Oversampled, 1e-08, 5e-08), // 2.81e-09 / 9.49e-09
            (Response, Shipping, 50.0, 100.0), // 7.14 / 19 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.05, NAN), // 0.0096
            (ThdN, Shipping, 0.5, NAN), // 0.108
        ],
        ("proco_rat", Tier::T1) => &[
            (Aliasing, Oversampled, -117.0, NAN), // -120
            (Aliasing, Shipping, -43.0, NAN), // -45.2 — aliasing as it ships
            (Anmr, Shipping, 28.0, NAN), // 24.1 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -74 / -56.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 20.0), // -6.7 / 11 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.01, 0.05), // 0.00245 / 0.00954
            (Bla, Shipping, 50.0, 100.0), // 9.49 / 24.7 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.002, 0.01), // 0.000457 / 0.00175
            (Compression, Shipping, 5.0, 10.0), // 1.25 / 3.17 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 20.0, NAN), // 6.52
            (Detection, Shipping, 100.0, NAN), // 16.9 — aliasing as it ships
            (Dynamics, Oversampled, 0.0002, 0.2), // 5.98e-05 / 0.0628
            (Dynamics, Shipping, 0.002, 5.0), // 0.000604 / 0.678
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.11e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 4.29e-07
            (Harmonics, Oversampled, 1.0, 0.5), // 0.23 / 0.0869
            (Harmonics, Shipping, 100.0, 200.0), // 18 / 42.9 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 1.0, 0.02), // 0.172 / 0.0056
            (Intermodulation, Shipping, 100.0, 10.0), // 31.7 / 2.65 — aliasing as it ships
            (OperatingPoint, Oversampled, 1e-09, NAN), // 3.33e-10
            (OperatingPoint, Shipping, 1e-09, NAN), // 3.33e-10
            (Response, Oversampled, 1e-08, 5e-08), // 2.81e-09 / 9.49e-09
            (Response, Shipping, 50.0, 100.0), // 9.44 / 24.5 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 50.0, NAN), // 12.1
            (ThdN, Shipping, 50.0, NAN), // 13.4 — aliasing as it ships
        ],
        ("marshall_jcm800_2203", Tier::T0) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -108
            (Aliasing, Shipping, -59.0, NAN), // -61.8 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -16.4 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -72.2 / -58.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -26.1 / -11 — inaudible (R16: ≤ −10 dB total and worst band)
            (Bla, Oversampled, 0.005, 0.1), // 0.00124 / 0.0191
            (Bla, Shipping, 5.0, 20.0), // 0.755 / 3.8
            (Compression, Oversampled, 0.0002, 0.0002), // 5.18e-05 / 5.9e-05
            (Compression, Shipping, 0.05, 0.05), // 0.0132 / 0.0162
            (Detection, Oversampled, 0.5, NAN), // 0.129
            (Detection, Shipping, 5.0, NAN), // 1.27 — aliasing as it ships
            (GroupDelay, Oversampled, 0.0002, NAN), // 4.02e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 6.71e-07
            (Harmonics, Oversampled, 0.02, 0.05), // 0.00335 / 0.00785
            (Harmonics, Shipping, 2.0, 5.0), // 0.547 / 0.997
            (OperatingPoint, Oversampled, 2e-07, NAN), // 4.09e-08
            (OperatingPoint, Shipping, 2e-07, NAN), // 4.09e-08
            (Response, Oversampled, 1e-08, 5e-08), // 2.09e-09 / 7.22e-09
            (Response, Shipping, 0.2, 20.0), // 0.0615 / 3.52
            (ThdN, Oversampled, 0.0005, NAN), // 0.000127
            (ThdN, Shipping, 0.2, NAN), // 0.0376
        ],
        ("marshall_jcm800_2203", Tier::T1) => &[
            (Aliasing, Oversampled, -63.0, NAN), // -65.3
            (Aliasing, Shipping, -7.0, NAN), // -9.1 — aliasing as it ships
            (Anmr, Shipping, 64.0, NAN), // 60.4 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -57.4 / -40.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 10.0), // -13.8 / 3.6 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.02, 0.2), // 0.00372 / 0.0622
            (Bla, Shipping, 5.0, 50.0), // 1.09 / 15.9 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.002, 0.02), // 0.000476 / 0.00582
            (Compression, Shipping, 0.5, 10.0), // 0.0727 / 2.01
            (Detection, Oversampled, 1.0, NAN), // 0.232
            (Detection, Shipping, 20.0, NAN), // 4.83 — aliasing as it ships
            (Dynamics, Oversampled, 0.005, 0.05), // 0.00119 / 0.0113
            (Dynamics, Shipping, 0.1, 0.5), // 0.0179 / 0.162
            (GroupDelay, Oversampled, 0.0002, NAN), // 4.02e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 7.96e-07
            (Harmonics, Oversampled, 2.0, 1.0), // 0.335 / 0.292
            (Harmonics, Shipping, 20.0, 20.0), // 3.88 / 5.01 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 0.5, 2.0), // 0.0827 / 0.544
            (Intermodulation, Shipping, 2.0, 20.0), // 0.366 / 6.2 — aliasing as it ships
            (OperatingPoint, Oversampled, 2e-07, NAN), // 4.09e-08
            (OperatingPoint, Shipping, 2e-07, NAN), // 4.09e-08
            (Response, Oversampled, 1e-08, 5e-08), // 2.09e-09 / 7.22e-09
            (Response, Shipping, 0.5, 20.0), // 0.0725 / 4.24
            (ThdN, Oversampled, 0.05, NAN), // 0.0134
            (ThdN, Shipping, 50.0, NAN), // 14.2 — aliasing as it ships
        ],
        ("moog_ladder_feedback", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -45.4 / -29.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 10.0), // -10.7 / 4.8 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 1.0, 1.0), // 0.202 / 0.251
            (Bla, Shipping, 100.0, 100.0), // 18.4 / 30.5 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.0005, 0.01), // 0.000122 / 0.00243
            (Compression, Shipping, 0.05, 0.5), // 0.00761 / 0.156
            (Detection, Oversampled, 0.002, NAN), // 0.000587
            (Detection, Shipping, 0.5, NAN), // 0.0991
            (GroupDelay, Oversampled, 0.0002, NAN), // 6.75e-10
            (GroupDelay, Shipping, 0.0002, NAN), // 8.88e-06
            (Harmonics, Oversampled, 0.1, 0.5), // 0.0297 / 0.0692
            (Harmonics, Shipping, 10.0, 20.0), // 2.28 / 4.69 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 1e-06, NAN), // 1.99e-07
            (OperatingPoint, Shipping, 1e-06, NAN), // 1.99e-07
            (Response, Oversampled, 0.0001, 0.0005), // 2.43e-05 / 0.000126
            (Response, Shipping, 100.0, 50.0), // 18.1 / 11.3 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.02, NAN), // 0.0034
            (ThdN, Shipping, 1.0, NAN), // 0.221
        ],
        ("moog_ladder_feedback", Tier::T1) => &[
            (Aliasing, Oversampled, -56.0, NAN), // -58.2
            (Aliasing, Shipping, 22.0, NAN), // 19.8 — aliasing as it ships
            (Anmr, Shipping, 81.0, NAN), // 77.3 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -45 / -25.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 20.0), // -8.9 / 10.9 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 1.0, 1.0), // 0.203 / 0.257
            (Bla, Shipping, 100.0, 200.0), // 23.8 / 48.6 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.2, 0.5), // 0.0475 / 0.0756
            (Compression, Shipping, 20.0, 20.0), // 4.03 / 5.91 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 0.01, NAN), // 0.00227
            (Detection, Shipping, 500.0, NAN), // 90.7 — aliasing as it ships
            (Dynamics, Oversampled, 5e-08, 0.002), // 8.2e-09 / 0.00041
            (Dynamics, Shipping, 2e-05, 1.0), // 3.86e-06 / 0.271
            (GroupDelay, Oversampled, 0.0002, NAN), // 6.75e-10
            (GroupDelay, Shipping, 0.0002, NAN), // 1.06e-05
            (Harmonics, Oversampled, 20.0, 5.0), // 4.31 / 0.725
            (Harmonics, Shipping, 100.0, 500.0), // 32.2 / 151 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 10.0, 5.0), // 3.15 / 1.12
            (Intermodulation, Shipping, 500.0, 100.0), // 77.1 / 19.2 — aliasing as it ships
            (OperatingPoint, Oversampled, 1e-06, NAN), // 1.99e-07
            (OperatingPoint, Shipping, 1e-06, NAN), // 1.99e-07
            (Response, Oversampled, 0.0001, 0.0005), // 2.43e-05 / 0.000126
            (Response, Shipping, 100.0, 50.0), // 23.4 / 13.6 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 200.0, NAN), // 61.8
            (ThdN, Shipping, 500.0, NAN), // 140 — aliasing as it ships
        ],
        ("mxr_phase90", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -68.0, NAN), // -70.4 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -13.8 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -53 / -36.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 5.0, 20.0), // -4.6 / 11.9 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.1, 0.2), // 0.0301 / 0.0625
            (Bla, Shipping, 100.0, 200.0), // 24.4 / 63.6 — FINDING: the notches move under bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.002, 0.005), // 0.000548 / 0.000927
            (Compression, Shipping, 0.5, 1.0), // 0.148 / 0.249
            (Detection, Oversampled, 0.005, NAN), // 0.00162
            (Detection, Shipping, 2.0, NAN), // 0.477
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.68e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 2.73e-06
            (Harmonics, Oversampled, 0.05, 0.5), // 0.0139 / 0.0999
            (Harmonics, Shipping, 20.0, 100.0), // 4.78 / 31.3 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 2e-06, NAN), // 5.94e-07
            (OperatingPoint, Shipping, 2e-06, NAN), // 5.94e-07
            (Response, Oversampled, 1e-06, 1e-06), // 2.94e-07 / 2.06e-07
            (Response, Shipping, 50.0, 100.0), // 7.74 / 19.8 — FINDING: the notches move under bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.005, NAN), // 0.000895
            (ThdN, Shipping, 1.0, NAN), // 0.254
        ],
        ("mxr_phase90", Tier::T1) => &[
            (Aliasing, Oversampled, -83.0, NAN), // -85.4
            (Aliasing, Shipping, -13.0, NAN), // -15.4 — aliasing as it ships
            (Anmr, Shipping, 65.0, NAN), // 61.1 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -33.2 / -12.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 25.0, 45.0), // 17.2 / 37.6 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.1, 0.5), // 0.0297 / 0.136
            (Bla, Shipping, 100.0, 500.0), // 24.3 / 153 — FINDING: the notches move under bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.1, 0.1), // 0.0222 / 0.0265
            (Compression, Shipping, 50.0, 100.0), // 15.5 / 25.7 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 0.01, NAN), // 0.00189
            (Detection, Shipping, 2.0, NAN), // 0.628
            (Dynamics, Oversampled, 5e-05, 0.01), // 8.13e-06 / 0.00202
            (Dynamics, Shipping, 0.0005, 0.2), // 0.000124 / 0.0606
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.68e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 3.24e-06
            (Harmonics, Oversampled, 5.0, 5.0), // 0.904 / 1.21
            (Harmonics, Shipping, 100.0, 500.0), // 27.8 / 163 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 2.0, 5.0), // 0.55 / 0.721
            (Intermodulation, Shipping, 100.0, 500.0), // 29.2 / 114 — aliasing as it ships
            (OperatingPoint, Oversampled, 2e-06, NAN), // 5.94e-07
            (OperatingPoint, Shipping, 2e-06, NAN), // 5.94e-07
            (Response, Oversampled, 1e-06, 1e-06), // 2.94e-07 / 2.06e-07
            (Response, Shipping, 50.0, 100.0), // 10.5 / 24.5 — FINDING: the notches move under bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.5, NAN), // 0.0725
            (ThdN, Shipping, 50.0, NAN), // 14.4 — aliasing as it ships
        ],
        ("boss_ds1", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -68.0, NAN), // -70.4 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -26 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -75.2 / -56.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -9 / 9.1 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.01, 0.1), // 0.00245 / 0.0234
            (Bla, Shipping, 50.0, 200.0), // 7.59 / 35.6 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 5e-05, 0.0002), // 9.27e-06 / 3.75e-05
            (Compression, Shipping, 0.05, 0.2), // 0.00712 / 0.0368
            (Detection, Oversampled, 5.0, NAN), // 0.843
            (Detection, Shipping, 10.0, NAN), // 2.31 — aliasing as it ships
            (GroupDelay, Oversampled, 0.0002, NAN), // 7.87e-12
            (GroupDelay, Shipping, 0.0002, NAN), // 1.3e-06
            (Harmonics, Oversampled, 0.05, 0.02), // 0.0116 / 0.00515
            (Harmonics, Shipping, 5.0, 50.0), // 1 / 7.62 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 5e-06, NAN), // 1.12e-06
            (OperatingPoint, Shipping, 5e-06, NAN), // 1.12e-06
            (Response, Oversampled, 1e-05, 5e-05), // 2.35e-06 / 6.85e-06
            (Response, Shipping, 50.0, 200.0), // 7.59 / 35.4 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.005, NAN), // 0.00125
            (ThdN, Shipping, 0.05, NAN), // 0.0117
        ],
        ("boss_ds1", Tier::T1) => &[
            (Aliasing, Oversampled, -88.0, NAN), // -90.1
            (Aliasing, Shipping, -10.0, NAN), // -12.4 — aliasing as it ships
            (Anmr, Shipping, 68.0, NAN), // 65 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -73.9 / -55.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 20.0), // -5.1 / 12.1 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.01, 0.1), // 0.00247 / 0.0196
            (Bla, Shipping, 50.0, 200.0), // 10.2 / 45 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.001, 0.02), // 0.000182 / 0.00508
            (Compression, Shipping, 5.0, 50.0), // 0.808 / 14
            (Detection, Oversampled, 5.0, NAN), // 0.843
            (Detection, Shipping, 50.0, NAN), // 13.9 — aliasing as it ships
            (Dynamics, Oversampled, 0.0001, 1.0), // 2.52e-05 / 0.234
            (Dynamics, Shipping, 0.001, 10.0), // 0.00024 / 2.43
            (GroupDelay, Oversampled, 0.0002, NAN), // 7.87e-12
            (GroupDelay, Shipping, 0.0002, NAN), // 1.54e-06
            (Harmonics, Oversampled, 1.0, 1.0), // 0.245 / 0.271
            (Harmonics, Shipping, 100.0, 500.0), // 21.3 / 96.5 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 0.5, 0.5), // 0.0986 / 0.0765
            (Intermodulation, Shipping, 200.0, 500.0), // 44.8 / 96.4 — aliasing as it ships
            (OperatingPoint, Oversampled, 5e-06, NAN), // 1.12e-06
            (OperatingPoint, Shipping, 5e-06, NAN), // 1.12e-06
            (Response, Oversampled, 1e-05, 5e-05), // 2.35e-06 / 6.85e-06
            (Response, Shipping, 50.0, 200.0), // 10.2 / 44.8 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 20.0, NAN), // 4.75
            (ThdN, Shipping, 100.0, NAN), // 26.6 — aliasing as it ships
        ],
        ("klon_centaur", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -77 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -46.8 / -30.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 15.0), // -10.3 / 6.5 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.2, 0.1), // 0.0558 / 0.0269
            (Bla, Shipping, 20.0, 10.0), // 4.94 / 1.86 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.0002, 0.005), // 3.96e-05 / 0.00121
            (Compression, Shipping, 0.01, 0.5), // 0.00293 / 0.0786
            (Detection, Oversampled, 0.02, NAN), // 0.00631
            (Detection, Shipping, 1.0, NAN), // 0.328
            (GroupDelay, Oversampled, 0.0002, NAN), // 1.3e-12
            (GroupDelay, Shipping, 0.0002, NAN), // 6.16e-07
            (Harmonics, Oversampled, 0.2, 0.1), // 0.0371 / 0.0174
            (Harmonics, Shipping, 10.0, 5.0), // 3.14 / 1.18 — FINDING: bilinear warping and aliasing at 1×
            (OperatingPoint, Oversampled, 2e-09, NAN), // 4.65e-10
            (OperatingPoint, Shipping, 2e-09, NAN), // 4.65e-10
            (Response, Oversampled, 5e-06, 5e-06), // 1.09e-06 / 1.31e-06
            (Response, Shipping, 20.0, 10.0), // 4.94 / 1.87 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.005, NAN), // 0.000919
            (ThdN, Shipping, 0.2, NAN), // 0.0622
        ],
        ("klon_centaur", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -25.0, NAN), // -27.9 — aliasing as it ships
            (Anmr, Shipping, 59.0, NAN), // 55.6 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -46.8 / -29.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -8.8 / 9.1 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.2, 0.1), // 0.0558 / 0.0269
            (Bla, Shipping, 20.0, 10.0), // 6.32 / 2.12 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.05, 0.05), // 0.0164 / 0.00878
            (Compression, Shipping, 5.0, 2.0), // 1.37 / 0.66 — FINDING: bilinear warping at 1× (no prewarp)
            (Detection, Oversampled, 0.05, NAN), // 0.0156
            (Detection, Shipping, 200.0, NAN), // 40 — aliasing as it ships
            (Dynamics, Oversampled, 5e-06, 0.002), // 6.86e-07 / 0.000353
            (Dynamics, Shipping, 0.005, 5.0), // 0.000979 / 0.732
            (GroupDelay, Oversampled, 0.0002, NAN), // 1.3e-12
            (GroupDelay, Shipping, 0.0002, NAN), // 7.3e-07
            (Harmonics, Oversampled, 20.0, 1.0), // 3.84 / 0.275
            (Harmonics, Shipping, 100.0, 100.0), // 22.8 / 21 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 5.0, 0.5), // 0.754 / 0.117
            (Intermodulation, Shipping, 100.0, 20.0), // 21.6 / 5.19 — aliasing as it ships
            (OperatingPoint, Oversampled, 2e-09, NAN), // 4.65e-10
            (OperatingPoint, Shipping, 2e-09, NAN), // 4.65e-10
            (Response, Oversampled, 5e-06, 5e-06), // 1.09e-06 / 1.31e-06
            (Response, Shipping, 20.0, 10.0), // 6.32 / 2.12 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.2, NAN), // 0.0451
            (ThdN, Shipping, 500.0, NAN), // 92.1 — aliasing as it ships
        ],
        ("mxr_distortion_plus", Tier::T0) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120 — aliasing as it ships
            (Anmr, Shipping, -10.0, NAN), // -121 — inaudible (Kahles et al.'s −10 dB)
            (Audibility, Oversampled, -10.0, -10.0), // -78.8 / -60.7 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, 15.0), // -10.9 / 7.2 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.005, 0.05), // 0.00146 / 0.00954
            (Bla, Shipping, 20.0, 100.0), // 5.02 / 18.1 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 5e-05, 0.0002), // 7.75e-06 / 4.01e-05
            (Compression, Shipping, 0.02, 0.2), // 0.00453 / 0.0547
            (Detection, Oversampled, 2.0, NAN), // 0.558
            (Detection, Shipping, 2.0, NAN), // 0.5
            (GroupDelay, Oversampled, 0.0002, NAN), // 5.11e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 1.05e-06
            (Harmonics, Oversampled, 0.002, 0.01), // 0.000652 / 0.00208
            (Harmonics, Shipping, 1.0, 10.0), // 0.204 / 2.56
            (OperatingPoint, Oversampled, 5e-13, NAN), // 8.79e-14
            (OperatingPoint, Shipping, 5e-13, NAN), // 8.79e-14
            (Response, Oversampled, 5e-06, 1e-05), // 1.25e-06 / 2.13e-06
            (Response, Shipping, 20.0, 100.0), // 5.02 / 18.2 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.0001, NAN), // 2.89e-05
            (ThdN, Shipping, 0.05, NAN), // 0.00873
        ],
        ("mxr_distortion_plus", Tier::T1) => &[
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -18.0, NAN), // -20.3 — aliasing as it ships
            (Anmr, Shipping, 56.0, NAN), // 52.6 — audible as it ships (1×): aliasing
            (Audibility, Oversampled, -10.0, -10.0), // -78.5 / -60.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, 0.0, 15.0), // -8.7 / 9.3 — FINDING: audible as it ships (1×): bilinear warping and aliasing
            (Bla, Oversampled, 0.005, 0.05), // 0.00146 / 0.00954
            (Bla, Shipping, 20.0, 100.0), // 6.64 / 23.5 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.001, 0.01), // 0.00022 / 0.00304
            (Compression, Shipping, 5.0, 20.0), // 0.843 / 4.75
            (Detection, Oversampled, 5.0, NAN), // 1.47
            (Detection, Shipping, 200.0, NAN), // 65.8 — aliasing as it ships
            (Dynamics, Oversampled, 5e-05, 0.2), // 7.98e-06 / 0.0345
            (Dynamics, Shipping, 0.0005, 2.0), // 8.29e-05 / 0.442
            (GroupDelay, Oversampled, 0.0002, NAN), // 5.11e-13
            (GroupDelay, Shipping, 0.0002, NAN), // 1.24e-06
            (Harmonics, Oversampled, 0.5, 5.0), // 0.115 / 0.922
            (Harmonics, Shipping, 50.0, 500.0), // 15.4 / 70.4 — FINDING: bilinear warping and aliasing at 1×
            (Intermodulation, Oversampled, 0.2, 0.5), // 0.0366 / 0.153
            (Intermodulation, Shipping, 200.0, 500.0), // 62.1 / 101 — aliasing as it ships
            (OperatingPoint, Oversampled, 5e-13, NAN), // 8.79e-14
            (OperatingPoint, Shipping, 5e-13, NAN), // 8.79e-14
            (Response, Oversampled, 5e-06, 1e-05), // 1.25e-06 / 2.13e-06
            (Response, Shipping, 20.0, 100.0), // 6.64 / 23.5 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 50.0, NAN), // 8.9
            (ThdN, Shipping, 500.0, NAN), // 99.7 — aliasing as it ships
        ],
        // Cathode bias and blocking (DSP.md R15), measured 2026-11-13. The
        // hand topologies' rows are the tabulated stages under
        // `Bias::Cathode` held to the circuit: their findings are marked.
        ("triode_stage_cathode", Tier::T0) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -62.3 / -50.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -62.3 / -50.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -109.0, NAN), // -111.1
            (Aliasing, Shipping, -109.0, NAN), // -111.1
            (Anmr, Shipping, -10.0, NAN), // -24.6
            (Bla, Oversampled, 0.005, 0.002), // 0.000969 / 0.00037
            (Bla, Shipping, 0.005, 0.002), // 0.000969 / 0.00037
            (Compression, Oversampled, 2e-05, 0.0001), // 3.61e-06 / 2.12e-05
            (Compression, Shipping, 2e-05, 0.0001), // 3.61e-06 / 2.12e-05
            (Detection, Oversampled, 0.005, NAN), // 0.00115
            (Detection, Shipping, 0.005, NAN), // 0.00115
            (Dynamics, Oversampled, 0.0002, 0.0002), // 5.67e-05 / 4.1e-05
            (Dynamics, Shipping, 0.0002, 0.0002), // 5.67e-05 / 4.1e-05
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.62e-15
            (GroupDelay, Shipping, 0.0002, NAN), // 6.32e-11
            (Harmonics, Oversampled, 2.0, 0.05), // 0.408 / 0.00696
            (Harmonics, Shipping, 2.0, 0.05), // 0.408 / 0.00696
            (OperatingPoint, Oversampled, 5e-08, NAN), // 8.09e-09
            (OperatingPoint, Shipping, 5e-08, NAN), // 8.09e-09
            (Recovery, Oversampled, 0.0002, NAN), // 4.3e-05
            (Recovery, Shipping, 0.0002, NAN), // 4.3e-05
            (Response, Oversampled, 5e-09, 2e-09), // 6.88e-10 / 6.27e-10
            (Response, Shipping, 2e-07, 0.002), // 4.76e-08 / 0.000372
            (ThdN, Oversampled, 0.0002, NAN), // 4.18e-05
            (ThdN, Shipping, 0.0002, NAN), // 4.18e-05
        ],
        ("triode_stage_cathode", Tier::T1) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -62.1 / -44.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -61.5 / -44.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -76.0, NAN), // -78.4
            (Aliasing, Shipping, -75.0, NAN), // -77.6
            (Anmr, Shipping, 5.6, NAN), // 2.6 — FINDING: audible on the matrix's 8 kHz tones at +20 dB at 4×, as it ships (the piano sweep reads −42.7 dB)
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.00103
            (Bla, Shipping, 0.005, 0.005), // 0.000968 / 0.00145
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.00019
            (Compression, Shipping, 0.001, 0.001), // 0.000244 / 0.000225
            (Detection, Oversampled, 0.02, NAN), // 0.00452
            (Detection, Shipping, 0.02, NAN), // 0.00494
            (Dynamics, Oversampled, 0.0002, 0.0002), // 5.67e-05 / 4.1e-05
            (Dynamics, Shipping, 0.0005, 0.0002), // 6.83e-05 / 4.65e-05
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.62e-15
            (GroupDelay, Shipping, 0.0002, NAN), // 7.49e-11
            (Harmonics, Oversampled, 2.0, 0.5), // 0.408 / 0.123
            (Harmonics, Shipping, 2.0, 0.5), // 0.448 / 0.102
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0495 / 0.0138
            (Intermodulation, Shipping, 0.2, 0.01), // 0.0426 / 0.00308
            (OperatingPoint, Oversampled, 5e-08, NAN), // 8.09e-09
            (OperatingPoint, Shipping, 5e-08, NAN), // 8.09e-09
            (Recovery, Oversampled, 0.0002, NAN), // 4.49e-05
            (Recovery, Shipping, 0.0002, NAN), // 4.5e-05
            (Response, Oversampled, 5e-09, 2e-09), // 6.88e-10 / 6.27e-10
            (Response, Shipping, 2e-07, 0.002), // 5.62e-08 / 0.000441
            (ThdN, Oversampled, 0.02, NAN), // 0.0054
            (ThdN, Shipping, 0.02, NAN), // 0.00541
        ],
        ("push_pull_cathode", Tier::T0) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -82.5 / -64.9 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -82.8 / -65.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120
            (Anmr, Shipping, -10.0, NAN), // -36.4
            (Bla, Oversampled, 0.005, 0.01), // 0.000968 / 0.00216
            (Bla, Shipping, 0.005, 0.005), // 0.000968 / 0.00104
            (Compression, Oversampled, 2e-05, 0.0005), // 5.1e-06 / 9.6e-05
            (Compression, Shipping, 2e-05, 0.0001), // 5.2e-06 / 2.38e-05
            (Detection, Oversampled, 0.002, NAN), // 0.00037
            (Detection, Shipping, 0.002, NAN), // 0.000372
            (Dynamics, Oversampled, 0.0005, 0.002), // 9.91e-05 / 0.000435
            (Dynamics, Shipping, 0.0002, 0.002), // 5.06e-05 / 0.000494
            (GroupDelay, Oversampled, 0.0002, NAN), // 0
            (GroupDelay, Shipping, 0.0002, NAN), // 0
            (Harmonics, Oversampled, 0.01, 0.02), // 0.00294 / 0.00587
            (Harmonics, Shipping, 0.01, 0.005), // 0.00289 / 0.00135
            (OperatingPoint, Oversampled, 5e-08, NAN), // 8.16e-09
            (OperatingPoint, Shipping, 5e-08, NAN), // 8.16e-09
            (Response, Oversampled, 1e-09, 1e-09), // 2.91e-10 / 2.52e-31
            (Response, Shipping, 1e-09, 1e-09), // 2.91e-10 / 2.52e-31
            (ThdN, Oversampled, 0.0005, NAN), // 0.000118
            (ThdN, Shipping, 0.0005, NAN), // 8.44e-05
        ],
        ("push_pull_cathode", Tier::T1) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -62.3 / -47.5 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -67.9 / -48.8 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -80.0, NAN), // -82.1
            (Aliasing, Shipping, -118.0, NAN), // -120
            (Anmr, Shipping, -10.0, NAN), // -19.3
            (Bla, Oversampled, 0.01, 0.05), // 0.00183 / 0.0113
            (Bla, Shipping, 0.005, 0.01), // 0.000968 / 0.00252
            (Compression, Oversampled, 0.002, 0.005), // 0.000371 / 0.00119
            (Compression, Shipping, 0.002, 0.002), // 0.000371 / 0.000336
            (Detection, Oversampled, 0.2, NAN), // 0.0421
            (Detection, Shipping, 0.1, NAN), // 0.0177
            (Dynamics, Oversampled, 0.0005, 0.002), // 9.91e-05 / 0.000435
            (Dynamics, Shipping, 0.0002, 0.002), // 5.35e-05 / 0.000508
            (GroupDelay, Oversampled, 0.0002, NAN), // 0
            (GroupDelay, Shipping, 0.0002, NAN), // 0
            (Harmonics, Oversampled, 5.0, 5.0), // 0.843 / 0.696
            (Harmonics, Shipping, 5.0, 5.0), // 0.842 / 0.686
            (Intermodulation, Oversampled, 0.1, 0.2), // 0.0298 / 0.0352
            (Intermodulation, Shipping, 0.1, 0.1), // 0.0174 / 0.0211
            (OperatingPoint, Oversampled, 5e-08, NAN), // 8.16e-09
            (OperatingPoint, Shipping, 5e-08, NAN), // 8.16e-09
            (Recovery, Oversampled, 0.002, NAN), // 0.000519
            (Recovery, Shipping, 0.002, NAN), // 0.000519
            (Response, Oversampled, 1e-09, 1e-09), // 2.91e-10 / 5.04e-31
            (Response, Shipping, 1e-09, 1e-09), // 2.91e-10 / 5.04e-31
            (ThdN, Oversampled, 10.0, NAN), // 1.68
            (ThdN, Shipping, 0.05, NAN), // 0.00942
        ],
        ("grid_blocking", Tier::T0) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -76.1 / -62.2 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -80.6 / -64.1 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -118.0, NAN), // -120
            (Aliasing, Shipping, -118.0, NAN), // -120
            (Anmr, Shipping, -10.0, NAN), // -37.8
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.000992
            (Bla, Shipping, 0.005, 0.002), // 0.000968 / 0.00059
            (Compression, Oversampled, 2e-05, 0.0002), // 3.46e-06 / 5.99e-05
            (Compression, Shipping, 2e-05, 5e-05), // 3.39e-06 / 1.47e-05
            (Detection, Oversampled, 0.005, NAN), // 0.000792
            (Detection, Shipping, 0.002, NAN), // 0.000501
            (Dynamics, Oversampled, 0.02, 0.05), // 0.00603 / 0.0149
            (Dynamics, Shipping, 0.002, 0.002), // 0.000437 / 0.000418
            (GroupDelay, Oversampled, 0.0002, NAN), // 1.45e-14
            (GroupDelay, Shipping, 0.0002, NAN), // 4.22e-11
            (Harmonics, Oversampled, 0.05, 0.05), // 0.0077 / 0.00811
            (Harmonics, Shipping, 0.02, 0.02), // 0.00437 / 0.00353
            (OperatingPoint, Oversampled, 2e-07, NAN), // 4.61e-08
            (OperatingPoint, Shipping, 2e-07, NAN), // 4.61e-08
            (Recovery, Oversampled, 0.0001, NAN), // 2.4e-05
            (Recovery, Shipping, 1e-05, NAN), // 2.29e-06
            (Response, Oversampled, 2e-09, 2e-09), // 6.52e-10 / 4.82e-10
            (Response, Shipping, 1e-07, 0.001), // 2.39e-08 / 0.000247
            (ThdN, Oversampled, 0.0002, NAN), // 3.79e-05
            (ThdN, Shipping, 0.0001, NAN), // 2.99e-05
        ],
        ("grid_blocking", Tier::T1) => &[
            (Audibility, Oversampled, -10.0, -10.0), // -60.0 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Audibility, Shipping, -10.0, -10.0), // -60.0 / -44.4 — inaudible (R16: ≤ −10 dB total and worst band)
            (Aliasing, Oversampled, -65.0, NAN), // -67.3
            (Aliasing, Shipping, -88.0, NAN), // -90.1
            (Anmr, Shipping, -5.0, NAN), // -8.0 — FINDING: audible above −10 dB on the matrix's high tones at 8× (the piano sweep reads −30.1 dB)
            (Bla, Oversampled, 0.005, 0.01), // 0.000969 / 0.00291
            (Bla, Shipping, 0.005, 0.002), // 0.000969 / 0.000491
            (Compression, Oversampled, 0.001, 0.002), // 0.000243 / 0.000605
            (Compression, Shipping, 0.001, 0.0005), // 0.000243 / 0.00016
            (Detection, Oversampled, 0.02, NAN), // 0.0035
            (Detection, Shipping, 0.01, NAN), // 0.00193
            (Dynamics, Oversampled, 0.02, 0.05), // 0.00603 / 0.0149
            (Dynamics, Shipping, 0.01, 0.05), // 0.00315 / 0.008
            (GroupDelay, Oversampled, 0.0002, NAN), // 1.45e-14
            (GroupDelay, Shipping, 0.0002, NAN), // 5e-11
            (Harmonics, Oversampled, 2.0, 2.0), // 0.359 / 0.591
            (Harmonics, Shipping, 2.0, 0.5), // 0.363 / 0.124
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.0616 / 0.0309
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0283 / 0.00357
            (OperatingPoint, Oversampled, 2e-07, NAN), // 4.61e-08
            (OperatingPoint, Shipping, 2e-07, NAN), // 4.61e-08
            (Recovery, Oversampled, 0.0001, NAN), // 2.4e-05
            (Recovery, Shipping, 5e-05, NAN), // 8.52e-06
            (Response, Oversampled, 2e-09, 2e-09), // 6.52e-10 / 4.82e-10
            (Response, Shipping, 1e-07, 0.001), // 2.82e-08 / 0.000293
            (ThdN, Oversampled, 0.01, NAN), // 0.0017
            (ThdN, Shipping, 0.01, NAN), // 0.00169
        ],
        ("triode_stage_cathode/hand", Tier::T0) => &[
            (Audibility, Oversampled, -10.0, 3.6), // -21.1 / -1.4 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Audibility, Shipping, -10.0, 3.6), // -21.1 / -1.4 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Aliasing, Oversampled, -92.0, NAN), // -94.9
            (Aliasing, Shipping, -92.0, NAN), // -94.9
            (Anmr, Shipping, -10.0, NAN), // -24.6
            (Bla, Oversampled, 1.0, 50.0), // 0.307 / 15.2
            (Bla, Shipping, 1.0, 50.0), // 0.307 / 15.2
            (Compression, Oversampled, 0.01, 5.0), // 0.00281 / 0.893
            (Compression, Shipping, 0.01, 5.0), // 0.00281 / 0.893
            (Detection, Oversampled, 50.0, NAN), // 8.8
            (Detection, Shipping, 50.0, NAN), // 8.8
            (Dynamics, Oversampled, 2.0, 0.2), // 0.574 / 0.0511
            (Dynamics, Shipping, 2.0, 0.2), // 0.574 / 0.0511
            (GroupDelay, Oversampled, 0.0002, NAN), // 2.62e-06
            (GroupDelay, Shipping, 0.0002, NAN), // 2.62e-06
            (Harmonics, Oversampled, 200.0, 0.1), // 54 / 0.0193 — FINDING: the table's interpolation floor, H4 near −66 dBc at −20 dB re knee where the circuit has −149
            (Harmonics, Shipping, 200.0, 0.1), // 54 / 0.0193
            (OperatingPoint, Oversampled, 2.0, NAN), // 0.508 — FINDING (R15): the table reads plate volts to ground; the circuit's tube sees them less its cathode, so it rests 0.51 V higher (cathode 1.492 V, not 1.5)
            (OperatingPoint, Shipping, 2.0, NAN), // 0.508
            (Recovery, Oversampled, 0.02, NAN), // 0.00487 — the one-pole cathode recovers within 0.5% of the circuit's
            (Recovery, Shipping, 0.02, NAN), // 0.00487
            (Response, Oversampled, 1.0, 50.0), // 0.304 / 15.2 — FINDING: first-order ADAA's half-sample delay (15° at 16 kHz at 192 kHz) and its roll-off
            (Response, Shipping, 1.0, 50.0), // 0.304 / 15.2
            (ThdN, Oversampled, 5.0, NAN), // 1.15
            (ThdN, Shipping, 5.0, NAN), // 1.15
        ],
        ("triode_stage_cathode/hand", Tier::T1) => &[
            (Audibility, Oversampled, -10.0, 3.7), // -21.1 / -1.4 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Audibility, Shipping, -10.0, 3.7), // -21.0 / -1.3 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Aliasing, Oversampled, -77.0, NAN), // -79.6
            (Aliasing, Shipping, -77.0, NAN), // -79.5
            (Anmr, Shipping, -4.2, NAN), // -7.2
            (Bla, Oversampled, 1.0, 50.0), // 0.307 / 15.2
            (Bla, Shipping, 2.0, 50.0), // 0.366 / 16.6
            (Compression, Oversampled, 0.5, 50.0), // 0.0757 / 7.64
            (Compression, Shipping, 0.5, 50.0), // 0.0901 / 8.32
            (Detection, Oversampled, 100.0, NAN), // 20
            (Detection, Shipping, 100.0, NAN), // 20.1
            (Dynamics, Oversampled, 2.0, 0.2), // 0.574 / 0.0511
            (Dynamics, Shipping, 2.0, 0.2), // 0.574 / 0.0511
            (GroupDelay, Oversampled, 0.0002, NAN), // 2.62e-06
            (GroupDelay, Shipping, 0.0002, NAN), // 2.86e-06
            (Harmonics, Oversampled, 200.0, 10.0), // 55 / 1.87
            (Harmonics, Shipping, 200.0, 10.0), // 55.4 / 2.06
            (Intermodulation, Oversampled, 200.0, 200.0), // 33.4 / 56.3
            (Intermodulation, Shipping, 200.0, 100.0), // 33.4 / 21.5
            (OperatingPoint, Oversampled, 2.0, NAN), // 0.508
            (OperatingPoint, Shipping, 2.0, NAN), // 0.508
            (Recovery, Oversampled, 0.02, NAN), // 0.00487
            (Recovery, Shipping, 0.02, NAN), // 0.00509
            (Response, Oversampled, 1.0, 50.0), // 0.304 / 15.2
            (Response, Shipping, 2.0, 50.0), // 0.362 / 16.6
            (ThdN, Oversampled, 50.0, NAN), // 11.3
            (ThdN, Shipping, 50.0, NAN), // 11.6
        ],
        ("push_pull_cathode/hand", Tier::T0) => &[
            (Audibility, Oversampled, 4.1, 23.8), // -0.9 / 18.8 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Audibility, Shipping, 4.1, 23.8), // -0.9 / 18.8 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Aliasing, Oversampled, -89.0, NAN), // -91.9
            (Aliasing, Shipping, -90.0, NAN), // -92
            (Anmr, Shipping, -10.0, NAN), // -35.9
            (Bla, Oversampled, 2.0, 50.0), // 0.354 / 15.2
            (Bla, Shipping, 2.0, 50.0), // 0.351 / 7.64
            (Compression, Oversampled, 0.5, 5.0), // 0.071 / 0.896
            (Compression, Shipping, 0.5, 2.0), // 0.0727 / 0.451
            (Detection, Oversampled, 200.0, NAN), // 53
            (Detection, Shipping, 200.0, NAN), // 54.1
            (Dynamics, Oversampled, 0.5, 5.0), // 0.0805 / 1.31
            (Dynamics, Shipping, 0.5, 5.0), // 0.0792 / 1.31
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.91e-06
            (GroupDelay, Shipping, 0.0002, NAN), // 2.74e-06
            (Harmonics, Oversampled, 500.0, 10.0), // 74.8 / 3.11 — FINDING: the composed table's interpolation floor, H5 near −45 dBc at −20 dB re knee where the circuit has −169
            (Harmonics, Shipping, 500.0, 10.0), // 74.8 / 3.12
            (OperatingPoint, Oversampled, 0.2, NAN), // 0.0418 — FINDING (R15): the shared cathode rests at 3.458 V in the circuit, 3.5 V in the pair (its RK from plate volts to ground)
            (OperatingPoint, Shipping, 0.2, NAN), // 0.0418
            (Response, Oversampled, 2.0, 50.0), // 0.354 / 15.2 — FINDING: ADAA's half-sample delay and roll-off
            (Response, Shipping, 2.0, 50.0), // 0.351 / 7.62
            (ThdN, Oversampled, 200.0, NAN), // 38.7
            (ThdN, Shipping, 200.0, NAN), // 38.7
        ],
        ("push_pull_cathode/hand", Tier::T1) => &[
            (Audibility, Oversampled, 4.1, 23.8), // -0.9 / 18.8 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Audibility, Shipping, 4.0, 23.7), // -1.0 / 18.7 — FINDING (R16): audible — the table's interpolation floor on the −20 dB tone; held at measurement + 5 dB
            (Aliasing, Oversampled, -57.0, NAN), // -59.2
            (Aliasing, Shipping, -63.0, NAN), // -65.2
            (Anmr, Shipping, -10.0, NAN), // -12.2
            (Bla, Oversampled, 2.0, 50.0), // 0.351 / 15.3
            (Bla, Shipping, 2.0, 50.0), // 0.349 / 8.34
            (Compression, Oversampled, 1.0, 50.0), // 0.328 / 7.64
            (Compression, Shipping, 1.0, 20.0), // 0.307 / 4.16
            (Detection, Oversampled, 200.0, NAN), // 53.8
            (Detection, Shipping, 200.0, NAN), // 56.8
            (Dynamics, Oversampled, 0.5, 5.0), // 0.0805 / 1.31
            (Dynamics, Shipping, 0.5, 5.0), // 0.0793 / 1.31
            (GroupDelay, Oversampled, 0.0002, NAN), // 3.67e-06
            (GroupDelay, Shipping, 0.0002, NAN), // 2.54e-06
            (Harmonics, Oversampled, 500.0, 200.0), // 74.8 / 46.6
            (Harmonics, Shipping, 500.0, 200.0), // 80.9 / 48.1
            (Intermodulation, Oversampled, 100.0, 200.0), // 33.3 / 56.2
            (Intermodulation, Shipping, 200.0, 50.0), // 33.4 / 12.8
            (OperatingPoint, Oversampled, 0.2, NAN), // 0.0418
            (OperatingPoint, Shipping, 0.2, NAN), // 0.0418
            (Recovery, Oversampled, 0.05, NAN), // 0.012
            (Recovery, Shipping, 0.05, NAN), // 0.0162
            (Response, Oversampled, 2.0, 50.0), // 0.351 / 15.2
            (Response, Shipping, 2.0, 50.0), // 0.349 / 8.3
            (ThdN, Oversampled, 500.0, NAN), // 78.5
            (ThdN, Shipping, 500.0, NAN), // 84.8
        ],
        (case, tier) => panic!("no budgets for `{case}` at {tier:?}"),
    }
}
