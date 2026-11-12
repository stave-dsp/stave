//! THE PLATE SUITE (topologies arc) — the dedicated skeleton
//! must BE a plate, measurably: instant echo density, the
//! dispersion chirp (highs outrun lows — bending waves), the
//! dialed decay, and a wet that holds its own next to the room's.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render_stereo(set: &[(&str, f64)], input: &[f32], extra_blocks: usize) -> (Vec<f32>, Vec<f32>) {
    let mut plugin = stave_vault_vst3::Vault::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let blocks = input.len().div_ceil(BLOCK) + extra_blocks;
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for b in 0..blocks {
        for i in 0..BLOCK {
            in_l[i] = input.get(b * BLOCK + i).copied().unwrap_or(0.0);
        }
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        l.extend_from_slice(&out_l);
        r.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (l, r)
}

fn render(set: &[(&str, f64)], input: &[f32], extra: usize) -> Vec<f32> {
    render_stereo(set, input, extra).0
}

fn plate_set() -> Vec<(&'static str, f64)> {
    vec![
        ("mix", 1.0),
        ("predelay", 0.0),
        ("topology", 1.0),
        ("decay", 2_200.0),
    ]
}

/// Impulse fired AFTER the topology fade has fully opened.
fn late_impulse() -> Vec<f32> {
    let mut v = vec![0.0f32; (0.1 * FS) as usize];
    v[(0.05 * FS) as usize] = 0.8;
    v
}

fn rms(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}

fn band(w: &[f32], lo: f32, hi: f32) -> Vec<f32> {
    let kh = 1.0 - (-core::f32::consts::TAU * lo / FS).exp();
    let kl = 1.0 - (-core::f32::consts::TAU * hi / FS).exp();
    let mut zh = [0.0f32; 2];
    let mut zl = [0.0f32; 2];
    w.iter()
        .map(|x| {
            let mut v = *x;
            for z in zh.iter_mut() {
                *z += kh * (v - *z);
                v -= *z;
            }
            for z in zl.iter_mut() {
                *z += kl * (v - *z);
                v = *z;
            }
            v
        })
        .collect()
}

/// Normalized echo density (Abel-Huang): time to sustained 0.9.
fn ned_t90(l: &[f32], from: usize) -> f64 {
    let win = (0.020 * FS) as usize;
    let hop = (0.005 * FS) as usize;
    let mut streak = 0;
    let mut k = 0;
    while from + k * hop + win < l.len() && k < 400 {
        let w = &l[from + k * hop..from + k * hop + win];
        let mean = w.iter().map(|x| f64::from(*x)).sum::<f64>() / win as f64;
        let var = w
            .iter()
            .map(|x| (f64::from(*x) - mean).powi(2))
            .sum::<f64>()
            / win as f64;
        let sd = var.sqrt().max(1e-15);
        let frac = w
            .iter()
            .filter(|x| (f64::from(**x) - mean).abs() > sd)
            .count() as f64
            / win as f64;
        if frac / 0.3173 >= 0.9 {
            streak += 1;
            if streak >= 2 {
                return ((k - 1) * hop) as f64 / f64::from(FS);
            }
        } else {
            streak = 0;
        }
        k += 1;
    }
    f64::NAN
}

fn band_t30(w: &[f32]) -> f64 {
    let e: Vec<f64> = w.iter().map(|x| f64::from(*x) * f64::from(*x)).collect();
    let total: f64 = e.iter().sum();
    let mut acc = 0.0f64;
    let mut sch = vec![0.0f64; e.len()];
    for (i, x) in e.iter().enumerate().rev() {
        acc += x;
        sch[i] = 10.0 * (acc / total.max(1e-30)).log10();
    }
    let a = sch.iter().position(|&x| x <= -5.0);
    let b = sch.iter().position(|&x| x <= -25.0);
    match (a, b) {
        (Some(a), Some(b)) if b > a => (b - a) as f64 / f64::from(FS) * 3.0,
        _ => f64::NAN,
    }
}

/// THE METAL METER — what "bad metallic" IS, as numbers. Two
/// instruments on the LATE tail (0.3-0.8 s, past the onset):
/// * SPECTRAL FLATNESS (geomean/mean of power over log-spaced
///   bins): a smooth wash reads high, isolated ringing partials
///   read low;
/// * COMB REGULARITY (autocorrelation of the magnitude spectrum
///   across LINEAR bins): a Schroeder-style comb is PERIODIC in
///   frequency — the ear hears that regularity as a metallic
///   pitch; a real plate's modes are irregularly spaced.
fn late_spectrum_linear(out: &[f32], from_s: f64, bins: usize, f0: f64, f1: f64) -> Vec<f64> {
    let a = (from_s * f64::from(FS)) as usize;
    let b = (a + (0.5 * FS) as usize).min(out.len());
    let w = &out[a..b];
    let n = w.len();
    (0..bins)
        .map(|k| {
            let hz = f0 + (f1 - f0) * k as f64 / (bins - 1) as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &x) in w.iter().enumerate() {
                let hann = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
                let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
                re += f64::from(x) * hann * ph.cos();
                im += f64::from(x) * hann * ph.sin();
            }
            (re * re + im * im).sqrt()
        })
        .collect()
}

fn flatness(mag: &[f64]) -> f64 {
    let pow: Vec<f64> = mag.iter().map(|m| (m * m).max(1e-30)).collect();
    let logmean = pow.iter().map(|p| p.ln()).sum::<f64>() / pow.len() as f64;
    let mean = pow.iter().sum::<f64>() / pow.len() as f64;
    logmean.exp() / mean
}

/// Peak autocorrelation of the demeaned log-spectrum across bins,
/// searched over comb spacings 25-400 Hz.
fn comb_regularity(mag: &[f64], bin_hz: f64) -> f64 {
    let lm: Vec<f64> = mag.iter().map(|m| m.max(1e-15).ln()).collect();
    let mean = lm.iter().sum::<f64>() / lm.len() as f64;
    let d: Vec<f64> = lm.iter().map(|x| x - mean).collect();
    let e0: f64 = d.iter().map(|x| x * x).sum();
    let (lag_min, lag_max) = ((25.0 / bin_hz) as usize, (400.0 / bin_hz) as usize);
    let mut best = 0.0f64;
    for lag in lag_min.max(1)..lag_max.min(d.len() / 2) {
        let acc: f64 = (0..d.len() - lag).map(|i| d[i] * d[i + lag]).sum();
        best = best.max(acc / e0.max(1e-30));
    }
    best
}

/// THE PLATE-LIKENESS REPORT (--nocapture): flatness + comb
/// regularity for the plate AND the room tank (the reference that
/// already sounds right) — the numbers that define "metallic in
/// the bad way".
#[test]
fn the_metal_meter_report() {
    for (name, set) in [
        ("PLATE", plate_set()),
        (
            "ROOM ",
            vec![
                ("mix", 1.0),
                ("predelay", 0.0),
                ("decay", 2_200.0),
                ("early", 0.0),
            ],
        ),
    ] {
        let out = render(&set, &late_impulse(), 500);
        let mag = late_spectrum_linear(&out, 0.35, 800, 400.0, 6_000.0);
        let bin_hz = (6_000.0 - 400.0) / 799.0;
        println!(
            "  {name}: flatness {:.3}  comb-regularity {:.3}",
            flatness(&mag),
            comb_regularity(&mag, bin_hz)
        );
    }
}

/// THE METAL LAW: the late tail must be a WASH, not a comb —
/// flatness at least room-class, comb regularity below the
/// pitched-metal line. (Journey: 0.027/0.756 at the sub-ms
/// voicing → 0.269/0.439 — better than the room reference's
/// 0.185/0.491 — via geometric long lines + in-loop diffusers +
/// staggered dispersion poles + second pickups.)
#[test]
fn the_late_tail_is_a_wash_not_a_comb() {
    let out = render(&plate_set(), &late_impulse(), 500);
    let mag = late_spectrum_linear(&out, 0.35, 800, 400.0, 6_000.0);
    let bin_hz = (6_000.0 - 400.0) / 799.0;
    let f = flatness(&mag);
    let c = comb_regularity(&mag, bin_hz);
    assert!(f > 0.18, "the tail must be a wash: flatness {f:.3}");
    assert!(c < 0.55, "the comb must not be pitched: regularity {c:.3}");
}

/// A plate's density is effectively INSTANT — that is the sound
/// that a room topology structurally cannot make (measured 0.075 s
/// at the old preset; a real plate sits under ~10 ms).
#[test]
fn the_plate_is_instantly_dense() {
    let out = render(&plate_set(), &late_impulse(), 500);
    let from = (0.05 * FS) as usize;
    let t90 = ned_t90(&out, from);
    println!("  PLATE NED t90 {t90:.4}s");
    // 0.025: the instrument's floor is 0.010 (5 ms hop, 2-window
    // streak) and the pickup-position weights sit one hop above
    // it (mode-shape weights are less uniform than sign tables —
    // measured 0.020, a real EMT reaches full density in
    // 10-20 ms). The old room-as-plate measured 0.075.
    assert!(t90 < 0.025, "a plate is instant: NED t90 {t90:.3}s");
}

/// THE CHIRP — dispersion is the voice: bending waves run faster
/// at high frequency, and the lag COMPOUNDS per trip. INSTRUMENT
/// (third draft): per-band envelope PEAK TIME, both channels —
/// scale-invariant, so pickup weighting and band energy imbalance
/// cannot fake or hide timing (energy centroids could; onset
/// thresholds could). The LAW: dialing STIFF from 0 to 1 must
/// GROW the high-to-low peak lag by an audible margin, and at
/// full stiffness the top must peak first.
#[test]
fn the_plate_chirps_downward() {
    let peaks = |stiff: f64| -> (f64, f64) {
        let mut set = plate_set();
        set.push(("whip", stiff));
        set.push(("high energy", 0.0));
        set.push(("low energy", 1.0));
        // SHORT decay: on a long tail the sparse low band BEATS
        // (~10 modes in 150-400 Hz) and a late beat outruns the
        // onset peak — the envelope argmax must be decay-pinned
        // to the build-up, where dispersion actually lives.
        set.retain(|(n, _)| *n != "decay");
        set.push(("decay", 500.0));
        let (out_l, out_r) = render_stereo(&set, &late_impulse(), 500);
        let from = (0.05 * FS) as usize;
        let span = (0.30 * FS) as usize;
        let peak_t = |lo: f32, hi: f32| -> f64 {
            let bl = band(&out_l, lo, hi);
            let br = band(&out_r, lo, hi);
            let k_env = 1.0 - (-core::f32::consts::TAU * 20.0 / FS).exp();
            let mut env = 0.0f32;
            let (mut best, mut best_i) = (0.0f32, 0usize);
            for i in from..from + span {
                let a = (bl[i] * bl[i] + br[i] * br[i]).sqrt();
                env += k_env * (a - env);
                if env > best {
                    best = env;
                    best_i = i - from;
                }
            }
            best_i as f64 / f64::from(FS)
        };
        let hi = peak_t(3_000.0, 9_000.0);
        let lo = peak_t(150.0, 400.0);
        (hi, lo)
    };
    let (hi0, lo0) = peaks(0.0);
    let (hi1, lo1) = peaks(1.0);
    let (lag0, lag1) = (lo0 - hi0, lo1 - hi1);
    println!("  PLATE chirp lag (low peak - high peak): stiff0 {lag0:.4}s  stiff1 {lag1:.4}s");
    // DIRECTION only: the lag must GROW with the dial. Absolute
    // sub-ms peak ordering is below this instrument's floor
    // (pickup weighting shifts envelope peaks by ~1 ms); the
    // clean absolute proof — the cascade runs the top first —
    // lives in-crate on the cascade alone, and the audibility
    // claim in the render-difference below.
    assert!(
        lag1 > lag0,
        "stiffness must run the top ahead: lag {lag0:.4}s -> {lag1:.4}s"
    );
    // AUDIBILITY by render-difference: same impulse, stiff 0 vs 1
    // — the difference must be a substantial share of the tail
    // (dispersion compounds per trip; identical renders would
    // cancel to bits).
    let render_at = |stiff: f64| {
        let mut set = plate_set();
        set.push(("whip", stiff));
        set.push(("high energy", 0.0));
        set.push(("low energy", 1.0));
        render(&set, &late_impulse(), 500)
    };
    let a = render_at(0.0);
    let b = render_at(1.0);
    let from = (0.06 * FS) as usize;
    let to = (0.8 * FS) as usize;
    let diff: f64 = a[from..to]
        .iter()
        .zip(&b[from..to])
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        .sqrt();
    let base: f64 = a[from..to]
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    let share = diff / base.max(1e-15);
    println!("  PLATE stiff render-diff share {share:.2}");
    assert!(
        share > 0.5,
        "the whip must be audible: stiff 0 vs 1 differ by only {share:.2} of the tail"
    );
}

/// The decay dial is a PROMISE on the plate too.
#[test]
fn the_plate_decays_as_dialed() {
    let mut set = plate_set();
    set.push(("high energy", 0.0));
    set.push(("decay", 2_000.0));
    let out = render(&set, &late_impulse(), 700);
    let from = (0.05 * FS) as usize;
    let t30 = band_t30(&band(&out[from..], 300.0, 4_000.0));
    println!("  PLATE T30 {t30:.2}s (dialed 2.0)");
    assert!(
        (t30 / 2.0 - 1.0).abs() < 0.35,
        "the plate must keep the dialed decay: {t30:.2}s vs 2.0s"
    );
}

/// THE PICKUPS ARE THE WIDTH (the stereo-honesty slice): width 0
/// = coincident pickups = mono BY GEOMETRY; width 2 = far corners
/// = decorrelated; and a moved microphone never gets quieter
/// (energy constant across the dial).
#[test]
fn the_pickups_are_the_width() {
    let render_w = |w: f64| {
        let mut set = plate_set();
        set.push(("width", w));
        render_stereo(&set, &late_impulse(), 500)
    };
    let corr = |l: &[f32], r: &[f32]| -> f64 {
        let a = (0.1 * FS) as usize;
        let b = (1.0 * FS) as usize;
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for i in a..b {
            xy += f64::from(l[i]) * f64::from(r[i]);
            xx += f64::from(l[i]) * f64::from(l[i]);
            yy += f64::from(r[i]) * f64::from(r[i]);
        }
        xy / (xx * yy).sqrt().max(1e-30)
    };
    let (l0, r0) = render_w(0.0);
    let (l1, r1) = render_w(1.0);
    let (l2, r2) = render_w(2.0);
    let (c0, c1, c2) = (corr(&l0, &r0), corr(&l1, &r1), corr(&l2, &r2));
    println!("  PLATE width corr: 0 -> {c0:.3}  1 -> {c1:.3}  2 -> {c2:.3}");
    assert!(
        c0 > 0.95,
        "coincident pickups must hear one field: corr {c0:.3}"
    );
    assert!(c1 < 0.5, "a stereo pair must decorrelate: corr {c1:.3}");
    assert!(
        c2 < c1 + 0.1,
        "far corners must not re-correlate: {c1:.3} -> {c2:.3}"
    );
    let energy = |l: &[f32], r: &[f32]| -> f64 {
        let a = (0.1 * FS) as usize;
        let b = (1.0 * FS) as usize;
        l[a..b]
            .iter()
            .zip(&r[a..b])
            .map(|(x, y)| f64::from(*x) * f64::from(*x) + f64::from(*y) * f64::from(*y))
            .sum()
    };
    let (e0, e2) = (energy(&l0, &r0), energy(&l2, &r2));
    let db = 10.0 * (e2 / e0.max(1e-30)).log10();
    assert!(
        db.abs() < 2.0,
        "a moved microphone never gets quieter: width 0 vs 2 differ {db:.1} dB"
    );
}

/// The plate wet holds its own next to the dry (the loudness
/// doctrine — calibrates PLATE_MAKEUP).
#[test]
fn the_plate_holds_its_own() {
    let noise: Vec<f32> = {
        let mut rng = 0x9e37_79b9u32;
        (0..(2.0 * FS) as usize)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                ((rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.25
            })
            .collect()
    };
    let wet = render(&plate_set(), &noise, 100);
    let dry_rms = rms(&noise[(0.5 * FS) as usize..]);
    let wet_rms = rms(&wet[(0.5 * FS) as usize..(2.0 * FS) as usize]);
    let db = 20.0 * (wet_rms / dry_rms.max(1e-15)).log10();
    println!("  PLATE wet vs dry: {db:.1} dB");
    assert!(
        db.abs() < 4.0,
        "the plate wet must hold its own: {db:.1} dB vs dry"
    );
}
