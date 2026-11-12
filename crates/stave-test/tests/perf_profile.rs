//! Component-level cost attribution — the "what is most expensive" table
//! the host bench can't see. Run explicitly:
//!
//!   cargo test -p stave-test --release --test perf_profile -- --ignored --nocapture
//!
//! Each primitive is timed standalone at the rate it actually runs in a
//! plugin (4× inner rate for in-oversampler stages, base rate for the
//! oversamplers themselves, drivers, and full engines). Cost is reported
//! per BASE sample (ns/tick × rate multiple), so rows are directly
//! comparable and sum roughly to a plugin's measured cost.

use stave_dsp::RcClipperConfig;
use std::hint::black_box;
use std::time::Instant;

use stave_anvil_vst3::RcCoupledTriodeCascade;
use stave_dsp::{
    shapes, Bias, Chain, CurveStage, EnvelopeFollower, EnvelopeTimes, FilterSpec, FilterStage,
    Frame, Hysteresis, ModMatrix, NonlinearRc, Oversampler2xFir, Oversampler2xFirInner, Route,
    Slew, SmoothSat, Stage, StageKind, Target, Triode,
};
use stave_multiband_comp_vst3::saturation::BandSaturator;

const BASE: f32 = 48_000.0;
/// ns available per base sample at 48 kHz.
const BUDGET_NS: f64 = 1.0e9 / 48_000.0;
const N_TICKS: usize = 1_000_000;
/// Block size for the block-API rows (a typical host block).
const BLOCK: usize = 128;

struct Row {
    label: &'static str,
    rate_mult: f64,
    ns_per_tick: f64,
}

/// Time `f` over N_TICKS of a precomputed sine (generator cost excluded).
fn time_fn(label: &'static str, rate_mult: f64, mut f: impl FnMut(f32) -> f32) -> Row {
    let fs = BASE as f64 * rate_mult;
    let input: Vec<f32> = (0..4096)
        .map(|i| (0.5 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / fs).sin()) as f32)
        .collect();
    // Warmup: settle envelopes/bias walks and fault in tables.
    for i in 0..20_000 {
        black_box(f(input[i % input.len()]));
    }
    let t0 = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..N_TICKS {
        acc += f(input[i % input.len()]);
    }
    let dt = t0.elapsed();
    black_box(acc);
    Row {
        label,
        rate_mult,
        ns_per_tick: dt.as_nanos() as f64 / N_TICKS as f64,
    }
}

/// Time a BLOCK processor over the same total sample count; reported
/// ns/tick is per sample so rows compare directly with `time_fn`.
fn time_block_fn(label: &'static str, rate_mult: f64, mut f: impl FnMut(&mut [f32])) -> Row {
    let fs = BASE as f64 * rate_mult;
    let input: Vec<f32> = (0..4096)
        .map(|i| (0.5 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / fs).sin()) as f32)
        .collect();
    let mut buf = vec![0.0f32; BLOCK];
    let blocks = N_TICKS / BLOCK;
    for w in 0..blocks / 50 {
        for (k, s) in buf.iter_mut().enumerate() {
            *s = input[(w * BLOCK + k) % input.len()];
        }
        f(&mut buf);
        black_box(buf[BLOCK - 1]);
    }
    let t0 = Instant::now();
    let mut acc = 0.0f32;
    for b in 0..blocks {
        for (k, s) in buf.iter_mut().enumerate() {
            *s = input[(b * BLOCK + k) % input.len()];
        }
        f(&mut buf);
        acc += buf[BLOCK - 1];
    }
    let dt = t0.elapsed();
    black_box(acc);
    Row {
        label,
        rate_mult,
        ns_per_tick: dt.as_nanos() as f64 / (blocks * BLOCK) as f64,
    }
}

#[test]
#[ignore = "profile report — run with --ignored --nocapture in release"]
fn component_cost_table() {
    let os_rate = BASE * 4.0;
    let mut rows: Vec<Row> = vec![];

    // ---- In-oversampler stages (4× rate, ticked 4× per base sample) ----
    let mut cs = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(2.0));
    cs.prepare(os_rate);
    cs.set_adaa(false);
    rows.push(time_fn("CurveStage (no ADAA) @4x", 4.0, |x| cs.tick(x)));
    let mut cs = CurveStage::from_fn(-6.0, 6.0, 0.0, shapes::knee(2.0));
    cs.prepare(os_rate);
    rows.push(time_fn("CurveStage (ADAA) @4x", 4.0, |x| cs.tick(x)));

    let mut sm = SmoothSat::new();
    sm.prepare(BASE);
    let mut cs = sm.to_curve_stage();
    cs.prepare(os_rate);
    rows.push(time_fn("SmoothSat curve @4x", 4.0, |x| cs.tick(x)));

    let mut tri = Triode::new();
    tri.prepare(BASE);
    tri.set_grid_drive(11.11);
    let mut cs = tri.to_curve_stage();
    cs.prepare(os_rate);
    rows.push(time_fn("Triode curve @4x", 4.0, |x| cs.tick(x)));

    let mut fs = FilterStage::new(FilterSpec::HighShelf {
        hz: 900.0,
        db: -6.0,
    });
    fs.prepare(os_rate);
    rows.push(time_fn("FilterStage shelf @4x", 4.0, |x| fs.tick(x)));

    let mut sl = Slew::new(1.0, 1.0);
    sl.prepare(os_rate);
    rows.push(time_fn("Slew @4x", 4.0, |x| sl.tick(x)));

    let mut hy = Hysteresis::new(2.0, 0.5);
    hy.prepare(os_rate);
    rows.push(time_fn("Hysteresis @4x", 4.0, |x| hy.tick(x)));

    let mut rc = NonlinearRc::diode_clipper(&RcClipperConfig {
        tau_ms: 5.0,
        threshold: 0.4,
        hardness: 3.0,
    });
    rc.prepare(os_rate);
    rows.push(time_fn("NonlinearRc diode @4x", 4.0, |x| rc.tick(x)));

    let mut rc = NonlinearRc::ts_feedback_clipper(
        &RcClipperConfig {
            tau_ms: 5.0,
            threshold: 0.4,
            hardness: 6.0,
        },
        4.0,
    );
    rc.prepare(os_rate);
    rows.push(time_fn("NonlinearRc TS fb @4x", 4.0, |x| rc.tick(x)));

    let mut cathode_biased = Triode::new();
    cathode_biased.prepare(os_rate);
    let bypass_farads = 60e-3 / cathode_biased.cathode_ohms();
    cathode_biased.set_bias(Bias::Cathode { bypass_farads });
    rows.push(time_fn("Triode cathode-biased @4x", 4.0, |x| {
        cathode_biased.process_sample(x)
    }));

    // A Forge-shaped 5-stage chain: curve, shelf, curve, slew, hysteresis.
    let mut ch = Chain::<5>::new();
    ch.push(StageKind::Curve(CurveStage::from_fn(
        -6.0,
        6.0,
        0.0,
        shapes::knee(1.6),
    )));
    ch.push(StageKind::Filter(FilterStage::new(FilterSpec::HighShelf {
        hz: 900.0,
        db: -6.0,
    })));
    ch.push(StageKind::Curve(CurveStage::from_fn(
        -6.0,
        6.0,
        0.0,
        shapes::knee(4.0),
    )));
    ch.push(StageKind::Slew(Slew::new(1.0, 1.0)));
    ch.push(StageKind::Hysteresis(Hysteresis::new(2.0, 0.5)));
    ch.prepare(os_rate);
    rows.push(time_fn("Chain<5> forge-like @4x", 4.0, |x| ch.tick(x)));

    // ---- Oversamplers (cost per base input sample, identity inner) ------
    let mut os = Oversampler2xFir::new();
    os.prepare(BASE);
    rows.push(time_fn("Oversampler2xFir (outer pair)", 1.0, |x| {
        os.process_one(x, |s| s)
    }));
    let mut inner = Oversampler2xFirInner::new();
    inner.prepare(BASE * 2.0);
    rows.push(time_fn("Oversampler2xFirInner @2x", 2.0, |x| {
        inner.process_one(x, |s| s)
    }));
    let mut os = Oversampler2xFir::new();
    os.prepare(BASE);
    let mut inner = Oversampler2xFirInner::new();
    inner.prepare(BASE * 2.0);
    rows.push(time_fn("Nested 4x OS (identity)", 1.0, |x| {
        os.process_one(x, |s| inner.process_one(s, |t| t))
    }));

    // ---- Base-rate control plane -----------------------------------------
    let mut env = EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: 5.0,
        release_ms: 100.0,
        slow_release_ms: None,
    });
    env.prepare(BASE);
    rows.push(time_fn("EnvelopeFollower @base", 1.0, |x| env.tick(x)));

    let mut ch2 = Chain::<2>::new();
    let id = ch2.push(StageKind::Curve(CurveStage::from_fn(
        -4.0,
        4.0,
        0.0,
        shapes::knee(2.0),
    )));
    ch2.prepare(BASE);
    let mut mm = ModMatrix::<1, 4>::new([EnvelopeFollower::new(EnvelopeTimes {
        attack_ms: 5.0,
        release_ms: 100.0,
        slow_release_ms: None,
    })]);
    mm.prepare(BASE);
    mm.push_route(Route {
        driver: 0,
        stage: id,
        target: Target::Bias,
        amount: 0.4,
    });
    mm.push_route(Route {
        driver: 0,
        stage: id,
        target: Target::Gain,
        amount: 0.3,
    });
    let base_frames = [(id, Frame::NEUTRAL)];
    rows.push(time_fn("ModMatrix 1d/2r @base", 1.0, |x| {
        mm.tick(&[x], &mut ch2, &base_frames, &[]);
        0.0
    }));

    // ---- Block API: same components, block dispatch ------------
    let mut os = Oversampler2xFir::new();
    os.prepare(BASE);
    let mut inner = Oversampler2xFirInner::new();
    inner.prepare(BASE * 2.0);
    let mut up2 = vec![0.0f32; 2 * BLOCK];
    let mut up4 = vec![0.0f32; 4 * BLOCK];
    rows.push(time_block_fn("Nested 4x OS block(128) ident", 1.0, |io| {
        os.process_block(io, &mut up2, |b2| {
            inner.process_block(b2, &mut up4, |_b4| {});
        });
    }));

    let mut chb = Chain::<5>::new();
    chb.push(StageKind::Curve(CurveStage::from_fn(
        -6.0,
        6.0,
        0.0,
        shapes::knee(1.6),
    )));
    chb.push(StageKind::Filter(FilterStage::new(FilterSpec::HighShelf {
        hz: 900.0,
        db: -6.0,
    })));
    chb.push(StageKind::Curve(CurveStage::from_fn(
        -6.0,
        6.0,
        0.0,
        shapes::knee(4.0),
    )));
    chb.push(StageKind::Slew(Slew::new(1.0, 1.0)));
    chb.push(StageKind::Hysteresis(Hysteresis::new(2.0, 0.5)));
    chb.prepare(os_rate);
    rows.push(time_block_fn("Chain<5> block(128) @4x", 4.0, |io| {
        chb.process_block(io);
    }));

    // ---- Full engines (own their oversampling; cost per base sample) ----
    let mut sat = BandSaturator::new();
    sat.prepare(BASE);
    sat.set_drive_db(8.0);
    rows.push(time_fn("BandSaturator (full, 4x OS)", 1.0, |x| {
        sat.process(x)
    }));

    let mut sat = BandSaturator::new();
    sat.prepare(BASE);
    sat.prepare_block(BLOCK);
    sat.set_drive_db(8.0);
    rows.push(time_block_fn("BandSaturator TUBE block(128)", 1.0, |io| {
        sat.process_block(io);
    }));

    let mut pre = RcCoupledTriodeCascade::new();
    pre.prepare(BASE);
    pre.set_drive_db(8.0);
    rows.push(time_fn("RcCoupledTriodeCascade (full)", 1.0, |x| {
        pre.process_sample(x)
    }));

    let mut tri = Triode::new();
    tri.prepare(BASE);
    tri.set_grid_drive(11.11);
    rows.push(time_fn("Triode (full)", 1.0, |x| tri.process_sample(x)));

    // ---- Report ----------------------------------------------------------
    rows.sort_by(|a, b| {
        (b.ns_per_tick * b.rate_mult)
            .partial_cmp(&(a.ns_per_tick * a.rate_mult))
            .unwrap()
    });
    println!("\ncomponent cost per BASE sample (48 kHz mono; budget {BUDGET_NS:.0} ns):");
    println!(
        "  {:<30} {:>9} {:>12} {:>9}",
        "component", "ns/tick", "ns/base-smp", "% of RT"
    );
    for r in &rows {
        let per_base = r.ns_per_tick * r.rate_mult;
        println!(
            "  {:<30} {:>9.1} {:>12.1} {:>8.3}%",
            r.label,
            r.ns_per_tick,
            per_base,
            per_base / BUDGET_NS * 100.0
        );
    }
}
