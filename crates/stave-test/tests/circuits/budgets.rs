//! Every case's budgets, per tier: the measured error with a margin
//! (three times, rounded up to 1-2-5; aliasing ceilings 5 dB above the
//! measured level; group delay at the audibility budget, 0.2 ms from
//! 500 Hz to 4 kHz; ANMR at Kahles et al.'s audibility threshold, −10 dB,
//! where the model is under it, else 5 dB above the measured level,
//! rounded up to 5 — every circuit here runs unoversampled at the host
//! rate, so a hot tone's aliases are heard: the case for the plugin's
//! oversampling or ADAA, recorded, not a budget to chase), as `(check, rates, first, second)` — the second is
//! degrees (dB of probe level for dynamics), `NAN` where the check has
//! none. Measured against the ngspice 42 tables; rows marked "R11" were
//! re-measured when the transistor decks gained their cards' charge (their
//! `*_static` twins keep the static rows). A row that holds a finding
//! says so beside it: its budget is the measured value with the same
//! margin, so the finding cannot get worse unnoticed; DSP.md R17 lists
//! them. Errors at shipping rates are the model run unoversampled at 44.1,
//! 48 and 96 kHz: what a plugin that ships it that way would have.

use stave_test::characterise::Check::{self, *};
use stave_test::characterise::Rates::{self, *};
use stave_test::characterise::Tier;

const NAN: f64 = f64::NAN;

pub type Row = (Check, Rates, f64, f64);

pub fn table(case: &str, tier: Tier) -> &'static [Row] {
    match (case, tier) {
        ("diode_clipper", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -65.0, NAN), // -73.8 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -34.6 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.491
            (Bla, Shipping, 20.0, 50.0), // 4.07 / 9.65 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 5e-4, 0.02), // 0.000109 / 0.0054
            (Compression, Shipping, 0.005, 0.1), // 0.00153 / 0.0301
            (Detection, Oversampled, 0.05, NAN), // 0.0108
            (Detection, Shipping, 1.0, NAN), // 0.186
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.73e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 6.09e-07
            (Harmonics, Oversampled, 0.2, 2.0), // 0.0663 / 0.402
            (Harmonics, Shipping, 5.0, 50.0), // 1.61 / 6.84
            (OperatingPoint, Oversampled, 1e-9, NAN), // 4.34e-22
            (OperatingPoint, Shipping, 1e-9, NAN), // 4.34e-22
            (Response, Oversampled, 0.02, 0.1), // 0.0058 / 0.0228 — FINDING: the deck's 4 pF Cjo, which DK leaves out (R11)
            (Response, Shipping, 20.0, 50.0), // 4.07 / 9.25 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.005, NAN), // 0.000825
            (ThdN, Shipping, 0.1, NAN), // 0.0211 — aliasing at the host rate
        ],
        ("ce_stage", Tier::T0) => &[
            (Aliasing, Oversampled, -95.0, NAN), // -101
            (Aliasing, Shipping, -60.0, NAN), // -65.5 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -13.2 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.1), // 0.00143 / 0.0271
            (Bla, Shipping, 0.5, 2.0), // 0.0977 / 0.458
            (Compression, Oversampled, 2e-5, 0.001), // 4.34e-06 / 0.000254
            (Compression, Shipping, 1e-4, 0.02), // 2.48e-05 / 0.00389
            (Detection, Oversampled, 50.0, NAN), // 9.28
            (Detection, Shipping, 50.0, NAN), // 9.27
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.98e-10
            (GroupDelay, Shipping, 2e-4, NAN), // 1.15e-08
            (Harmonics, Oversampled, 0.05, 0.05), // 0.0135 / 0.0118
            (Harmonics, Shipping, 0.5, 0.2), // 0.0797 / 0.0573
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.33e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.33e-05
            (Response, Oversampled, 5e-4, 0.002), // 0.00015 / 0.000361
            (Response, Shipping, 0.002, 2.0), // 0.00064 / 0.389 (R11: the whole card)
            (ThdN, Oversampled, 2e-4, NAN), // 6.06e-05
            (ThdN, Shipping, 0.002, NAN), // 0.000505 — aliasing at the host rate
        ],
        ("ce_stage_static", Tier::T0) => &[
            (Aliasing, Oversampled, -95.0, NAN), // -101
            (Aliasing, Shipping, -60.0, NAN), // -65.5 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -13.1 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.1), // 0.00143 / 0.0271
            (Bla, Shipping, 0.5, 2.0), // 0.0977 / 0.458
            (Compression, Oversampled, 2e-5, 0.001), // 4.34e-06 / 0.000254
            (Compression, Shipping, 1e-4, 0.02), // 2.48e-05 / 0.00389
            (Detection, Oversampled, 50.0, NAN), // 9.28
            (Detection, Shipping, 50.0, NAN), // 9.27
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.98e-10
            (GroupDelay, Shipping, 2e-4, NAN), // 1.15e-08
            (Harmonics, Oversampled, 0.05, 0.05), // 0.0135 / 0.0118
            (Harmonics, Shipping, 0.5, 0.2), // 0.0797 / 0.0573
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.33e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.33e-05
            (Response, Oversampled, 5e-4, 0.002), // 0.00015 / 0.000361
            (Response, Shipping, 5e-4, 0.5), // 0.000131 / 0.0723
            (ThdN, Oversampled, 2e-4, NAN), // 6.06e-05
            (ThdN, Shipping, 0.002, NAN), // 0.000505 — aliasing at the host rate
        ],
        ("common_cathode", Tier::T0) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -112
            (Aliasing, Shipping, -95.0, NAN), // -104 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -25.5 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.002), // 0.000969 / 0.000373
            (Bla, Shipping, 0.005, 0.02), // 0.00097 / 0.00649
            (Compression, Oversampled, 1e-5, 1e-4), // 3.03e-06 / 2.14e-05
            (Compression, Shipping, 2e-5, 0.002), // 3.76e-06 / 0.000348
            (Detection, Oversampled, 0.02, NAN), // 0.00421
            (Detection, Shipping, 0.05, NAN), // 0.0119
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.51e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.03e-09
            (Harmonics, Oversampled, 1.0, 0.02), // 0.239 / 0.00588
            (Harmonics, Shipping, 5.0, 0.1), // 1.13 / 0.0245
            (OperatingPoint, Oversampled, 1e-5, NAN), // 2.51e-06
            (OperatingPoint, Shipping, 1e-5, NAN), // 2.51e-06
            (Response, Oversampled, 1e-6, 1e-5), // 1.26e-07 / 1.56e-07
            (Response, Shipping, 2e-6, 0.02), // 6.61e-07 / 0.00648
            (ThdN, Oversampled, 2e-4, NAN), // 3.98e-05
            (ThdN, Shipping, 5e-4, NAN), // 9.32e-05 — aliasing at the host rate
        ],
        ("fuzz_face", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -37.0 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.5, 1.0), // 0.121 / 0.227
            (Bla, Shipping, 20.0, 50.0), // 4.27 / 13 (R11: the whole card)
            (Compression, Oversampled, 0.5, 0.05), // 0.12 / 0.0162 — FINDING: +0.12 dB of gain from the operating point
            (Compression, Shipping, 0.5, 0.05), // 0.12 / 0.0104
            (Detection, Oversampled, 1.0, NAN), // 0.324
            (Detection, Shipping, 1.0, NAN), // 0.332
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.53e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 1.71e-07
            (Harmonics, Oversampled, 2.0, 0.5), // 0.587 / 0.133 (R11: the whole card)
            (Harmonics, Shipping, 2.0, 1.0), // 0.423 / 0.231 (R11: the whole card)
            (OperatingPoint, Oversampled, 0.05, NAN), // 0.00936 — FINDING: 9.4 mV off ngspice at Q2's collector (open)
            (OperatingPoint, Shipping, 0.05, NAN), // 0.00936
            (Response, Oversampled, 0.5, 1.0), // 0.12 / 0.226 — FINDING: +0.12 dB of gain from the operating point
            (Response, Shipping, 20.0, 50.0), // 4.27 / 13 (R11: the whole card)
            (ThdN, Oversampled, 0.5, NAN), // 0.129
            (ThdN, Shipping, 0.5, NAN), // 0.129 — aliasing at the host rate
        ],
        ("fuzz_face_static", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -94.2 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.5, 1.0), // 0.121 / 0.227
            (Bla, Shipping, 0.5, 1.0), // 0.121 / 0.227
            (Compression, Oversampled, 0.5, 0.05), // 0.12 / 0.0162 — FINDING: +0.12 dB of gain from the operating point
            (Compression, Shipping, 0.5, 0.05), // 0.12 / 0.0104
            (Detection, Oversampled, 1.0, NAN), // 0.324
            (Detection, Shipping, 1.0, NAN), // 0.332
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.53e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 1.71e-07
            (Harmonics, Oversampled, 2.0, 0.1), // 0.645 / 0.0172
            (Harmonics, Shipping, 2.0, 0.05), // 0.645 / 0.0132
            (OperatingPoint, Oversampled, 0.05, NAN), // 0.00936 — FINDING: 9.4 mV off ngspice at Q2's collector (open)
            (OperatingPoint, Shipping, 0.05, NAN), // 0.00936
            (Response, Oversampled, 0.5, 1.0), // 0.12 / 0.226 — FINDING: +0.12 dB of gain from the operating point
            (Response, Shipping, 0.5, 1.0), // 0.12 / 0.226
            (ThdN, Oversampled, 0.5, NAN), // 0.129
            (ThdN, Shipping, 0.5, NAN), // 0.129 — aliasing at the host rate
        ],
        ("common_cathode/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -113
            (Aliasing, Shipping, -95.0, NAN), // -104 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -25.5 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.002), // 0.000995 / 0.000462
            (Bla, Shipping, 0.005, 0.05), // 0.0011 / 0.00669
            (Compression, Oversampled, 1e-5, 1e-4), // 2.99e-06 / 2.88e-05
            (Compression, Shipping, 2e-5, 0.002), // 6.25e-06 / 0.000343
            (Detection, Oversampled, 1.0, NAN), // 0.303
            (Detection, Shipping, 2.0, NAN), // 0.361
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.46e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 5.58e-08
            (Harmonics, Oversampled, 2.0, 0.05), // 0.38 / 0.0104
            (Harmonics, Shipping, 5.0, 0.1), // 1.06 / 0.0232
            (OperatingPoint, Oversampled, 1e-5, NAN), // 2.51e-06
            (OperatingPoint, Shipping, 1e-5, NAN), // 2.51e-06
            (Response, Oversampled, 0.01, 0.01), // 0.0022 / 0.00309
            (Response, Shipping, 0.01, 0.05), // 0.00224 / 0.0067
            (ThdN, Oversampled, 0.001, NAN), // 0.00024
            (ThdN, Shipping, 0.001, NAN), // 0.000178 — aliasing at the host rate
        ],
        ("common_cathode_grid_current/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -110
            (Aliasing, Shipping, -95.0, NAN), // -103 — aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.002), // 0.000981 / 0.000488
            (Bla, Shipping, 0.1, 0.5), // 0.0195 / 0.123
            (Compression, Oversampled, 2e-5, 1e-4), // 3.46e-06 / 2.36e-05
            (Compression, Shipping, 2e-5, 0.002), // 4.21e-06 / 0.000389
            (Detection, Oversampled, 0.5, NAN), // 0.141
            (Detection, Shipping, 0.2, NAN), // 0.0431
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.54e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 2.07e-08
            (Harmonics, Oversampled, 0.1, 0.2), // 0.0283 / 0.0554
            (Harmonics, Shipping, 0.5, 1.0), // 0.144 / 0.233
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 0.01, 0.01), // 0.00195 / 0.00274
            (Response, Shipping, 0.01, 0.05), // 0.00194 / 0.00752
            (ThdN, Oversampled, 2e-4, NAN), // 5.29e-05
            (ThdN, Shipping, 0.001, NAN), // 0.000271 — aliasing at the host rate
        ],
        ("common_cathode_grid_current", Tier::T0) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -111
            (Aliasing, Shipping, -95.0, NAN), // -103 — aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000411
            (Bla, Shipping, 0.1, 0.5), // 0.0195 / 0.123
            (Compression, Oversampled, 2e-5, 1e-4), // 3.49e-06 / 2.36e-05
            (Compression, Shipping, 2e-5, 0.002), // 4.31e-06 / 0.000384
            (Detection, Oversampled, 0.005, NAN), // 0.00125
            (Detection, Shipping, 0.2, NAN), // 0.0431
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.28e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 1.13e-09
            (Harmonics, Oversampled, 0.1, 0.2), // 0.0279 / 0.0536
            (Harmonics, Shipping, 0.5, 1.0), // 0.111 / 0.232
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 1e-6, 1e-5), // 6.39e-10 / 1.41e-09
            (Response, Shipping, 5e-6, 0.05), // 8.96e-07 / 0.00714
            (ThdN, Oversampled, 2e-4, NAN), // 5.3e-05
            (ThdN, Shipping, 5e-4, NAN), // 0.000103 — aliasing at the host rate
        ],
        ("diode_clipper/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -65.0, NAN), // -73.8 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -34.6 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.49
            (Bla, Shipping, 20.0, 50.0), // 4.07 / 9.65 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.01, 0.02), // 0.00257 / 0.00541
            (Compression, Shipping, 0.02, 0.1), // 0.00403 / 0.031
            (Detection, Oversampled, 0.05, NAN), // 0.0161
            (Detection, Shipping, 1.0, NAN), // 0.191
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.69e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 6.08e-07
            (Harmonics, Oversampled, 0.5, 2.0), // 0.0757 / 0.376
            (Harmonics, Shipping, 5.0, 50.0), // 1.61 / 6.8
            (OperatingPoint, Oversampled, 1e-9, NAN), // 4.34e-22
            (OperatingPoint, Shipping, 1e-9, NAN), // 4.34e-22
            (Response, Oversampled, 1.0, 2.0), // 0.169 / 0.489
            (Response, Shipping, 20.0, 50.0), // 4.07 / 9.25 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.02, NAN), // 0.00402
            (ThdN, Shipping, 0.05, NAN), // 0.0163 — aliasing at the host rate
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
            (Aliasing, Oversampled, -95.0, NAN), // -100
            (Aliasing, Shipping, -85.0, NAN), // -91.2 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -10.6 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.02), // 0.00113 / 0.00494
            (Bla, Shipping, 0.1, 0.5), // 0.0213 / 0.144
            (Compression, Oversampled, 5e-5, 2e-4), // 1.22e-05 / 3.99e-05
            (Compression, Shipping, 5e-5, 0.002), // 1.15e-05 / 0.000622
            (Detection, Oversampled, 0.05, NAN), // 0.00778
            (Detection, Shipping, 0.5, NAN), // 0.141
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.77e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 1.83e-09
            (Harmonics, Oversampled, 1.0, 0.1), // 0.245 / 0.0227
            (Harmonics, Shipping, 5.0, 0.5), // 1.28 / 0.095
            (OperatingPoint, Oversampled, 1e-4, NAN), // 3.21e-05 — FINDING: ngspice's gate sits 4.3 µV up (4.3 pA in 1 MΩ; not gmin)
            (OperatingPoint, Shipping, 1e-4, NAN), // 3.21e-05
            (Response, Oversampled, 5e-5, 5e-4), // 1.45e-05 / 7.98e-05
            (Response, Shipping, 5e-5, 0.05), // 1.37e-05 / 0.0116
            (ThdN, Oversampled, 5e-6, NAN), // 1.25e-06
            (ThdN, Shipping, 0.001, NAN), // 0.000299 — aliasing at the host rate
        ],
        ("jfet_booster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -95.0, NAN), // -100
            (Aliasing, Shipping, -85.0, NAN), // -91.2 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -10.6 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.02), // 0.00113 / 0.00494
            (Bla, Shipping, 0.1, 0.5), // 0.0213 / 0.144
            (Compression, Oversampled, 5e-5, 2e-4), // 1.22e-05 / 3.99e-05
            (Compression, Shipping, 5e-5, 0.002), // 1.15e-05 / 0.000622
            (Detection, Oversampled, 0.05, NAN), // 0.00778
            (Detection, Shipping, 0.5, NAN), // 0.141
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.77e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 1.83e-09
            (Harmonics, Oversampled, 1.0, 0.1), // 0.245 / 0.0227
            (Harmonics, Shipping, 5.0, 0.5), // 1.28 / 0.095
            (OperatingPoint, Oversampled, 1e-4, NAN), // 3.21e-05 — FINDING: ngspice's gate sits 4.3 µV up (4.3 pA in 1 MΩ; not gmin)
            (OperatingPoint, Shipping, 1e-4, NAN), // 3.21e-05
            (Response, Oversampled, 5e-5, 5e-4), // 1.45e-05 / 7.98e-05
            (Response, Shipping, 5e-5, 0.05), // 1.37e-05 / 0.0116
            (ThdN, Oversampled, 5e-6, NAN), // 1.25e-06
            (ThdN, Shipping, 0.001, NAN), // 0.000299 — aliasing at the host rate
        ],
        ("opamp_clipper/hand", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -90.0, NAN), // -97.5 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -55.0 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.001, 0.1), // 0.000317 / 0.021
            (Bla, Shipping, 2.0, 50.0), // 0.489 / 9.37 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 1e-4, 5e-4), // 1.9e-05 / 0.000118
            (Compression, Shipping, 0.02, 0.1), // 0.00404 / 0.0327
            (Detection, Oversampled, 5e-4, NAN), // 0.000107
            (Detection, Shipping, 0.5, NAN), // 0.105
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.68e-09
            (GroupDelay, Shipping, 2e-4, NAN), // 2.45e-07
            (Harmonics, Oversampled, 0.5, 0.05), // 0.0717 / 0.0106
            (Harmonics, Shipping, 0.5, 20.0), // 0.147 / 4.12
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 0.005, 0.1), // 0.000826 / 0.0224
            (Response, Shipping, 2.0, 50.0), // 0.49 / 9.37 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.002, NAN), // 0.000612
            (ThdN, Shipping, 0.05, NAN), // 0.0139 — aliasing at the host rate
        ],
        ("mosfet_booster", Tier::T0) => &[
            (Aliasing, Oversampled, -95.0, NAN), // -104
            (Aliasing, Shipping, -40.0, NAN), // -46.1 — aliasing at the host rate
            (Anmr, Shipping, 15.0, NAN), // 9.1 — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.05, 0.2), // 0.00683 / 0.0433
            (Bla, Shipping, 1.0, 5.0), // 0.205 / 1.13
            (Compression, Oversampled, 2e-5, 0.001), // 3.34e-06 / 0.000228
            (Compression, Shipping, 5e-4, 0.005), // 7.57e-05 / 0.00163 (R11: the whole card)
            (Detection, Oversampled, 1.0, NAN), // 0.224
            (Detection, Shipping, 2.0, NAN), // 0.414
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.05e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.46e-09
            (Harmonics, Oversampled, 0.2, 0.5), // 0.0406 / 0.148
            (Harmonics, Shipping, 1.0, 10.0), // 0.193 / 2.7 (R11: the whole card)
            (OperatingPoint, Oversampled, 2e-5, NAN), // 6.53e-06
            (OperatingPoint, Shipping, 2e-5, NAN), // 6.53e-06
            (Response, Oversampled, 2e-5, 1e-5), // 6.15e-06 / 1.03e-08
            (Response, Shipping, 0.01, 2.0), // 0.00262 / 0.423 (R11: the whole card)
            (ThdN, Oversampled, 0.01, NAN), // 0.00197
            (ThdN, Shipping, 0.02, NAN), // 0.00408 — aliasing at the host rate
        ],
        ("mosfet_booster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -95.0, NAN), // -104
            (Aliasing, Shipping, -40.0, NAN), // -46.1 — aliasing at the host rate
            (Anmr, Shipping, 15.0, NAN), // 9.1 — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.05, 0.2), // 0.00683 / 0.0433
            (Bla, Shipping, 1.0, 5.0), // 0.205 / 1.13
            (Compression, Oversampled, 2e-5, 0.001), // 3.34e-06 / 0.000228
            (Compression, Shipping, 5e-5, 0.005), // 8.18e-06 / 0.00129
            (Detection, Oversampled, 1.0, NAN), // 0.224
            (Detection, Shipping, 2.0, NAN), // 0.414
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.05e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.46e-09
            (Harmonics, Oversampled, 0.2, 0.5), // 0.0406 / 0.148
            (Harmonics, Shipping, 0.2, 2.0), // 0.0431 / 0.546
            (OperatingPoint, Oversampled, 2e-5, NAN), // 6.53e-06
            (OperatingPoint, Shipping, 2e-5, NAN), // 6.53e-06
            (Response, Oversampled, 2e-5, 1e-5), // 6.15e-06 / 1.03e-08
            (Response, Shipping, 2e-5, 0.05), // 5.86e-06 / 0.00918
            (ThdN, Oversampled, 0.01, NAN), // 0.00197
            (ThdN, Shipping, 0.02, NAN), // 0.00408 — aliasing at the host rate
        ],
        ("ota_vca", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -121.3 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.00105
            (Bla, Shipping, 0.2, 1.0), // 0.053 / 0.177
            (Compression, Oversampled, 2e-5, 1e-5), // 5.2e-06 / 1.36e-07
            (Compression, Shipping, 2e-5, 1e-5), // 5.2e-06 / 1.36e-07
            (Detection, Oversampled, 0.01, NAN), // 0.00257
            (Detection, Shipping, 0.2, NAN), // 0.0404
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.01, 5e-4), // 0.00206 / 8.53e-05
            (Harmonics, Shipping, 0.01, 5e-4), // 0.00206 / 8.53e-05
            (OperatingPoint, Oversampled, 5e-5, NAN), // 7.25e-06
            (OperatingPoint, Shipping, 5e-5, NAN), // 7.25e-06
            (Response, Oversampled, 0.005, NAN), // 0.00122
            (Response, Shipping, 0.005, NAN), // 0.00122
            (ThdN, Oversampled, 5e-4, NAN), // 8.6e-05
            (ThdN, Shipping, 5e-4, NAN), // 8.6e-05 — aliasing at the host rate
        ],
        ("opamp_clipper", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -90.0, NAN), // -97.5 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -55.0 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.001, 0.1), // 0.000317 / 0.021
            (Bla, Shipping, 2.0, 50.0), // 0.489 / 9.37 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 1e-4, 5e-4), // 1.9e-05 / 0.000118
            (Compression, Shipping, 0.02, 0.1), // 0.00404 / 0.0327
            (Detection, Oversampled, 5e-4, NAN), // 0.000107
            (Detection, Shipping, 0.5, NAN), // 0.105
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.43e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 2.45e-07
            (Harmonics, Oversampled, 0.5, 0.05), // 0.0717 / 0.0106
            (Harmonics, Shipping, 0.5, 20.0), // 0.147 / 4.12
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 5e-6, 1e-5), // 8.22e-07 / 2.39e-06
            (Response, Shipping, 2.0, 50.0), // 0.49 / 9.37 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.002, NAN), // 0.000612
            (ThdN, Shipping, 0.05, NAN), // 0.0139 — aliasing at the host rate
        ],
        ("power_supply", Tier::T0) => &[
            (Dynamics, Oversampled, 5e-4, 0.02), // 0.000113 / 0.00488
            (Dynamics, Shipping, 0.01, 0.1), // 0.00236 / 0.0319
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0507 / 1.28
            (OperatingPoint, Oversampled, 1e-4, NAN), // 2.45e-05
            (OperatingPoint, Shipping, 0.005, NAN), // 0.000679
        ],
        ("push_pull", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -118.9 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.2, 2.0), // 0.0517 / 0.367
            (Bla, Shipping, 0.2, 2.0), // 0.0517 / 0.367
            (Compression, Oversampled, 5e-5, 1e-5), // 8.39e-06 / 3.25e-07
            (Compression, Shipping, 5e-5, 1e-5), // 8.39e-06 / 3.25e-07
            (Detection, Oversampled, 0.2, NAN), // 0.0642
            (Detection, Shipping, 0.2, NAN), // 0.0642
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.01, 0.001), // 0.00286 / 0.000208
            (Harmonics, Shipping, 0.01, 0.001), // 0.00286 / 0.000208
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.5e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.5e-09
            (Response, Oversampled, 1e-6, NAN), // 4.68e-10
            (Response, Shipping, 1e-6, NAN), // 4.68e-10
            (ThdN, Oversampled, 5e-4, NAN), // 0.000101
            (ThdN, Shipping, 5e-4, NAN), // 0.000101 — aliasing at the host rate
        ],
        ("push_pull_pentode", Tier::T0) => &[
            (Aliasing, Oversampled, -85.0, NAN), // -91.4
            (Aliasing, Shipping, -85.0, NAN), // -91.4 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -27.4 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.2, 0.5), // 0.0426 / 0.149
            (Bla, Shipping, 0.2, 0.5), // 0.0426 / 0.149
            (Compression, Oversampled, 5e-5, 1e-5), // 9.12e-06 / 4.32e-08
            (Compression, Shipping, 5e-5, 1e-5), // 9.12e-06 / 4.32e-08
            (Detection, Oversampled, 0.2, NAN), // 0.0376
            (Detection, Shipping, 0.2, NAN), // 0.0376
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.05, 2e-4), // 0.00953 / 4.61e-05
            (Harmonics, Shipping, 0.05, 2e-4), // 0.00953 / 4.61e-05
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.08e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.08e-05
            (Response, Oversampled, 1e-6, NAN), // 5.65e-09
            (Response, Shipping, 1e-6, NAN), // 5.65e-09
            (ThdN, Oversampled, 5e-4, NAN), // 9.66e-05
            (ThdN, Shipping, 5e-4, NAN), // 9.66e-05 — aliasing at the host rate
        ],
        ("triode_stage", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -121.2 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.02, 0.1), // 0.0057 / 0.0244
            (Bla, Shipping, 0.02, 0.1), // 0.0057 / 0.0244
            (Compression, Oversampled, 2e-5, 1e-5), // 3.34e-06 / 2.54e-08
            (Compression, Shipping, 2e-5, 1e-5), // 3.34e-06 / 2.54e-08
            (Detection, Oversampled, 0.1, NAN), // 0.018
            (Detection, Shipping, 0.1, NAN), // 0.018
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.002, 1e-5), // 0.000635 / 1.46e-06
            (Harmonics, Shipping, 0.002, 1e-5), // 0.000635 / 1.46e-06
            (OperatingPoint, Oversampled, 1e-4, NAN), // 2.11e-05
            (OperatingPoint, Shipping, 1e-4, NAN), // 2.11e-05
            (Response, Oversampled, 5e-6, NAN), // 7.23e-07
            (Response, Shipping, 5e-6, NAN), // 7.23e-07
            (ThdN, Oversampled, 1e-4, NAN), // 2.29e-05
            (ThdN, Shipping, 1e-4, NAN), // 2.29e-05 — aliasing at the host rate
        ],
        ("rangemaster", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -52.1 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.5, 1.0), // 0.137 / 0.325
            (Bla, Shipping, 1.0, 20.0), // 0.189 / 3.37
            (Compression, Oversampled, 0.2, 1.0), // 0.0495 / 0.249
            (Compression, Shipping, 0.2, 1.0), // 0.0585 / 0.212
            (Detection, Oversampled, 1.0, NAN), // 0.215
            (Detection, Shipping, 1.0, NAN), // 0.231
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.08e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 6.65e-07
            (Harmonics, Oversampled, 2.0, 5.0), // 0.394 / 1.35
            (Harmonics, Shipping, 2.0, 2.0), // 0.354 / 0.549
            (OperatingPoint, Oversampled, 0.02, NAN), // 0.0039 — FINDING: 3.9 mV off ngspice at the collector (open)
            (OperatingPoint, Shipping, 0.02, NAN), // 0.0039
            (Response, Oversampled, 0.5, 2.0), // 0.133 / 0.337 — FINDING: 0.13 dB of gain from the operating point
            (Response, Shipping, 1.0, 10.0), // 0.184 / 3.22
            (ThdN, Oversampled, 0.1, NAN), // 0.0272
            (ThdN, Shipping, 0.05, NAN), // 0.00715 — aliasing at the host rate
        ],
        ("rangemaster_static", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -52.9 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.5, 1.0), // 0.137 / 0.325
            (Bla, Shipping, 1.0, 20.0), // 0.189 / 3.37
            (Compression, Oversampled, 0.2, 1.0), // 0.0495 / 0.249
            (Compression, Shipping, 0.2, 1.0), // 0.0585 / 0.212
            (Detection, Oversampled, 1.0, NAN), // 0.215
            (Detection, Shipping, 1.0, NAN), // 0.231
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.08e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 6.65e-07
            (Harmonics, Oversampled, 2.0, 5.0), // 0.394 / 1.35
            (Harmonics, Shipping, 2.0, 2.0), // 0.354 / 0.549
            (OperatingPoint, Oversampled, 0.02, NAN), // 0.0039 — FINDING: 3.9 mV off ngspice at the collector (open)
            (OperatingPoint, Shipping, 0.02, NAN), // 0.0039
            (Response, Oversampled, 0.5, 2.0), // 0.133 / 0.337 — FINDING: 0.13 dB of gain from the operating point
            (Response, Shipping, 1.0, 10.0), // 0.184 / 3.22
            (ThdN, Oversampled, 0.1, NAN), // 0.0272
            (ThdN, Shipping, 0.05, NAN), // 0.00715 — aliasing at the host rate
        ],
        ("tube_screamer", Tier::T0) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -115.0, NAN), // -120 — aliasing at the host rate
            (Anmr, Shipping, -10.0, NAN), // -103.7 — inaudible (Kahles et al.'s −10 dB)
            (Bla, Oversampled, 0.2, 0.5), // 0.0455 / 0.109
            (Bla, Shipping, 20.0, 50.0), // 4.33 / 8.4 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 5e-4, 0.005), // 7.09e-05 / 0.000779
            (Compression, Shipping, 0.02, 0.2), // 0.00383 / 0.0519
            (Detection, Oversampled, 0.01, NAN), // 0.00273
            (Detection, Shipping, 1.0, NAN), // 0.203
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.74e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 8.9e-07
            (Harmonics, Oversampled, 0.1, 1.0), // 0.0207 / 0.231
            (Harmonics, Shipping, 10.0, 100.0), // 2.53 / 17.4
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.05e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.05e-08
            (Response, Oversampled, 5e-5, 1e-4), // 9.11e-06 / 3.1e-05
            (Response, Shipping, 20.0, 50.0), // 4.33 / 6.8 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.001, NAN), // 0.000274
            (ThdN, Shipping, 0.1, NAN), // 0.0189 — aliasing at the host rate
        ],
        ("diode_clipper", Tier::T1) => &[
            (Aliasing, Oversampled, -55.0, NAN), // -62
            (Aliasing, Shipping, -5.0, NAN), // -11.9 — aliasing at the host rate
            (Anmr, Shipping, 75.0, NAN), // 68.3 / 57.6 / 28.6 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.493
            (Bla, Shipping, 20.0, 50.0), // 5.33 / 11.8 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.1, 0.5), // 0.0247 / 0.16
            (Compression, Shipping, 5.0, 100.0), // 0.703 / 19.5
            (Detection, Oversampled, 0.1, NAN), // 0.0191
            (Detection, Shipping, 500.0, NAN), // 89.1
            (Dynamics, Oversampled, 5e-5, 0.05), // 1.15e-05 / 0.00843
            (Dynamics, Shipping, 5e-4, 0.5), // 9.66e-05 / 0.101
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.73e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 7.26e-07
            (Harmonics, Oversampled, 100.0, 200.0), // 17.6 / 45.8
            (Harmonics, Shipping, 100.0, 500.0), // 21.7 / 77.3
            (Intermodulation, Oversampled, 20.0, 20.0), // 5.19 / 5.1
            (Intermodulation, Shipping, 200.0, 200.0), // 50.9 / 57.2
            (OperatingPoint, Oversampled, 1e-9, NAN), // 4.34e-22
            (OperatingPoint, Shipping, 1e-9, NAN), // 4.34e-22
            (Response, Oversampled, 0.02, 0.1), // 0.0058 / 0.0228 — FINDING: the deck's 4 pF Cjo, which DK leaves out (R11)
            (Response, Shipping, 20.0, 50.0), // 5.33 / 11.3 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 200.0, NAN), // 48.8
            (ThdN, Shipping, 500.0, NAN), // 98.9 — aliasing at the host rate
        ],
        ("ce_stage", Tier::T1) => &[
            (Aliasing, Oversampled, -40.0, NAN), // -48.6
            (Aliasing, Shipping, -5.0, NAN), // -11.9 — aliasing at the host rate
            (Anmr, Shipping, 65.0, NAN), // 58.8 / 47.9 / 37.8 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.02, 0.05), // 0.00389 / 0.0141
            (Bla, Shipping, 1.0, 5.0), // 0.224 / 0.691
            (Compression, Oversampled, 0.001, 0.01), // 0.000244 / 0.0022
            (Compression, Shipping, 0.02, 5.0), // 0.00606 / 0.683 (R11: the whole card)
            (Detection, Oversampled, 50.0, NAN), // 13.8
            (Detection, Shipping, 50.0, NAN), // 13.8
            (Dynamics, Oversampled, 5e-4, 0.02), // 8.79e-05 / 0.00601
            (Dynamics, Shipping, 0.002, 0.2), // 0.000585 / 0.0501
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.98e-10
            (GroupDelay, Shipping, 2e-4, NAN), // 1.36e-08
            (Harmonics, Oversampled, 1.0, 10.0), // 0.17 / 2.04
            (Harmonics, Shipping, 50.0, 50.0), // 7.37 / 10.2 (R11: the whole card)
            (Intermodulation, Oversampled, 0.2, 2.0), // 0.0265 / 0.345 (R11: the whole card)
            (Intermodulation, Shipping, 2.0, 50.0), // 0.382 / 9.29 (R11: the whole card)
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.33e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.33e-05
            (Response, Oversampled, 5e-4, 0.002), // 0.00015 / 0.000361
            (Response, Shipping, 0.005, 2.0), // 0.000916 / 0.532 (R11: the whole card)
            (ThdN, Oversampled, 0.1, NAN), // 0.0277
            (ThdN, Shipping, 20.0, NAN), // 5.27 — aliasing at the host rate
        ],
        ("ce_stage_static", Tier::T1) => &[
            (Aliasing, Oversampled, -40.0, NAN), // -48.6
            (Aliasing, Shipping, -5.0, NAN), // -11.9 — aliasing at the host rate
            (Anmr, Shipping, 65.0, NAN), // 58.8 / 47.7 / 37.8 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.02, 0.05), // 0.00389 / 0.0141
            (Bla, Shipping, 1.0, 5.0), // 0.224 / 0.691
            (Compression, Oversampled, 0.001, 0.01), // 0.000244 / 0.0022
            (Compression, Shipping, 0.002, 0.2), // 0.000361 / 0.0559
            (Detection, Oversampled, 50.0, NAN), // 13.8
            (Detection, Shipping, 50.0, NAN), // 13.8
            (Dynamics, Oversampled, 5e-4, 0.02), // 8.79e-05 / 0.00601
            (Dynamics, Shipping, 0.002, 0.2), // 0.000585 / 0.0501
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.98e-10
            (GroupDelay, Shipping, 2e-4, NAN), // 1.36e-08
            (Harmonics, Oversampled, 1.0, 10.0), // 0.17 / 2.04
            (Harmonics, Shipping, 5.0, 50.0), // 1.01 / 10.5
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.0512 / 0.03
            (Intermodulation, Shipping, 1.0, 0.5), // 0.239 / 0.139
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.33e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.33e-05
            (Response, Oversampled, 5e-4, 0.002), // 0.00015 / 0.000361
            (Response, Shipping, 5e-4, 0.5), // 0.000145 / 0.0871
            (ThdN, Oversampled, 0.1, NAN), // 0.0277
            (ThdN, Shipping, 20.0, NAN), // 5.27 — aliasing at the host rate
        ],
        ("common_cathode", Tier::T1) => &[
            (Aliasing, Oversampled, -90.0, NAN), // -96.5
            (Aliasing, Shipping, -25.0, NAN), // -32.7 — aliasing at the host rate
            (Anmr, Shipping, 20.0, NAN), // 12.7 / 3.2 / -3.5 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.002), // 0.000968 / 0.000382
            (Bla, Shipping, 0.05, 0.2), // 0.012 / 0.0553
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000192
            (Compression, Shipping, 0.001, 0.02), // 0.000244 / 0.00363
            (Detection, Oversampled, 0.05, NAN), // 0.0125
            (Detection, Shipping, 0.2, NAN), // 0.0617
            (Dynamics, Oversampled, 0.001, 5e-4), // 0.000319 / 9.51e-05
            (Dynamics, Shipping, 0.01, 0.005), // 0.00181 / 0.00154
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.51e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.22e-09
            (Harmonics, Oversampled, 2.0, 0.2), // 0.598 / 0.0663
            (Harmonics, Shipping, 10.0, 5.0), // 2.07 / 0.738
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0493 / 0.016
            (Intermodulation, Shipping, 0.5, 0.05), // 0.0828 / 0.0126
            (OperatingPoint, Oversampled, 1e-5, NAN), // 2.51e-06
            (OperatingPoint, Shipping, 1e-5, NAN), // 2.51e-06
            (Response, Oversampled, 1e-6, 1e-5), // 1.26e-07 / 1.56e-07
            (Response, Shipping, 5e-6, 0.05), // 8.06e-07 / 0.0078
            (ThdN, Oversampled, 0.01, NAN), // 0.0017
            (ThdN, Shipping, 0.5, NAN), // 0.163 — aliasing at the host rate
        ],
        ("common_cathode/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -85.0, NAN), // -92.6
            (Aliasing, Shipping, -25.0, NAN), // -32.7 — aliasing at the host rate
            (Anmr, Shipping, 20.0, NAN), // 12.7 / 3.2 / -3.5 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.002), // 0.000969 / 0.000381
            (Bla, Shipping, 0.05, 0.2), // 0.012 / 0.0553
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000192
            (Compression, Shipping, 0.001, 0.02), // 0.000245 / 0.00363
            (Detection, Oversampled, 10.0, NAN), // 2.77
            (Detection, Shipping, 20.0, NAN), // 5.91
            (Dynamics, Oversampled, 0.001, 5e-4), // 0.000319 / 9.73e-05
            (Dynamics, Shipping, 0.01, 0.005), // 0.00181 / 0.00154
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.64e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 2.82e-08
            (Harmonics, Oversampled, 50.0, 0.2), // 7.86 / 0.0662
            (Harmonics, Shipping, 50.0, 5.0), // 12.1 / 0.739
            (Intermodulation, Oversampled, 0.5, 0.1), // 0.13 / 0.0179
            (Intermodulation, Shipping, 1.0, 0.2), // 0.241 / 0.0447
            (OperatingPoint, Oversampled, 1e-5, NAN), // 2.51e-06
            (OperatingPoint, Shipping, 1e-5, NAN), // 2.51e-06
            (Response, Oversampled, 0.01, 0.01), // 0.0018 / 0.00332
            (Response, Shipping, 0.01, 0.05), // 0.00184 / 0.00749
            (ThdN, Oversampled, 0.5, NAN), // 0.129
            (ThdN, Shipping, 5.0, NAN), // 0.924 — aliasing at the host rate
        ],
        ("fuzz_face", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -35.0, NAN), // -44.6 — aliasing at the host rate
            (Anmr, Shipping, 5.0, NAN), // -0.6 / -20.0 / -19.9 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.5, 1.0), // 0.121 / 0.227
            (Bla, Shipping, 20.0, 100.0), // 5.61 / 16.8 (R11: the whole card)
            (Compression, Oversampled, 0.5, 1.0), // 0.121 / 0.227 — FINDING: +0.12 dB of gain from the operating point
            (Compression, Shipping, 2.0, 20.0), // 0.641 / 3.88 (R11: the whole card)
            (Detection, Oversampled, 1.0, NAN), // 0.326
            (Detection, Shipping, 2.0, NAN), // 0.336
            (Dynamics, Oversampled, 0.002, 0.1), // 0.000505 / 0.0257
            (Dynamics, Shipping, 0.005, 2.0), // 0.000698 / 0.566 (R11: the whole card)
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.53e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 1.75e-07
            (Harmonics, Oversampled, 5.0, 2.0), // 1.04 / 0.371
            (Harmonics, Shipping, 20.0, 50.0), // 5.17 / 8.55 (R11: the whole card)
            (Intermodulation, Oversampled, 2.0, 5.0), // 0.462 / 0.82
            (Intermodulation, Shipping, 100.0, 1e3), // 25.2 / 170 (R11: the whole card)
            (OperatingPoint, Oversampled, 0.05, NAN), // 0.00936 — FINDING: 9.4 mV off ngspice at Q2's collector (open)
            (OperatingPoint, Shipping, 0.05, NAN), // 0.00936
            (Response, Oversampled, 0.5, 1.0), // 0.12 / 0.226 — FINDING: +0.12 dB of gain from the operating point
            (Response, Shipping, 20.0, 100.0), // 5.61 / 16.7 (R11: the whole card)
            (ThdN, Oversampled, 0.5, NAN), // 0.145
            (ThdN, Shipping, 10.0, NAN), // 2.47 (R11: the whole card)
        ],
        ("fuzz_face_static", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -35.0, NAN), // -44.6 — aliasing at the host rate
            (Anmr, Shipping, 15.0, NAN), // 8.1 / -25.5 / -78.7 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.5, 1.0), // 0.121 / 0.227
            (Bla, Shipping, 0.5, 1.0), // 0.121 / 0.227
            (Compression, Oversampled, 0.5, 1.0), // 0.121 / 0.227 — FINDING: +0.12 dB of gain from the operating point
            (Compression, Shipping, 0.5, 1.0), // 0.121 / 0.227
            (Detection, Oversampled, 1.0, NAN), // 0.326
            (Detection, Shipping, 2.0, NAN), // 0.336
            (Dynamics, Oversampled, 0.002, 0.1), // 0.000505 / 0.0257
            (Dynamics, Shipping, 0.005, 0.2), // 0.000729 / 0.056
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.53e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 1.75e-07
            (Harmonics, Oversampled, 5.0, 2.0), // 1.04 / 0.371
            (Harmonics, Shipping, 5.0, 2.0), // 1.03 / 0.371
            (Intermodulation, Oversampled, 2.0, 5.0), // 0.462 / 0.82
            (Intermodulation, Shipping, 2.0, 5.0), // 0.452 / 0.82
            (OperatingPoint, Oversampled, 0.05, NAN), // 0.00936 — FINDING: 9.4 mV off ngspice at Q2's collector (open)
            (OperatingPoint, Shipping, 0.05, NAN), // 0.00936
            (Response, Oversampled, 0.5, 1.0), // 0.12 / 0.226 — FINDING: +0.12 dB of gain from the operating point
            (Response, Shipping, 0.5, 1.0), // 0.12 / 0.226
            (ThdN, Oversampled, 0.5, NAN), // 0.145
            (ThdN, Shipping, 0.5, NAN), // 0.159 — aliasing at the host rate
        ],
        ("diode_clipper/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -55.0, NAN), // -62.1
            (Aliasing, Shipping, -5.0, NAN), // -11.9 — aliasing at the host rate
            (Anmr, Shipping, 75.0, NAN), // 68.5 / 57.8 / 29.7 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 1.0, 2.0), // 0.167 / 0.493
            (Bla, Shipping, 20.0, 50.0), // 5.33 / 11.8 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.1, 0.5), // 0.027 / 0.159
            (Compression, Shipping, 5.0, 100.0), // 0.714 / 19.6
            (Detection, Oversampled, 0.1, NAN), // 0.0303
            (Detection, Shipping, 500.0, NAN), // 89.2
            (Dynamics, Oversampled, 5e-5, 0.1), // 1.15e-05 / 0.026
            (Dynamics, Shipping, 5e-4, 0.5), // 9.66e-05 / 0.119
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.69e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 7.26e-07
            (Harmonics, Oversampled, 100.0, 200.0), // 17.6 / 45.5
            (Harmonics, Shipping, 100.0, 500.0), // 21.7 / 77.4
            (Intermodulation, Oversampled, 20.0, 20.0), // 5.19 / 5.1
            (Intermodulation, Shipping, 200.0, 200.0), // 50.9 / 57.1
            (OperatingPoint, Oversampled, 1e-9, NAN), // 4.34e-22
            (OperatingPoint, Shipping, 1e-9, NAN), // 4.34e-22
            (Response, Oversampled, 1.0, 2.0), // 0.169 / 0.489
            (Response, Shipping, 20.0, 50.0), // 5.33 / 11.3 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 200.0, NAN), // 48.7
            (ThdN, Shipping, 500.0, NAN), // 98.9 — aliasing at the host rate
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
            (Aliasing, Oversampled, -55.0, NAN), // -60.3
            (Aliasing, Shipping, -5.0, NAN), // -13.2 — aliasing at the host rate
            (Anmr, Shipping, 65.0, NAN), // 59.9 / 42.6 / 23.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.02), // 0.000968 / 0.00432
            (Bla, Shipping, 0.5, 1.0), // 0.0789 / 0.272
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.000211
            (Compression, Shipping, 0.001, 0.02), // 0.000244 / 0.00401
            (Detection, Oversampled, 0.05, NAN), // 0.00981
            (Detection, Shipping, 5.0, NAN), // 1.34
            (Dynamics, Oversampled, 0.001, 1e-4), // 0.000246 / 2.44e-05
            (Dynamics, Shipping, 0.005, 0.002), // 0.000966 / 0.000582
            (GroupDelay, Oversampled, 2e-4, NAN), // 2.28e-15
            (GroupDelay, Shipping, 2e-4, NAN), // 1.34e-09
            (Harmonics, Oversampled, 1.0, 0.5), // 0.235 / 0.122
            (Harmonics, Shipping, 5.0, 1.0), // 0.989 / 0.265
            (Intermodulation, Oversampled, 0.2, 0.2), // 0.0617 / 0.0344
            (Intermodulation, Shipping, 0.2, 0.1), // 0.0343 / 0.0181
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 1e-6, 1e-5), // 6.39e-10 / 1.41e-09
            (Response, Shipping, 5e-6, 0.05), // 1.06e-06 / 0.0086
            (ThdN, Oversampled, 0.02, NAN), // 0.00386
            (ThdN, Shipping, 50.0, NAN), // 15.2 — aliasing at the host rate
        ],
        ("common_cathode_grid_current/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -55.0, NAN), // -60.3
            (Aliasing, Shipping, -5.0, NAN), // -13.2 — aliasing at the host rate
            (Anmr, Shipping, 65.0, NAN), // 59.9 / 42.6 / 23.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.02), // 0.000969 / 0.00432
            (Bla, Shipping, 0.5, 1.0), // 0.0789 / 0.272
            (Compression, Oversampled, 0.001, 0.001), // 0.000243 / 0.00021
            (Compression, Shipping, 0.001, 0.02), // 0.000244 / 0.004
            (Detection, Oversampled, 5.0, NAN), // 1.11
            (Detection, Shipping, 10.0, NAN), // 3.22
            (Dynamics, Oversampled, 0.001, 1e-4), // 0.000246 / 1.99e-05
            (Dynamics, Shipping, 0.005, 0.002), // 0.000966 / 0.000576
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.4e-08
            (GroupDelay, Shipping, 2e-4, NAN), // 2.16e-08
            (Harmonics, Oversampled, 1.0, 0.5), // 0.247 / 0.122
            (Harmonics, Shipping, 20.0, 1.0), // 5.46 / 0.265
            (Intermodulation, Oversampled, 0.2, 0.2), // 0.0623 / 0.037
            (Intermodulation, Shipping, 0.5, 0.1), // 0.0784 / 0.0205
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.03e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.03e-08
            (Response, Oversampled, 0.005, 0.01), // 0.00158 / 0.00278
            (Response, Shipping, 0.005, 0.05), // 0.00161 / 0.00857
            (ThdN, Oversampled, 0.2, NAN), // 0.041
            (ThdN, Shipping, 50.0, NAN), // 15.2 — aliasing at the host rate
        ],
        ("jfet_booster", Tier::T1) => &[
            (Aliasing, Oversampled, -45.0, NAN), // -54.8
            (Aliasing, Shipping, -10.0, NAN), // -16.7 — aliasing at the host rate
            (Anmr, Shipping, 60.0, NAN), // 52.7 / 41.9 / 33.0 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.05), // 0.00131 / 0.0101
            (Bla, Shipping, 0.1, 1.0), // 0.0331 / 0.236
            (Compression, Oversampled, 0.001, 0.002), // 0.000256 / 0.000358
            (Compression, Shipping, 0.001, 0.1), // 0.000235 / 0.0303 (R11: the whole card)
            (Detection, Oversampled, 0.05, NAN), // 0.0108
            (Detection, Shipping, 5.0, NAN), // 0.759
            (Dynamics, Oversampled, 2e-5, 0.002), // 4.77e-06 / 0.000449
            (Dynamics, Shipping, 5e-4, 0.02), // 7.82e-05 / 0.00622
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.77e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 2.17e-09
            (Harmonics, Oversampled, 2.0, 20.0), // 0.572 / 4.3 (R11: the whole card)
            (Harmonics, Shipping, 10.0, 20.0), // 2.23 / 4.69
            (Intermodulation, Oversampled, 5.0, 0.2), // 0.775 / 0.0482
            (Intermodulation, Shipping, 10.0, 1.0), // 1.72 / 0.225
            (OperatingPoint, Oversampled, 1e-4, NAN), // 3.21e-05 — FINDING: ngspice's gate sits 4.3 µV up (4.3 pA in 1 MΩ; not gmin)
            (OperatingPoint, Shipping, 1e-4, NAN), // 3.21e-05
            (Response, Oversampled, 5e-5, 5e-4), // 1.45e-05 / 7.98e-05
            (Response, Shipping, 1e-4, 0.2), // 2.04e-05 / 0.06 (R11: the whole card)
            (ThdN, Oversampled, 0.01, NAN), // 0.00309
            (ThdN, Shipping, 50.0, NAN), // 7.42 — aliasing at the host rate
        ],
        ("jfet_booster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -45.0, NAN), // -54.8
            (Aliasing, Shipping, -10.0, NAN), // -16.7 — aliasing at the host rate
            (Anmr, Shipping, 60.0, NAN), // 52.7 / 41.9 / 33.0 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.05), // 0.00131 / 0.0101
            (Bla, Shipping, 0.1, 1.0), // 0.0331 / 0.236
            (Compression, Oversampled, 0.001, 0.002), // 0.000256 / 0.000358
            (Compression, Shipping, 0.001, 0.02), // 0.000261 / 0.00648
            (Detection, Oversampled, 0.05, NAN), // 0.0108
            (Detection, Shipping, 5.0, NAN), // 0.759
            (Dynamics, Oversampled, 2e-5, 0.002), // 4.77e-06 / 0.000449
            (Dynamics, Shipping, 5e-4, 0.02), // 7.82e-05 / 0.00622
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.77e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 2.17e-09
            (Harmonics, Oversampled, 2.0, 1.0), // 0.575 / 0.294
            (Harmonics, Shipping, 10.0, 20.0), // 2.23 / 4.69
            (Intermodulation, Oversampled, 5.0, 0.2), // 0.775 / 0.0482
            (Intermodulation, Shipping, 10.0, 1.0), // 1.72 / 0.225
            (OperatingPoint, Oversampled, 1e-4, NAN), // 3.21e-05 — FINDING: ngspice's gate sits 4.3 µV up (4.3 pA in 1 MΩ; not gmin)
            (OperatingPoint, Shipping, 1e-4, NAN), // 3.21e-05
            (Response, Oversampled, 5e-5, 5e-4), // 1.45e-05 / 7.98e-05
            (Response, Shipping, 5e-5, 0.05), // 1.43e-05 / 0.0139
            (ThdN, Oversampled, 0.01, NAN), // 0.00309
            (ThdN, Shipping, 50.0, NAN), // 7.42 — aliasing at the host rate
        ],
        ("opamp_clipper/hand", Tier::T1) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -110
            (Aliasing, Shipping, -5.0, NAN), // -13.3 — aliasing at the host rate
            (Anmr, Shipping, 70.0, NAN), // 64.5 / 54.1 / 26.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.1), // 0.000793 / 0.0233
            (Bla, Shipping, 5.0, 50.0), // 0.737 / 12.6 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.001, 0.05), // 0.000216 / 0.0137
            (Compression, Shipping, 2.0, 50.0), // 0.37 / 9.29
            (Detection, Oversampled, 0.001, NAN), // 0.000272
            (Detection, Shipping, 500.0, NAN), // 88.5
            (Dynamics, Oversampled, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (Dynamics, Shipping, 0.002, 0.2), // 0.000357 / 0.0538
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.5e-09
            (GroupDelay, Shipping, 2e-4, NAN), // 2.91e-07
            (Harmonics, Oversampled, 2.0, 20.0), // 0.511 / 3.62
            (Harmonics, Shipping, 50.0, 500.0), // 12.2 / 72.1
            (Intermodulation, Oversampled, 0.5, 5.0), // 0.074 / 0.669
            (Intermodulation, Shipping, 100.0, 500.0), // 26.4 / 112
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 0.005, 0.1), // 0.000824 / 0.0224
            (Response, Shipping, 5.0, 50.0), // 0.738 / 12.6 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 50.0, NAN), // 9.53
            (ThdN, Shipping, 500.0, NAN), // 107 — aliasing at the host rate
        ],
        ("mosfet_booster", Tier::T1) => &[
            (Aliasing, Oversampled, -35.0, NAN), // -41.7
            (Aliasing, Shipping, 0.0, NAN), // -7.7 — aliasing at the host rate
            (Anmr, Shipping, 80.0, NAN), // 70.7 / 62.0 / 47.0 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.05, 0.1), // 0.00834 / 0.0303
            (Bla, Shipping, 1.0, 10.0), // 0.294 / 2.26
            (Compression, Oversampled, 0.002, 0.05), // 0.000406 / 0.00731 (R11: the whole card)
            (Compression, Shipping, 0.05, 5.0), // 0.0136 / 0.87 (R11: the whole card)
            (Detection, Oversampled, 50.0, NAN), // 9.93
            (Detection, Shipping, 50.0, NAN), // 9.93
            (Dynamics, Oversampled, 2e-4, 0.02), // 4.33e-05 / 0.00452
            (Dynamics, Shipping, 0.002, 0.5), // 0.000351 / 0.142
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.05e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.73e-09
            (Harmonics, Oversampled, 1.0, 2.0), // 0.275 / 0.638
            (Harmonics, Shipping, 50.0, 200.0), // 14.4 / 65.7 (R11: the whole card)
            (Intermodulation, Oversampled, 2.0, 1.0), // 0.639 / 0.245
            (Intermodulation, Shipping, 50.0, 200.0), // 9.76 / 44.5 (R11: the whole card)
            (OperatingPoint, Oversampled, 2e-5, NAN), // 6.53e-06
            (OperatingPoint, Shipping, 2e-5, NAN), // 6.53e-06
            (Response, Oversampled, 2e-5, 1e-5), // 6.15e-06 / 1.03e-08
            (Response, Shipping, 0.02, 2.0), // 0.00406 / 0.591 (R11: the whole card)
            (ThdN, Oversampled, 5.0, NAN), // 1.14
            (ThdN, Shipping, 100.0, NAN), // 21.2 — aliasing at the host rate
        ],
        ("mosfet_booster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -35.0, NAN), // -41.7
            (Aliasing, Shipping, 0.0, NAN), // -7.7 — aliasing at the host rate
            (Anmr, Shipping, 80.0, NAN), // 71.2 / 62.5 / 46.9 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.05, 0.1), // 0.00834 / 0.0303
            (Bla, Shipping, 1.0, 10.0), // 0.294 / 2.26
            (Compression, Oversampled, 0.001, 0.002), // 0.000243 / 0.000652
            (Compression, Shipping, 0.001, 0.02), // 0.000266 / 0.0062
            (Detection, Oversampled, 50.0, NAN), // 9.93
            (Detection, Shipping, 50.0, NAN), // 9.93
            (Dynamics, Oversampled, 2e-4, 0.02), // 4.33e-05 / 0.00452
            (Dynamics, Shipping, 0.002, 0.5), // 0.000351 / 0.142
            (GroupDelay, Oversampled, 2e-4, NAN), // 1.05e-14
            (GroupDelay, Shipping, 2e-4, NAN), // 1.73e-09
            (Harmonics, Oversampled, 1.0, 2.0), // 0.275 / 0.638
            (Harmonics, Shipping, 5.0, 50.0), // 0.905 / 8.21
            (Intermodulation, Oversampled, 2.0, 1.0), // 0.639 / 0.245
            (Intermodulation, Shipping, 2.0, 10.0), // 0.647 / 1.78
            (OperatingPoint, Oversampled, 2e-5, NAN), // 6.53e-06
            (OperatingPoint, Shipping, 2e-5, NAN), // 6.53e-06
            (Response, Oversampled, 2e-5, 1e-5), // 6.15e-06 / 1.03e-08
            (Response, Shipping, 2e-5, 0.05), // 6.07e-06 / 0.0111
            (ThdN, Oversampled, 5.0, NAN), // 1.14
            (ThdN, Shipping, 100.0, NAN), // 21.2 — aliasing at the host rate
        ],
        ("power_supply", Tier::T1) => &[
            (Dynamics, Oversampled, 0.001, 0.02), // 0.000177 / 0.00521
            (Dynamics, Shipping, 0.01, 0.2), // 0.00272 / 0.0374
            (Harmonics, Oversampled, 0.05, 1.0), // 0.00668 / 0.194
            (Harmonics, Shipping, 0.2, 5.0), // 0.0571 / 1.57
            (OperatingPoint, Oversampled, 5e-4, NAN), // 0.00014
            (OperatingPoint, Shipping, 0.005, NAN), // 0.00114
        ],
        ("ota_vca", Tier::T1) => &[
            (Aliasing, Oversampled, -80.0, NAN), // -88
            (Aliasing, Shipping, -5.0, NAN), // -11.1 — aliasing at the host rate
            (Anmr, Shipping, 75.0, NAN), // 65.7 / 48.4 / 8.4 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.00123
            (Bla, Shipping, 0.5, 5.0), // 0.1 / 0.733
            (Compression, Oversampled, 0.001, 0.002), // 0.000332 / 0.000346
            (Compression, Shipping, 0.001, 0.002), // 0.000332 / 0.000346
            (Detection, Oversampled, 0.01, NAN), // 0.00257
            (Detection, Shipping, 100.0, NAN), // 24.3
            (Dynamics, Oversampled, 5e-6, 2e-4), // 1.47e-06 / 4.73e-05
            (Dynamics, Shipping, 5e-6, 2e-4), // 1.47e-06 / 4.73e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.2), // 0.162 / 0.0608
            (Harmonics, Shipping, 0.1, 0.05), // 0.0293 / 0.0133
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.065 / 0.0254
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0265 / 0.00433
            (OperatingPoint, Oversampled, 5e-5, NAN), // 7.25e-06
            (OperatingPoint, Shipping, 5e-5, NAN), // 7.25e-06
            (Response, Oversampled, 0.005, NAN), // 0.00122
            (Response, Shipping, 0.005, NAN), // 0.00122
            (ThdN, Oversampled, 0.01, NAN), // 0.0024
            (ThdN, Shipping, 200.0, NAN), // 42.7 — aliasing at the host rate
        ],
        ("opamp_clipper", Tier::T1) => &[
            (Aliasing, Oversampled, -105.0, NAN), // -110
            (Aliasing, Shipping, -5.0, NAN), // -13.3 — aliasing at the host rate
            (Anmr, Shipping, 70.0, NAN), // 64.5 / 54.1 / 26.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.1), // 0.000793 / 0.0233
            (Bla, Shipping, 5.0, 50.0), // 0.737 / 12.6 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.001, 0.05), // 0.000216 / 0.0137
            (Compression, Shipping, 2.0, 50.0), // 0.37 / 9.29
            (Detection, Oversampled, 0.001, NAN), // 0.000272
            (Detection, Shipping, 500.0, NAN), // 88.5
            (Dynamics, Oversampled, 1e-4, 0.01), // 2.24e-05 / 0.00273
            (Dynamics, Shipping, 0.002, 0.2), // 0.000357 / 0.0538
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.43e-12
            (GroupDelay, Shipping, 2e-4, NAN), // 2.91e-07
            (Harmonics, Oversampled, 2.0, 20.0), // 0.511 / 3.62
            (Harmonics, Shipping, 50.0, 500.0), // 12.2 / 72.1
            (Intermodulation, Oversampled, 0.5, 5.0), // 0.074 / 0.669
            (Intermodulation, Shipping, 100.0, 500.0), // 26.4 / 112
            (OperatingPoint, Oversampled, 1e-9, NAN), // 0
            (OperatingPoint, Shipping, 1e-9, NAN), // 0
            (Response, Oversampled, 5e-6, 1e-5), // 8.22e-07 / 2.39e-06
            (Response, Shipping, 5.0, 50.0), // 0.738 / 12.6 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 50.0, NAN), // 9.53
            (ThdN, Shipping, 500.0, NAN), // 107 — aliasing at the host rate
        ],
        ("rangemaster", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -20.0, NAN), // -25.3 — aliasing at the host rate
            (Anmr, Shipping, 40.0, NAN), // 34.6 / 3.4 / -8.2 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.5, 10.0), // 0.137 / 2.84
            (Bla, Shipping, 5.0, 200.0), // 1.66 / 35.9
            (Compression, Oversampled, 0.5, 1.0), // 0.131 / 0.293
            (Compression, Shipping, 1.0, 10.0), // 0.191 / 2.03
            (Detection, Oversampled, 2.0, NAN), // 0.365
            (Detection, Shipping, 10.0, NAN), // 2.34
            (Dynamics, Oversampled, 2e-4, 0.02), // 4.34e-05 / 0.00587
            (Dynamics, Shipping, 5e-4, 0.5), // 0.000166 / 0.0976
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.08e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 7.73e-07
            (Harmonics, Oversampled, 10.0, 10.0), // 1.79 / 1.72
            (Harmonics, Shipping, 10.0, 50.0), // 2.51 / 10.9
            (Intermodulation, Oversampled, 2.0, 20.0), // 0.608 / 3.89
            (Intermodulation, Shipping, 10.0, 50.0), // 2.6 / 13.7
            (OperatingPoint, Oversampled, 0.02, NAN), // 0.0039 — FINDING: 3.9 mV off ngspice at the collector (open)
            (OperatingPoint, Shipping, 0.02, NAN), // 0.0039
            (Response, Oversampled, 0.5, 2.0), // 0.133 / 0.337 — FINDING: 0.13 dB of gain from the operating point
            (Response, Shipping, 1.0, 20.0), // 0.193 / 3.9
            (ThdN, Oversampled, 1.0, NAN), // 0.234
            (ThdN, Shipping, 2.0, NAN), // 0.342 — aliasing at the host rate
        ],
        ("rangemaster_static", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -20.0, NAN), // -25.3 — aliasing at the host rate
            (Anmr, Shipping, 45.0, NAN), // 37.3 / 12.6 / -9.0 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.5, 10.0), // 0.137 / 2.84
            (Bla, Shipping, 5.0, 200.0), // 1.66 / 35.9
            (Compression, Oversampled, 0.5, 1.0), // 0.131 / 0.293
            (Compression, Shipping, 1.0, 10.0), // 0.191 / 2.03
            (Detection, Oversampled, 2.0, NAN), // 0.365
            (Detection, Shipping, 10.0, NAN), // 2.34
            (Dynamics, Oversampled, 2e-4, 0.02), // 4.34e-05 / 0.00587
            (Dynamics, Shipping, 5e-4, 0.5), // 0.000166 / 0.0976
            (GroupDelay, Oversampled, 2e-4, NAN), // 7.08e-07
            (GroupDelay, Shipping, 2e-4, NAN), // 7.73e-07
            (Harmonics, Oversampled, 10.0, 10.0), // 1.79 / 1.72
            (Harmonics, Shipping, 10.0, 50.0), // 2.51 / 10.9
            (Intermodulation, Oversampled, 2.0, 20.0), // 0.608 / 3.89
            (Intermodulation, Shipping, 10.0, 50.0), // 2.6 / 13.7
            (OperatingPoint, Oversampled, 0.02, NAN), // 0.0039 — FINDING: 3.9 mV off ngspice at the collector (open)
            (OperatingPoint, Shipping, 0.02, NAN), // 0.0039
            (Response, Oversampled, 0.5, 2.0), // 0.133 / 0.337 — FINDING: 0.13 dB of gain from the operating point
            (Response, Shipping, 1.0, 20.0), // 0.193 / 3.9
            (ThdN, Oversampled, 1.0, NAN), // 0.234
            (ThdN, Shipping, 2.0, NAN), // 0.342 — aliasing at the host rate
        ],
        ("triode_stage", Tier::T1) => &[
            (Aliasing, Oversampled, -70.0, NAN), // -78.6
            (Aliasing, Shipping, -15.0, NAN), // -20.9 — aliasing at the host rate
            (Anmr, Shipping, 50.0, NAN), // 42.0 / 28.3 / 16.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.005), // 0.000968 / 0.000992
            (Bla, Shipping, 0.05, 1.0), // 0.0163 / 0.261
            (Compression, Oversampled, 0.001, 5e-5), // 0.000243 / 1.03e-05
            (Compression, Shipping, 0.001, 5e-5), // 0.000243 / 1.03e-05
            (Detection, Oversampled, 0.01, NAN), // 0.00327
            (Detection, Shipping, 2.0, NAN), // 0.397
            (Dynamics, Oversampled, 1e-4, 2e-4), // 2.27e-05 / 6.38e-05
            (Dynamics, Shipping, 1e-4, 2e-4), // 2.27e-05 / 6.38e-05
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.1), // 0.0949 / 0.0281
            (Harmonics, Shipping, 0.5, 0.05), // 0.0949 / 0.00675
            (Intermodulation, Oversampled, 0.2, 0.05), // 0.0655 / 0.0131
            (Intermodulation, Shipping, 0.1, 0.02), // 0.0273 / 0.00353
            (OperatingPoint, Oversampled, 1e-4, NAN), // 2.11e-05
            (OperatingPoint, Shipping, 1e-4, NAN), // 2.11e-05
            (Response, Oversampled, 5e-6, NAN), // 7.23e-07
            (Response, Shipping, 5e-6, NAN), // 7.23e-07
            (ThdN, Oversampled, 0.05, NAN), // 0.0101
            (ThdN, Shipping, 50.0, NAN), // 10.5 — aliasing at the host rate
        ],
        ("push_pull", Tier::T1) => &[
            (Aliasing, Oversampled, -45.0, NAN), // -52.8
            (Aliasing, Shipping, -5.0, NAN), // -14.8 — aliasing at the host rate
            (Anmr, Shipping, 60.0, NAN), // 49.0 / 53.1 / 30.0 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.05), // 0.00166 / 0.0114
            (Bla, Shipping, 0.5, 2.0), // 0.0912 / 0.48
            (Compression, Oversampled, 0.002, 5e-4), // 0.000455 / 0.000105
            (Compression, Shipping, 0.002, 5e-4), // 0.000455 / 0.000105
            (Detection, Oversampled, 0.02, NAN), // 0.00342
            (Detection, Shipping, 500.0, NAN), // 82.6
            (Dynamics, Oversampled, 2e-4, 5e-4), // 4.54e-05 / 7.9e-05
            (Dynamics, Shipping, 2e-4, 5e-4), // 4.54e-05 / 0.000114
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 0.5, 0.1), // 0.109 / 0.0175
            (Harmonics, Shipping, 0.1, 0.05), // 0.0283 / 0.0161
            (Intermodulation, Oversampled, 0.1, 0.1), // 0.0307 / 0.0332
            (Intermodulation, Shipping, 0.05, 0.05), // 0.0156 / 0.0157
            (OperatingPoint, Oversampled, 5e-8, NAN), // 7.5e-09
            (OperatingPoint, Shipping, 5e-8, NAN), // 7.5e-09
            (Response, Oversampled, 1e-6, NAN), // 4.68e-10
            (Response, Shipping, 1e-6, NAN), // 4.68e-10
            (ThdN, Oversampled, 200.0, NAN), // 64.9
            (ThdN, Shipping, 500.0, NAN), // 103 — aliasing at the host rate
        ],
        ("push_pull_pentode", Tier::T1) => &[
            (Aliasing, Oversampled, -55.0, NAN), // -63.7
            (Aliasing, Shipping, -10.0, NAN), // -18.7 — aliasing at the host rate
            (Anmr, Shipping, 65.0, NAN), // 59.6 / 31.2 / 18.3 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.005, 0.02), // 0.00112 / 0.0061
            (Bla, Shipping, 0.2, 2.0), // 0.0629 / 0.588
            (Compression, Oversampled, 0.002, 5e-4), // 0.000426 / 0.000146
            (Compression, Shipping, 0.002, 5e-4), // 0.000426 / 0.000146
            (Detection, Oversampled, 0.01, NAN), // 0.00217
            (Detection, Shipping, 500.0, NAN), // 82.3
            (Dynamics, Oversampled, 2e-4, 2e-4), // 5.19e-05 / 3.92e-05
            (Dynamics, Shipping, 2e-4, 0.02), // 5.19e-05 / 0.00377
            (GroupDelay, Oversampled, 2e-4, NAN), // 0
            (GroupDelay, Shipping, 2e-4, NAN), // 0
            (Harmonics, Oversampled, 2.0, 1.0), // 0.346 / 0.171
            (Harmonics, Shipping, 1.0, 0.1), // 0.202 / 0.0322
            (Intermodulation, Oversampled, 0.2, 0.1), // 0.0372 / 0.0235
            (Intermodulation, Shipping, 50.0, 0.02), // 15.8 / 0.00381
            (OperatingPoint, Oversampled, 5e-5, NAN), // 1.08e-05
            (OperatingPoint, Shipping, 5e-5, NAN), // 1.08e-05
            (Response, Oversampled, 1e-6, NAN), // 5.65e-09
            (Response, Shipping, 1e-6, NAN), // 5.65e-09
            (ThdN, Oversampled, 200.0, NAN), // 52.6
            (ThdN, Shipping, 500.0, NAN), // 97.7 — aliasing at the host rate
        ],
        ("tube_screamer", Tier::T1) => &[
            (Aliasing, Oversampled, -115.0, NAN), // -120
            (Aliasing, Shipping, -15.0, NAN), // -20.6 — aliasing at the host rate
            (Anmr, Shipping, 70.0, NAN), // 60.5 / 43.2 / -7.1 at 44.1 / 48 / 96 kHz — FINDING: audible aliasing at the host rate
            (Bla, Oversampled, 0.05, 0.1), // 0.0101 / 0.0265
            (Bla, Shipping, 20.0, 50.0), // 5.62 / 10.6 — FINDING: bilinear warping at 1× (no prewarp)
            (Compression, Oversampled, 0.01, 0.05), // 0.00198 / 0.0115
            (Compression, Shipping, 5.0, 20.0), // 0.805 / 5.21
            (Detection, Oversampled, 0.005, NAN), // 0.000945
            (Detection, Shipping, 500.0, NAN), // 79.5
            (Dynamics, Oversampled, 5e-5, 0.1), // 1.33e-05 / 0.0224
            (Dynamics, Shipping, 0.001, 2.0), // 0.000208 / 0.466
            (GroupDelay, Oversampled, 2e-4, NAN), // 3.74e-11
            (GroupDelay, Shipping, 2e-4, NAN), // 1.06e-06
            (Harmonics, Oversampled, 5.0, 20.0), // 0.699 / 3.53
            (Harmonics, Shipping, 100.0, 500.0), // 23 / 69.6
            (Intermodulation, Oversampled, 1.0, 1.0), // 0.207 / 0.203
            (Intermodulation, Shipping, 200.0, 100.0), // 56.5 / 22.6
            (OperatingPoint, Oversampled, 1e-7, NAN), // 3.05e-08
            (OperatingPoint, Shipping, 1e-7, NAN), // 3.05e-08
            (Response, Oversampled, 5e-5, 1e-4), // 9.11e-06 / 3.1e-05
            (Response, Shipping, 20.0, 50.0), // 5.62 / 8.07 — FINDING: bilinear warping at 1× (no prewarp)
            (ThdN, Oversampled, 0.1, NAN), // 0.0248
            (ThdN, Shipping, 500.0, NAN), // 99.4 — aliasing at the host rate
        ],
        (case, tier) => panic!("no budgets for `{case}` at {tier:?}"),
    }
}
