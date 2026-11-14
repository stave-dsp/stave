//! DSP.md R6: the whole library across the rates a host may pick, one
//! sweep over every catalogued stage (`library/mod.rs`: the stave-host
//! catalog and its reference models, the circuits — hand topologies and
//! every deck and classic on `Circuit<Dk>` and `Circuit<Wdf>` — learned,
//! physical, effects and primitives).
//!
//! - **Robust**, 8 kHz to 768 kHz: prepared at the rate, each stage takes
//!   silence (its rest input), a hot 1 kHz sine, DC and an impulse, a
//!   reset between: no panic, every sample finite, never subnormal, and
//!   inside the entry's bound; nothing allocates after `prepare` (a
//!   counting allocator, armed per thread). A learned model's rate
//!   policy refusing a rate is an expected refusal — it must say so
//!   (`rate_mismatch`) and output silence — not a failure.
//! - **Invariant**, 44.1 to 192 kHz: the same response as at 48 kHz
//!   within the entry's budget ([`BUDGETS`]): small-signal gain at 100
//!   Hz, 1 kHz and 5 kHz and the level-dependent harmonic (the stronger
//!   of the 2nd and 3rd under a hot 200 Hz sine, floored at −100 dBc) for
//!   a time-invariant stage; the output level of a multisine for a
//!   time-varying one; the energy out for a short pulse for a resonant
//!   or reverberant one; level and zero-crossing rate for a generator;
//!   level alone for a random process (`library::Measure`).
//!
//! T0 (every run, about 30 s on three cores unoptimised): robustness at
//! 8, 44.1 and 768 kHz, and invariance at 44.1 kHz for every entry but
//! the heavy netlists (the costliest decks, the WDF twins of the DK
//! decks, the reference models). T1 (`cargo test -p stave-test --test
//! rate_range -- --ignored`, deep.yml's rate-range job, about eleven
//! minutes): robustness at fifteen rates from 8 to 768 kHz and
//! invariance at 44.1, 48, 88.2, 96, 176.4 and 192 kHz. Both read the
//! same signals over the same seconds, so a budget derived from T1's
//! spread holds T0's. Entries run on `stave_test::threads()`.

mod library;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use library::{library, Entry, Measure};
use stave_measure::level::db;
use stave_measure::spectrum::goertzel;

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            COUNT.with(|c| c.set(c.get() + 1));
        }
    });
}

// SAFETY: every method forwards to `System` with the caller's arguments,
// unchanged; counting touches only thread-local cells.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc`, passed through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` via `alloc`/`realloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `realloc`, passed through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Allocations `f` made on this thread, and what it returned.
fn allocations_in<T>(f: impl FnOnce() -> T) -> (usize, T) {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    let out = f();
    ARMED.with(|a| a.set(false));
    (COUNT.with(Cell::get), out)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Tier {
    T0,
    T1,
}

const T0_ROBUST: [f32; 3] = [8_000.0, 44_100.0, 768_000.0];
const T1_ROBUST: [f32; 15] = [
    8_000.0, 11_025.0, 16_000.0, 22_050.0, 32_000.0, 44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0,
    192_000.0, 352_800.0, 384_000.0, 705_600.0, 768_000.0,
];
/// The reference rate the others are held to.
const REFERENCE: f32 = 48_000.0;
const T0_INVARIANT: [f32; 1] = [44_100.0];
const T1_INVARIANT: [f32; 5] = [44_100.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0];

/// Seconds of each robustness stimulus.
const ROBUST_SECONDS: f64 = 0.025;
/// Seconds before an invariance capture (the slowest settling stage
/// here, a 20 Hz coupler or a compressor's release, is well inside), and
/// the captures: a response's (whole cycles of 5 Hz multiples) and a
/// level's or generator's (whole cycles of the slowest modulation that
/// matters, a rotor's).
const WARM_SECONDS: f64 = 0.5;
const RESPONSE_SECONDS: f64 = 0.2;
const LEVEL_SECONDS: f64 = 1.0;
const BLOCK: usize = 64;

/// What one rate did to one entry.
#[derive(Default)]
struct Robust {
    refused: bool,
    failures: Vec<String>,
}

/// Prepare `entry` at `rate` and run the four stimuli through it.
fn robust(entry: &Entry, rate: f32) -> Robust {
    let mut subject = (entry.build)();
    subject.stage().prepare(rate);
    let mut out = Robust::default();
    // A refusal is the policy's (`refuses`): the model must report the
    // mismatch, and output silence.
    let expected = (entry.refuses)(rate);
    if expected && subject.refusal().is_none() {
        out.failures.push(format!("its policy refuses {rate} Hz, but it reports no rate mismatch"));
    }
    out.refused = expected;
    let n = ((f64::from(rate) * ROBUST_SECONDS) as usize).max(4 * BLOCK);
    let mut block = vec![0.0f32; BLOCK];
    // Worst |y|, non-finite and subnormal counts per stimulus.
    let mut stats = [(0.0f32, 0usize, 0usize, 0.0f32); 4];
    // Twice the entry's hot level (a generator, which ignores its
    // input, takes full scale).
    let (bias, hot) = (entry.bias, if entry.hot > 0.0 { 2.0 * entry.hot } else { 1.0 });
    let input = |stimulus: usize, i: usize| -> f32 {
        bias + match stimulus {
            0 => 0.0,
            1 => hot * (std::f64::consts::TAU * 1_000.0 * i as f64 / f64::from(rate)).sin() as f32,
            2 => hot,
            _ => {
                if i == 0 {
                    hot
                } else {
                    0.0
                }
            }
        }
    };
    let (allocations, ()) = allocations_in(|| {
        for (stimulus, s) in stats.iter_mut().enumerate() {
            if stimulus > 0 {
                subject.stage().reset();
            }
            for start in (0..n).step_by(BLOCK) {
                for (j, x) in block.iter_mut().enumerate() {
                    *x = input(stimulus, start + j);
                }
                subject.stage().process(&mut block);
                for &y in &block {
                    if !y.is_finite() {
                        s.1 += 1;
                    } else {
                        s.0 = s.0.max(y.abs());
                        if y.is_subnormal() {
                            s.2 += 1;
                        }
                    }
                }
            }
        }
    });
    if allocations > 0 {
        out.failures.push(format!("{allocations} allocations after prepare at {rate} Hz"));
    }
    for (stimulus, (peak, nonfinite, subnormal, _)) in stats.iter().enumerate() {
        let what = ["silence", "hot sine", "DC", "impulse"][stimulus];
        if *nonfinite > 0 {
            out.failures.push(format!("{nonfinite} non-finite samples under {what} at {rate} Hz"));
        }
        if *subnormal > 0 {
            out.failures.push(format!("{subnormal} subnormal samples under {what} at {rate} Hz"));
        }
        if *peak > entry.bound {
            out.failures.push(format!("peak {peak:e} past its bound {:e} under {what} at {rate} Hz", entry.bound));
        }
        if out.refused && *peak != 0.0 {
            out.failures.push(format!("refused {rate} Hz, yet output {peak:e} under {what}"));
        }
    }
    out
}

/// One rate's reading of an entry (dB, dBc, Hz).
#[derive(Clone, Debug, Default)]
struct Reading {
    gains: Vec<f64>,
    harmonic: f64,
    level: f64,
    crossings: f64,
}

const PROBES: [f64; 3] = [100.0, 1_000.0, 5_000.0];
/// A level's multisine: 64 tones from 50 Hz to 10 kHz, log-spaced on
/// whole hertz (whole cycles in the second captured), dense enough that
/// a reverb's or a comb's peaks and notches average out.
fn multisine() -> Vec<f64> {
    (0..64).map(|k| (50.0 * 200f64.powf(f64::from(k) / 63.0)).round()).collect()
}
const HOT_HZ: f64 = 200.0;

/// Run a fresh prepared `entry` at `rate` on `input(t)` for the warm-up
/// and `seconds` more; the capture after the warm-up.
fn capture(entry: &Entry, rate: f32, seconds: f64, input: impl Fn(f64) -> f64) -> Vec<f32> {
    let mut subject = (entry.build)();
    let stage = subject.stage();
    stage.prepare(rate);
    let warm = (WARM_SECONDS * f64::from(rate)).round() as usize;
    let n = (seconds * f64::from(rate)).round() as usize;
    let mut buf: Vec<f32> = (0..warm + n).map(|i| entry.bias + input(i as f64 / f64::from(rate)) as f32).collect();
    for block in buf.chunks_mut(BLOCK) {
        stage.process(block);
    }
    buf.split_off(warm)
}

fn rms_db(x: &[f32]) -> f64 {
    let mean = x.iter().map(|&v| f64::from(v)).sum::<f64>() / x.len() as f64;
    let power = x.iter().map(|&v| (f64::from(v) - mean).powi(2)).sum::<f64>() / x.len() as f64;
    10.0 * power.max(1e-30).log10()
}

fn read(entry: &Entry, rate: f32) -> Reading {
    let r = f64::from(rate);
    let tau = std::f64::consts::TAU;
    let small = f64::from(entry.small);
    match entry.measure {
        Measure::Response => {
            let y = capture(entry, rate, RESPONSE_SECONDS, |t| PROBES.iter().map(|f| small * (tau * f * t).sin()).sum());
            let gains = PROBES.iter().map(|f| db(goertzel(&y, f / r) / small)).collect();
            let hot = f64::from(entry.hot);
            let y = capture(entry, rate, RESPONSE_SECONDS, |t| hot * (tau * HOT_HZ * t).sin());
            let fundamental = goertzel(&y, HOT_HZ / r).max(1e-30);
            let harmonic = [2.0, 3.0]
                .iter()
                .map(|k| db(goertzel(&y, k * HOT_HZ / r) / fundamental))
                .fold(-100.0f64, f64::max);
            Reading { gains, harmonic, ..Reading::default() }
        }
        Measure::Level => {
            let tones = multisine();
            // Each tone at an eighth of `small`'s power: the sum as loud as
            // eight tones at `small`.
            let a = small / 8.0f64.sqrt();
            let x = |t: f64| tones.iter().enumerate().map(|(k, f)| a * (tau * f * t + k as f64).sin()).sum::<f64>();
            let y = capture(entry, rate, LEVEL_SECONDS, x);
            let input: Vec<f32> = (0..y.len()).map(|i| x(i as f64 / r) as f32).collect();
            Reading { level: rms_db(&y) - rms_db(&input), ..Reading::default() }
        }
        Measure::Energy => {
            // σ = 50 µs: a band to about 8 kHz, the same pulse at every
            // rate, 5 ms after the warm-up.
            let (centre, sigma) = (WARM_SECONDS + 0.005, 50e-6);
            let pulse = |t: f64| 0.5 * f64::from(entry.hot) * (-0.5 * ((t - centre) / sigma).powi(2)).exp();
            let y = capture(entry, rate, LEVEL_SECONDS, pulse);
            let warm = (WARM_SECONDS * r).round();
            let energy = |v: &mut dyn Iterator<Item = f64>| v.map(|x| x * x).sum::<f64>().max(1e-300);
            let input = energy(&mut (0..y.len()).map(|i| pulse((warm + i as f64) / r)));
            let output = energy(&mut y.iter().map(|&v| f64::from(v) - f64::from(entry.bias)));
            Reading { level: 10.0 * (output / input).log10(), ..Reading::default() }
        }
        Measure::Random => Reading { level: rms_db(&capture(entry, rate, LEVEL_SECONDS, |_| 0.0)), ..Reading::default() },
        Measure::Generator => {
            let y = capture(entry, rate, LEVEL_SECONDS, |_| 0.0);
            let mean = y.iter().map(|&v| f64::from(v)).sum::<f64>() / y.len() as f64;
            let flips = y.windows(2).filter(|w| (f64::from(w[0]) >= mean) != (f64::from(w[1]) >= mean)).count();
            Reading { level: rms_db(&y), crossings: flips as f64 / LEVEL_SECONDS / 2.0, ..Reading::default() }
        }
    }
}

/// How far an entry's readings strayed from 48 kHz over some rates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spread {
    /// dB: the largest small-signal gain change (or level change, for a
    /// level or generator reading).
    response: f64,
    /// dB: the largest change of the level-dependent harmonic.
    harmonic: f64,
    /// Fraction: the largest change of a generator's crossing rate.
    pitch: f64,
}

fn spread(reference: &Reading, readings: &[Reading]) -> Spread {
    let mut s = Spread::default();
    for r in readings {
        for (a, b) in r.gains.iter().zip(&reference.gains) {
            s.response = s.response.max((a - b).abs());
        }
        s.response = s.response.max((r.level - reference.level).abs());
        s.harmonic = s.harmonic.max((r.harmonic - reference.harmonic).abs());
        // A pitch is read where it crosses often enough to count.
        if reference.crossings >= 20.0 {
            s.pitch = s.pitch.max((r.crossings / reference.crossings - 1.0).abs());
        }
    }
    s
}

/// Each entry's budget: (name, response dB, harmonic dB, pitch). Derived
/// from T1's spread (the widest reading; the build host, 2026-11-13)
/// with headroom: the measured value plus half again, and at least 0.05
/// dB more (response), 1 dB more (harmonic) or 0.5 % more (pitch). The
/// T1 run prints each spread beside its budget; `STAVE_RATE_DETAIL`
/// prints each rate's reading.
///
/// What the larger ones are (all measured, none a failure):
/// - circuits at 1×, against 48 kHz at 44.1: the bilinear transform's
///   warping of their highest poles (JCM800 2203 2.8 dB at 100 Hz, its
///   bright cap; DS-1 1.7; the ladders and the 904-A ~1; the power
///   supply's 5 kHz ripple filter 1.3) — R12's shipping-rate error, which
///   oversampling removes;
/// - the BLT-designed filters' stopbands at 5 kHz (LR4 1 dB, SVF, the
///   Sallen-Key and Korg-35 0.6, Formants 0.9) and ADAA's half-sample
///   average at 5 kHz (tanh curve, triode, TS808 ~0.45);
/// - floors: a harmonic near −100 dBc moves by tens of dB on nothing (the
///   true-peak limiter's 25.7, the BBD's 18);
/// - random processes (OU 2.6, Wiener 3.3 dB): one realisation a rate.
const BUDGETS: &[(&str, f64, f64, f64)] = &[
    // catalog
    ("filter/peak", 0.067, 1.0, 0.005),
    ("filter/lowpass", 0.052, 1.0, 0.005),
    ("coupler", 0.05, 1.0, 0.005),
    ("slew", 0.05, 1.0, 0.005),
    ("clipper/diode", 0.06, 1.1, 0.005),
    ("circuit/diode_clipper", 0.15, 1.0, 0.005),
    ("circuit/opamp_clipper", 0.055, 1.0, 0.005),
    ("circuit/transformer", 0.05, 1.0, 0.005),
    ("circuit/common_cathode", 0.05, 1.0, 0.005),
    ("hysteresis", 0.428, 1.01, 0.005),
    // references
    ("circuit/ts808", 0.669, 1.0, 0.005),
    ("circuit/ts808_lfo", 0.536, 1.0, 0.005),
    ("circuit/klon_centaur", 0.627, 1.0, 0.005),
    ("circuit/mxr_distortion_plus", 0.053, 1.0, 0.005),
    ("circuit/boss_ds1", 2.738, 1.1, 0.005),
    ("circuit/proco_rat", 0.681, 1.0, 0.005),
    ("circuit/big_muff", 0.684, 1.0, 0.005),
    ("circuit/fuzz_face", 0.194, 1.01, 0.005),
    ("circuit/rangemaster", 0.087, 1.0, 0.005),
    ("circuit/phase90", 0.488, 1.0, 0.005),
    ("reverb/plate_emt140", 0.06, 1.09, 0.005),
    ("rotary/leslie_122h", 0.13, 1.0, 0.005),
    // Its flutter on (depth 1): the wow's draw moves with the rate.
    ("tape/chowdhury", 2.838, 4.935, 0.005),
    ("dynamics/opto", 0.05, 1.03, 0.005),
    ("converter/sampler", 0.06, 2.97, 0.005),
    ("quantise/dither", 0.134, 2.32, 0.005),
    ("cabinet/close_mic", 0.39, 1.0, 0.005),
    ("instrument/string", 0.063, 1.01, 0.005),
    ("instrument/drum", 0.077, 1.94, 0.005),
    ("circuit/jcm800_2203", 1.155, 1.0, 0.005),
    ("circuit/moog_904a", 1.557, 1.0, 0.005),
    ("filter/ladder", 1.658, 1.0, 0.005),
    // circuits
    ("DiodeClipper (newton)", 0.15, 1.0, 0.005),
    ("DiodeClipper (wright-omega)", 0.15, 1.0, 0.005),
    ("OpAmpClipper (TS808)", 0.055, 1.0, 0.005),
    ("CommonCathode (Dempwolf)", 0.05, 1.0, 0.005),
    ("PowerSupply (silicon, choke)", 0.062, 1.0, 0.005),
    ("PushPull (EL34)", 0.668, 1.0, 0.005),
    ("PassiveToneStack", 0.107, 1.0, 0.005),
    ("Triode (behavioural)", 0.666, 1.0, 0.005),
    ("Coupler (20 Hz)", 0.05, 1.0, 0.005),
    ("HoltersParker (MN3007, Juno-60)", 0.052, 1.0, 0.005),
    ("BucketBrigade (MN3007, NE570)", 0.061, 18.11, 0.005),
    ("Compander (NE570)", 0.056, 1.01, 0.005),
    ("OptoCompressor (VTL5C3)", 0.05, 1.0, 0.005),
    ("Converter (SP-1200)", 0.061, 2.97, 0.005),
    ("Digital delay (SP-1200 word)", 0.123, 1.0, 0.005),
    ("Circuit<Dk> diode clipper", 0.15, 1.0, 0.005),
    ("Circuit<Wdf> diode clipper", 0.15, 1.0, 0.005),
    ("Circuit<Dk> op-amp clipper", 0.055, 1.0, 0.005),
    ("Circuit<Wdf> op-amp clipper", 0.055, 1.0, 0.005),
    ("Circuit<Dk> common cathode", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> common cathode", 0.05, 1.0, 0.005),
    ("Circuit<Dk> Tube Screamer", 0.239, 1.0, 0.005),
    ("Circuit<Wdf> Tube Screamer", 0.239, 1.0, 0.005),
    ("Circuit<Dk> op-amp slew", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> op-amp slew", 0.05, 1.0, 0.005),
    ("Circuit<Dk> OTA VCA", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> OTA VCA", 0.05, 1.0, 0.005),
    ("Circuit<Dk> EL34 push-pull", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> EL34 push-pull", 0.05, 1.0, 0.005),
    ("Circuit<Dk> power supply", 1.941, 1.01, 0.005),
    ("Circuit<Wdf> power supply", 1.941, 1.01, 0.005),
    ("Circuit<Dk> fuzz face", 0.12, 1.02, 0.005),
    ("Circuit<Wdf> fuzz face", 0.141, 1.02, 0.005),
    ("Circuit<Dk> rangemaster", 0.088, 1.0, 0.005),
    ("Circuit<Wdf> rangemaster", 0.098, 1.0, 0.005),
    ("Circuit<Dk> CE stage", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> CE stage", 0.05, 1.0, 0.005),
    ("Circuit<Dk> JFET booster", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> JFET booster", 0.05, 1.0, 0.005),
    ("Circuit<Dk> MOSFET booster", 0.05, 1.0, 0.005),
    ("Circuit<Wdf> MOSFET booster", 0.05, 1.0, 0.005),
    ("Circuit<Dk> Big Muff", 0.684, 1.0, 0.005),
    ("Circuit<Wdf> Big Muff", 0.684, 1.0, 0.005),
    ("Circuit<Dk> TS-808", 0.669, 1.0, 0.005),
    ("Circuit<Wdf> TS-808", 0.669, 1.0, 0.005),
    ("Circuit<Dk> ProCo RAT", 0.462, 1.0, 0.005),
    ("Circuit<Wdf> ProCo RAT", 0.462, 1.0, 0.005),
    ("Circuit<Dk> Boss DS-1", 2.526, 1.08, 0.005),
    ("Circuit<Wdf> Boss DS-1", 2.526, 1.08, 0.005),
    ("Circuit<Dk> JCM800 2203", 4.245, 1.0, 0.005),
    ("Circuit<Wdf> JCM800 2203", 4.245, 1.0, 0.005),
    ("Circuit<Dk> Klon Centaur", 0.627, 1.0, 0.005),
    ("Circuit<Wdf> Klon Centaur", 0.627, 1.0, 0.005),
    ("Circuit<Dk> Moog 904-A", 1.572, 1.0, 0.005),
    ("Circuit<Wdf> Moog 904-A", 1.572, 1.0, 0.005),
    ("Circuit<Dk> MXR Phase 90", 0.055, 1.0, 0.005),
    ("Circuit<Wdf> MXR Phase 90", 0.055, 1.0, 0.005),
    ("Circuit<Dk> CE-2 filters", 0.404, 1.0, 0.005),
    ("Circuit<Wdf> CE-2 filters", 0.404, 1.0, 0.005),
    ("Circuit<Dk> Big Muff tone", 0.091, 1.0, 0.005),
    ("Circuit<Wdf> Big Muff tone", 0.091, 1.0, 0.005),
    ("Circuit<Dk> AB763 tone stack", 0.11, 1.0, 0.005),
    ("Circuit<Wdf> AB763 tone stack", 0.11, 1.0, 0.005),
    ("Circuit<Dk> transistor ladder", 1.722, 1.02, 0.005),
    ("Circuit<Wdf> transistor ladder", 1.722, 1.02, 0.005),
    ("Circuit<Dk> diode ladder", 1.5, 1.0, 0.005),
    ("Circuit<Wdf> diode ladder", 1.5, 1.0, 0.005),
    // learned
    ("LSTM (NAM 1 x 16, interpolated)", 0.077, 1.0, 0.005),
    ("GRU (1 x 16, interpolated)", 0.106, 1.0, 0.005),
    ("WienerHammerstein (spline)", 1.399, 1.0, 0.005),
    ("ParallelHammerstein (order 5)", 1.742, 1.0, 0.005),
    // physical
    ("Plate (EMT 140, to 5 kHz)", 0.05, 1.0, 0.005),
    ("Chirp (100 sections)", 0.05, 1.0, 0.005),
    ("Spring", 0.053, 1.0, 0.005),
    ("Tank (3 springs)", 0.097, 1.0, 0.005),
    ("Radiator (horn, tremolo)", 0.075, 1.0, 0.005),
    ("Cabinet (Leslie 122H, chorale)", 0.073, 1.0, 0.005),
    ("Chowdhury tape (TC-260)", 0.052, 1.0, 0.005),
    ("Recording (TC-260, 16x)", 0.063, 1.0, 0.005),
    ("Playback (7.5 ips)", 0.05, 1.0, 0.005),
    ("Flutter (Akai 4000D)", 0.065, 1.0, 0.005),
    // effects
    ("Peak detector", 0.061, 1.0, 0.005),
    ("Rms detector", 0.05, 1.0, 0.005),
    ("ProgramDependent detector", 0.326, 1.0, 0.005),
    ("Sidechain (HP + LP)", 0.177, 1.0, 0.005),
    ("Compressor (feed-forward)", 0.05, 1.0, 0.005),
    ("Compressor (feedback)", 0.05, 1.01, 0.005),
    ("Limiter (true peak)", 0.05, 25.73, 0.005),
    ("Limiter (sample peak)", 0.05, 1.53, 0.005),
    ("Crossover (LR8, 4 bands)", 0.05, 1.0, 0.005),
    ("Crossover (linear phase, 4 bands)", 0.05, 1.0, 0.005),
    ("Multiband (LR8, compressors)", 0.05, 1.0, 0.005),
    ("MinimumPhase EQ (5 bands)", 0.077, 1.0, 0.005),
    ("LinearPhase EQ (5 bands)", 0.077, 1.0, 0.005),
    ("Dynamic EQ", 0.086, 1.0, 0.005),
    ("PhaseVocoder +7 st (Prusa)", 0.062, 1.0, 0.005),
    ("PhaseVocoder +7 st (Laroche-Dolson)", 0.064, 1.0, 0.005),
    ("Psola +7 st", 0.05, 1.0, 0.005),
    ("Granulator", 0.065, 1.0, 0.005),
    ("FrequencyShifter (+100 Hz)", 0.05, 1.0, 0.005),
    ("Fdn (Householder)", 0.06, 1.0, 0.005),
    ("Fdn (Hadamard)", 0.058, 1.0, 0.005),
    ("Fdn (Kronecker, modulated)", 0.058, 1.0, 0.005),
    ("VelvetNoise", 0.109, 1.0, 0.005),
    ("Dattorro (plate)", 0.078, 1.0, 0.005),
    ("Schroeder", 0.143, 1.0, 0.005),
    ("Schroeder::moorer", 0.084, 1.0, 0.005),
    // primitives
    ("Biquad (peak)", 0.067, 1.0, 0.005),
    ("Allpass1", 0.05, 1.0, 0.005),
    ("OnePole (lowpass)", 0.218, 1.0, 0.005),
    ("DcBlocker", 0.055, 1.0, 0.005),
    ("Svf", 0.86, 1.0, 0.005),
    ("LinkwitzRiley4 (lowpass)", 1.476, 1.0, 0.005),
    ("Hilbert", 0.05, 1.0, 0.005),
    ("Ladder (Huovilainen)", 1.657, 1.03, 0.005),
    ("Ladder (Stinchcombe)", 1.557, 1.04, 0.005),
    ("SallenKey", 0.878, 1.0, 0.005),
    ("Korg35", 0.878, 1.04, 0.005),
    ("Formants (aa, man)", 1.289, 1.0, 0.005),
    ("Resonator (formant)", 0.438, 1.0, 0.005),
    ("CurveStage (tanh)", 0.668, 1.0, 0.005),
    ("SmoothSat", 0.05, 1.0, 0.005),
    ("Hysteresis", 0.428, 1.05, 0.005),
    ("Slew", 0.05, 1.0, 0.005),
    ("NonlinearRc (diode)", 0.074, 1.12, 0.005),
    ("Oversampled 4x (SmoothSat, linear)", 0.05, 1.0, 0.005),
    ("Oversampled 2x (SmoothSat, minimum)", 0.05, 1.0, 0.005),
    ("Quantiser (16-bit TPDF)", 0.05, 1.0, 0.005),
    ("Delay<Lagrange3> (7.3 ms)", 0.084, 1.0, 0.005),
    ("Delay<Sinc<16>> (7.3 ms)", 0.055, 1.0, 0.005),
    ("Comb (feedback)", 0.05, 1.0, 0.005),
    ("DelayAllpass", 0.05, 1.0, 0.005),
    ("Comb resonator", 0.106, 1.0, 0.005),
    ("KarplusStrong", 0.05, 1.0, 0.005),
    ("Bank (16 modes)", 0.05, 1.0, 0.005),
    ("Convolver (decaying noise, uniform)", 0.339, 1.0, 0.005),
    ("Convolver (damped 1 kHz resonance, non-uniform)", 2.189, 1.0, 0.005),
    ("Fir (Kaiser lowpass)", 0.05, 1.0, 0.005),
    ("Stft (1024/256 Hann, identity)", 0.05, 1.0, 0.005),
    ("EnvelopeFollower", 0.068, 1.0, 0.005),
    ("Lag", 0.216, 1.0, 0.005),
    ("SubOctave", 0.052, 1.0, 0.005),
    ("Osc (sine)", 0.05, 1.0, 0.0073),
    ("Osc (saw, polyBLEP)", 0.092, 1.0, 0.0073),
    ("Osc (triangle)", 0.052, 1.0, 0.005),
    ("Osc (saw, minBLEP)", 0.07, 1.0, 0.005),
    ("Osc (saw, hard sync)", 0.122, 1.0, 0.005),
    ("Wavetable (saw)", 0.081, 1.0, 0.0073),
    ("Lfo (1.3 Hz)", 0.056, 1.0, 0.005),
    ("ModLfo", 0.061, 1.0, 0.005),
    ("RandomWalk", 0.05, 1.0, 0.005),
    ("Adsr (held)", 0.05, 1.0, 0.005),
    ("OrnsteinUhlenbeck", 2.592, 1.0, 0.005),
    ("Wiener", 3.308, 1.0, 0.005),
    // H10, E9, H7 (measured after they landed)
    ("String (110 Hz, driven)", 0.052, 1.0, 0.005),
    ("Membrane (110 Hz tom)", 0.054, 1.0, 0.005),
    ("Bar (880 Hz, free)", 0.054, 1.0, 0.005),
    ("Tube (clarinet, reed, blown)", 1.101, 1.0, 0.005),
    ("Driver (Jensen C12N, linear)", 0.194, 1.0, 0.005),
    ("Driver (Jensen C12N, large signal)", 0.194, 1.0, 0.005),
    ("Captured (one position)", 0.2, 1.0, 0.005),
    ("CloseMic (cardioid, 2.5 cm)", 0.39, 1.0, 0.005),
    ("Room (live room)", 1.298, 1.0, 0.005),
];

fn budget(name: &str) -> Option<Spread> {
    BUDGETS
        .iter()
        .find(|b| b.0 == name)
        .map(|&(_, response, harmonic, pitch)| Spread { response, harmonic, pitch })
}

/// Run `job` over every entry on the test's threads; panics caught and
/// returned as failures, in library order. The slowest entries are
/// printed (where the run's time goes).
fn each<T: Send>(entries: &[Entry], job: impl Fn(&Entry) -> T + Sync) -> Vec<Result<T, String>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<T, String>>>> = Mutex::new((0..entries.len()).map(|_| None).collect());
    let seconds: Mutex<Vec<(f64, usize)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..stave_test::threads().min(entries.len()).max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(entry) = entries.get(i) else { break };
                let started = std::time::Instant::now();
                let r = catch_unwind(AssertUnwindSafe(|| job(entry))).map_err(|p| {
                    ARMED.with(|a| a.set(false));
                    p.downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default()
                });
                seconds.lock().unwrap().push((started.elapsed().as_secs_f64(), i));
                results.lock().unwrap()[i] = Some(r);
            });
        }
    });
    let mut seconds = seconds.into_inner().unwrap();
    seconds.sort_by(|a, b| b.0.total_cmp(&a.0));
    let slowest: Vec<String> = seconds.iter().take(6).map(|&(t, i)| format!("{} {t:.1} s", entries[i].name)).collect();
    println!("  slowest: {}", slowest.join(", "));
    results.into_inner().unwrap().into_iter().map(Option::unwrap).collect()
}

fn robustness(rates: &[f32]) {
    let entries = library();
    let started = std::time::Instant::now();
    let results = each(&entries, |e| rates.iter().map(|&r| robust(e, r)).collect::<Vec<_>>());
    let mut failures = Vec::new();
    let mut refusals = Vec::new();
    for (e, r) in entries.iter().zip(results) {
        match r {
            Err(p) => failures.push(format!("{} / {}: panicked: {p}", e.family, e.name)),
            Ok(runs) => {
                for (rate, run) in rates.iter().zip(runs) {
                    if run.refused {
                        refusals.push(format!("{} @ {rate}", e.name));
                    }
                    failures.extend(run.failures.into_iter().map(|f| format!("{} / {}: {f}", e.family, e.name)));
                }
            }
        }
    }
    println!(
        "  {} stages × {} rates in {:.1} s; expected refusals (rate policy): {}",
        entries.len(),
        rates.len(),
        started.elapsed().as_secs_f64(),
        if refusals.is_empty() { "none".into() } else { refusals.join(", ") }
    );
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty(), "{} robustness failures (above)", failures.len());
}

fn invariance(rates: &[f32], tier: Tier) {
    // STAVE_RATE_DETAIL=<part of a name>[|<another>…]: only the stages it names, each
    // rate's reading printed.
    let detail = std::env::var("STAVE_RATE_DETAIL").ok().filter(|d| !d.is_empty());
    let entries: Vec<Entry> = library()
        .into_iter()
        .filter(|e| detail.as_ref().is_none_or(|d| d.split('|').any(|p| e.name.contains(p))))
        .filter(|e| tier == Tier::T1 || !e.heavy)
        .collect();
    let started = std::time::Instant::now();
    let results = each(&entries, |e| {
        if std::iter::once(REFERENCE).chain(rates.iter().copied()).any(e.refuses) {
            return None;
        }
        let reference = read(e, REFERENCE);
        let readings: Vec<Reading> = rates.iter().map(|&r| read(e, r)).collect();
        if detail.is_some() {
            for (rate, r) in std::iter::once(&REFERENCE).chain(rates).zip(std::iter::once(&reference).chain(&readings)) {
                println!("  {} @ {rate}: {r:?}", e.name);
            }
        }
        Some((spread(&reference, &readings), reference))
    });
    let mut failures = Vec::new();
    println!("  {:<12} {:<44} {:>9} {:>9} {:>8}   budget", "family", "stage", "response", "harmonic", "pitch");
    for (e, r) in entries.iter().zip(results) {
        let (s, reference) = match r {
            Err(p) => {
                failures.push(format!("{}: panicked: {p}", e.name));
                continue;
            }
            Ok(None) => {
                println!("  {:<12} {:<44} refuses rates other than its own (rate policy)", e.family, e.name);
                continue;
            }
            Ok(Some(x)) => x,
        };
        let b = budget(&e.name);
        println!(
            "  {:<12} {:<44} {:>9.3} {:>9.2} {:>8.4}   {} (48 kHz harmonic {:.1} dBc)",
            e.family,
            e.name,
            s.response,
            s.harmonic,
            s.pitch,
            b.map_or("none".into(), |b| format!("{:.3} / {:.2} / {:.4}", b.response, b.harmonic, b.pitch)),
            reference.harmonic,
        );
        match b {
            None => failures.push(format!("{}: no budget", e.name)),
            Some(b) => {
                if !(s.response <= b.response && s.harmonic <= b.harmonic && s.pitch <= b.pitch) {
                    failures.push(format!("{}: spread {s:?} past budget {b:?}", e.name));
                }
            }
        }
    }
    println!("  {} stages × {} rates in {:.1} s", entries.len(), rates.len() + 1, started.elapsed().as_secs_f64());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty(), "{} invariance failures (above)", failures.len());
}

#[test]
#[cfg_attr(miri, ignore)]
fn every_stage_is_robust_at_t0_rates() {
    robustness(&T0_ROBUST);
}

#[test]
#[cfg_attr(miri, ignore)]
fn every_stage_is_rate_invariant_at_t0_rates() {
    invariance(&T0_INVARIANT, Tier::T0);
}

#[test]
#[ignore = "T1: deep.yml's rate-range job runs it"]
fn every_stage_is_robust_from_8_to_768_khz() {
    robustness(&T1_ROBUST);
}

#[test]
#[ignore = "T1: deep.yml's rate-range job runs it"]
fn every_stage_is_rate_invariant_from_44_1_to_192_khz() {
    invariance(&T1_INVARIANT, Tier::T1);
}

/// Every entry has a budget and every budget an entry: a stage added to
/// the library lands with its measured budget.
#[test]
fn budgets_and_library_agree() {
    let entries = library();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    for n in &names {
        assert!(seen.insert(*n), "{n} is in the library twice");
    }
    for (name, ..) in BUDGETS {
        assert!(names.contains(name), "budget for {name}, no such stage");
    }
}
