//! Eddy's OWN laws — the tests only this project can write (the
//! rule: a project without them is not done).
//!
//!  1. The echo arrives at the dialed time.
//!  2. Feedback builds the echo train at the dialed ratio.
//!  3. wet = 0 (dry = 1) is BIT-identical dry — the wet path can
//!     never contaminate a parallel chain the user set to dry.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_eddy_vst3::Eddy::new();
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
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

/// Peak position in a window (start, len).
fn peak_in(out: &[f32], start: usize, len: usize) -> (usize, f32) {
    let mut best = (start, 0.0f32);
    for i in start..(start + len).min(out.len()) {
        if out[i].abs() > best.1 {
            best = (i, out[i].abs());
        }
    }
    best
}

#[test]
fn echo_arrives_at_the_dialed_time() {
    // 100 ms at 48 kHz = 4800 samples. Impulse in, full wet, no
    // feedback: the (only) echo lands at the dialed time, ± the
    // interpolator's reach.
    let mut input = vec![0.0f32; 48_000];
    input[0] = 1.0;
    let out = render(
        &[
            ("time", 100.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
        ],
        &input,
    );
    let (pos, amp) = peak_in(&out, 1_000, 40_000);
    let expect = (0.100 * FS) as usize;
    assert!(
        (pos as i64 - expect as i64).unsigned_abs() <= 4,
        "echo at {pos}, dialed {expect}"
    );
    assert!(amp > 0.5, "echo audible: {amp}");
}

#[test]
fn feedback_builds_the_echo_train() {
    // fb = 0.5: repeats at time, 2×time, 3×time, each ≈ half the
    // last (cubic-read losses grant a small tolerance).
    let mut input = vec![0.0f32; 96_000];
    input[0] = 1.0;
    let out = render(
        &[
            ("time", 100.0),
            ("feedback", 0.5),
            ("dry", 0.0),
            ("wet", 1.0),
        ],
        &input,
    );
    let n = (0.100 * FS) as usize;
    let (_, a1) = peak_in(&out, n - 100, 200);
    let (_, a2) = peak_in(&out, 2 * n - 100, 200);
    let (_, a3) = peak_in(&out, 3 * n - 100, 200);
    assert!(a1 > 0.5, "first echo: {a1}");
    let r21 = a2 / a1;
    let r32 = a3 / a2;
    assert!(
        (r21 - 0.5).abs() < 0.08,
        "echo 2/1 ratio {r21} (dialed 0.5)"
    );
    assert!(
        (r32 - 0.5).abs() < 0.08,
        "echo 3/2 ratio {r32} (dialed 0.5)"
    );
}

#[test]
fn wet_at_zero_is_bit_identical_dry() {
    // The parallel-chain contract: wet at 0 with dry at 1 (and trim
    // at 0 dB) may not move ONE BIT of the dry signal, feedback
    // ringing or not.
    let mut input = vec![0.0f32; 24_000];
    let mut lcg: u32 = 0x1234_5678;
    for x in input.iter_mut() {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        *x = ((lcg >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.5;
    }
    let out = render(
        &[
            ("time", 50.0),
            ("feedback", 0.9),
            ("dry", 1.0),
            ("wet", 0.0),
        ],
        &input,
    );
    for (i, (a, b)) in input.iter().zip(out.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "dry bit moved at sample {i}");
    }
}

#[test]
fn depth_wobbles_the_pitch_as_dialed() {
    // Vibrato law: a modulated delay bends pitch by
    // f_dev/f0 = d(delay)/dt, peaking at depth·DEPTH_MS·2π·rate.
    // 440 Hz in, full wet, no feedback, sine motion at 2 Hz, full
    // depth: predicted peak deviation ≈ 18ms·2π·2 ≈ 22.6%. Measure
    // instantaneous frequency by zero-crossing spacing.
    let n = 96_000;
    let mut input = vec![0.0f32; n];
    for (i, x) in input.iter_mut().enumerate() {
        *x = (2.0 * core::f32::consts::PI * 440.0 * i as f32 / FS).sin() * 0.5;
    }
    let out = render(
        &[
            ("time", 30.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("rate", 2.0),
            ("depth", 1.0),
            ("motion", 0.0),
            ("spread", 0.0),
        ],
        &input,
    );
    // Instantaneous period from rising zero-crossing spacing, after
    // the delay fills and the ramps settle.
    let mut periods = Vec::new();
    let mut last_cross = None;
    for i in 24_000..n - 1 {
        if out[i] < 0.0 && out[i + 1] >= 0.0 {
            if let Some(prev) = last_cross {
                periods.push((i - prev) as f32);
            }
            last_cross = Some(i);
        }
    }
    assert!(periods.len() > 200, "enough cycles to measure");
    let base = FS / 440.0;
    let max_dev = periods
        .iter()
        .map(|p| (p - base).abs() / base)
        .fold(0.0f32, f32::max);
    // Predicted ≈ 0.226; crossing quantization is coarse (±1 sample
    // on a ~109-sample period ≈ 1%), so bound it loosely both ways.
    assert!(
        max_dev > 0.15 && max_dev < 0.30,
        "peak pitch deviation {max_dev} (predicted ≈ 0.226)"
    );
}

/// The recipes must LOOK like themselves — the published wave view
/// cycle differs measurably from the raw shape (the bug this law
/// captures: control-rate lag constants sized with audio-rate
/// intuition round nothing and draw nothing).
#[test]
fn recipes_are_visibly_shaped() {
    use core::sync::atomic::Ordering;
    let read_curve = |motion: f64| -> Vec<f32> {
        let mut plugin = stave_eddy_vst3::Eddy::new();
        let bus = DebugBus::new();
        plugin.debug_attach(bus.clone());
        let table = plugin.param_table();
        for (n, v) in [("motion", motion), ("rate", 0.8), ("flavor", 0.5)] {
            table.by_name(n).unwrap().set_plain(v);
        }
        plugin.activate(FS, BLOCK);
        // One block resolves the recipe and publishes the cycle.
        let mut in_l = vec![0.0f32; BLOCK];
        let mut in_r = vec![0.0f32; BLOCK];
        let mut out_l = vec![0.0f32; BLOCK];
        let mut out_r = vec![0.0f32; BLOCK];
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let input = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let output = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input,
            output,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        let n = bus.curve_used.load(Ordering::Acquire) as usize;
        (0..n)
            .map(|i| f32::from_bits(bus.curve[i].load(Ordering::Relaxed)))
            .collect()
    };

    let sine = read_curve(0.0);
    let tri = read_curve(1.0);
    let juno = read_curve(2.0);
    let vibe = read_curve(3.0);
    let rms_diff = |a: &[f32], b: &[f32]| -> f32 {
        let n = a.len().min(b.len()).max(1);
        (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n as f32).sqrt()
    };
    // JUNO is a rounded triangle: clearly not the raw triangle —
    // and at FULL scale (the makeup restores amplitude; flavor buys
    // skew, never level).
    assert!(
        rms_diff(&juno, &tri) > 0.05,
        "JUNO ≠ TRI: {}",
        rms_diff(&juno, &tri)
    );
    let jmax = juno.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(jmax > 0.9, "JUNO keeps full scale after rounding: {jmax}");
    // VIBE is the lamp: clearly not the raw sine, and LOPSIDED —
    // it spends less of the cycle rising than falling.
    assert!(
        rms_diff(&vibe, &sine) > 0.05,
        "VIBE ≠ SINE: {}",
        rms_diff(&vibe, &sine)
    );
    let rising = vibe.windows(2).filter(|w| w[1] > w[0]).count() as f32;
    let ratio = rising / (vibe.len() - 1) as f32;
    assert!(
        ratio < 0.45,
        "the lamp brightens fast and dims slow — rising fraction {ratio}"
    );
}

/// Render stereo (correlated input) and return both channels.
fn render_stereo(set: &[(&str, f64)], input: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut plugin = stave_eddy_vst3::Eddy::new();
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
    let (mut outs_l, mut outs_r) = (Vec::new(), Vec::new());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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
        outs_l.extend_from_slice(&out_l);
        outs_r.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (outs_l, outs_r)
}

/// SPREAD = 0: for a centered input, the two ears are the SAME ear —
/// bit-identical channels at full depth. Mono-safety as a law.
#[test]
fn spread_zero_is_the_same_ear_twice() {
    let mut input = vec![0.0f32; 48_000];
    for (i, x) in input.iter_mut().enumerate() {
        *x = (2.0 * core::f32::consts::PI * 330.0 * i as f32 / FS).sin() * 0.4;
    }
    let (l, r) = render_stereo(
        &[
            ("time", 25.0),
            ("feedback", 0.4),
            ("dry", 0.2),
            ("wet", 0.8),
            ("rate", 3.0),
            ("depth", 1.0),
            ("spread", 0.0),
        ],
        &input,
    );
    for (i, (a, b)) in l.iter().zip(r.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "ears diverged at {i}");
    }
}

/// SPREAD = 1: the phantom center SURVIVES full depth — the summed
/// (mid) signal carries no vibrato while the sides breathe. The law
/// this whole control exists to keep.
#[test]
fn center_survives_full_depth_at_full_spread() {
    let n = 96_000;
    let mut input = vec![0.0f32; n];
    for (i, x) in input.iter_mut().enumerate() {
        *x = (2.0 * core::f32::consts::PI * 440.0 * i as f32 / FS).sin() * 0.4;
    }
    let params: &[(&str, f64)] = &[
        ("time", 30.0),
        ("feedback", 0.0),
        ("dry", 0.0),
        ("wet", 1.0),
        ("rate", 2.0),
        ("depth", 1.0),
        ("motion", 0.0),
    ];
    let measure_mid_dev = |spread: f64| -> f32 {
        let mut set = params.to_vec();
        set.push(("spread", spread));
        let (l, r) = render_stereo(&set, &input);
        let mid: Vec<f32> = l.iter().zip(r.iter()).map(|(a, b)| 0.5 * (a + b)).collect();
        let mut periods = Vec::new();
        let mut last_cross = None;
        for i in 24_000..n - 1 {
            if mid[i] < 0.0 && mid[i + 1] >= 0.0 {
                if let Some(prev) = last_cross {
                    periods.push((i - prev) as f32);
                }
                last_cross = Some(i);
            }
        }
        let base = FS / 440.0;
        periods
            .iter()
            .map(|p| (p - base).abs() / base)
            .fold(0.0f32, f32::max)
    };
    let anchored = measure_mid_dev(1.0);
    let wobbling = measure_mid_dev(0.0);
    assert!(
        anchored < 0.02,
        "center pitch stands still at spread 1: {anchored}"
    );
    assert!(
        wobbling > 0.15,
        "control experiment — spread 0 mid carries the vibrato: {wobbling}"
    );
}

/// FLAVOR owns the whole journey: at 0 the recipes ARE their
/// pristine bases (bit-equal published cycles), and the character
/// GROWS monotonically with the knob.
#[test]
fn flavor_scales_the_character_from_pristine() {
    use core::sync::atomic::Ordering;
    let curve = |motion: f64, flavor: f64| -> Vec<f32> {
        let mut plugin = stave_eddy_vst3::Eddy::new();
        let bus = DebugBus::new();
        plugin.debug_attach(bus.clone());
        let table = plugin.param_table();
        for (n, v) in [("motion", motion), ("rate", 0.8), ("flavor", flavor)] {
            table.by_name(n).unwrap().set_plain(v);
        }
        plugin.activate(FS, BLOCK);
        let mut in_l = vec![0.0f32; BLOCK];
        let mut in_r = vec![0.0f32; BLOCK];
        let mut out_l = vec![0.0f32; BLOCK];
        let mut out_r = vec![0.0f32; BLOCK];
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let input = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let output = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input,
            output,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        let n = bus.curve_used.load(Ordering::Acquire) as usize;
        (0..n)
            .map(|i| f32::from_bits(bus.curve[i].load(Ordering::Relaxed)))
            .collect()
    };
    let rms_diff = |a: &[f32], b: &[f32]| -> f32 {
        let n = a.len().min(b.len()).max(1);
        (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n as f32).sqrt()
    };
    // Flavor 0 = pristine, to the BIT.
    let tri = curve(1.0, 0.0);
    let juno0 = curve(2.0, 0.0);
    for (a, b) in tri.iter().zip(juno0.iter()) {
        assert_eq!(a.to_bits(), b.to_bits(), "JUNO at flavor 0 IS the triangle");
    }
    let sine = curve(0.0, 0.0);
    let vibe0 = curve(3.0, 0.0);
    for (a, b) in sine.iter().zip(vibe0.iter()) {
        assert_eq!(a.to_bits(), b.to_bits(), "VIBE at flavor 0 IS the sine");
    }
    // Character grows with the knob.
    let j_low = rms_diff(&curve(2.0, 0.25), &tri);
    let j_high = rms_diff(&curve(2.0, 1.0), &tri);
    assert!(
        j_high > j_low * 1.5,
        "JUNO exaggerates with flavor: {j_low} → {j_high}"
    );
    let rising = |c: &[f32]| -> f32 {
        c.windows(2).filter(|w| w[1] > w[0]).count() as f32 / (c.len() - 1) as f32
    };
    let v_low = rising(&curve(3.0, 0.25));
    let v_high = rising(&curve(3.0, 1.0));
    assert!(
        v_high < v_low - 0.03,
        "VIBE grows MORE lopsided with flavor: rising {v_low} → {v_high}"
    );
}

/// DRY and WET are INDEPENDENT LEVELS, pinned at both extremes: wet
/// at 0 is bit-identical dry (above), and dry at 0 lets NO dry reach
/// the output — an impulse produces silence at t=0 and all its
/// energy at the dialed delay. (What a full-wet delay carries is the
/// whole program, late — which EARS can read as "dry plus echo";
/// this law is the arbiter, and the split knobs make the remedy a
/// dial: pull DRY down.)
#[test]
fn dry_at_zero_carries_no_dry_at_all() {
    let mut input = vec![0.0f32; 24_000];
    input[0] = 1.0;
    let out = render(
        &[
            ("time", 100.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("depth", 0.0),
        ],
        &input,
    );
    let n = (0.100 * FS) as usize;
    // Everything before the echo window is SILENT — zero dry leak.
    let pre = out[..n - 16].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert_eq!(pre, 0.0, "dry leaked into a full-wet output: {pre}");
    let (_, amp) = peak_in(&out, n - 16, 64);
    assert!(amp > 0.5, "the echo itself is present: {amp}");
}
