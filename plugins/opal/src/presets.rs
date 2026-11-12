//! Factory presets — the algorithm-space corners plus the M-mod
//! showcases (each one leads with a different modulation idea:
//! drift, weather, fatigue, rhythm, sympathy), declared typed
//! against the params (enum values are step indices). Every preset
//! states the FULL musical surface so applying one is a known
//! place, not a diff — mod presets also ZERO the slots they don't
//! use.

extern crate alloc;
use alloc::vec::Vec;

use stave_core::editor_kit::preset;
use stave_core::Preset;

use crate::params::OpalParams;

pub fn presets(p: &OpalParams) -> Vec<Preset> {
    // (algo: CHAIN 0 / FORK 1 / TWIN 2 / STACK2 3 / TRIPLE 4 /
    //  BRIGHT 5 / ORGAN2 6 / ORGAN 7)
    let full = |algo: f64,
                feedback: f64,
                vel_mod: f64,
                ops: [(f64, f64, f64, f64, f64, f64); 4]| {
        let [(r1, l1, a1, d1, s1, e1), (r2, l2, a2, d2, s2, e2), (r3, l3, a3, d3, s3, e3), (r4, l4, a4, d4, s4, e4)] =
            ops;
        alloc::vec![
            (&p.algo as &dyn stave_core::params::AnyParam, algo),
            (&p.feedback, feedback),
            (&p.vel_mod, vel_mod),
            (&p.ratio1, r1),
            (&p.level1, l1),
            (&p.attack1, a1),
            (&p.decay1, d1),
            (&p.sustain1, s1),
            (&p.release1, e1),
            (&p.ratio2, r2),
            (&p.level2, l2),
            (&p.attack2, a2),
            (&p.decay2, d2),
            (&p.sustain2, s2),
            (&p.release2, e2),
            (&p.ratio3, r3),
            (&p.level3, l3),
            (&p.attack3, a3),
            (&p.decay3, d3),
            (&p.sustain3, s3),
            (&p.release3, e3),
            (&p.ratio4, r4),
            (&p.level4, l4),
            (&p.attack4, a4),
            (&p.decay4, d4),
            (&p.sustain4, s4),
            (&p.release4, e4),
        ]
    };
    // Route slots: (src, tgt, depth, via, mode, lag_ms, quant) —
    // indices are the enum steps (SOURCES: LFO1=1 VEL=2 KEY=3
    // ENV=4 SPREAD=5 HEAT=6 SYMP=7 LFO2=8; TARGETS: PITCH=0
    // L2..L4=1..3 FB=4 L1=5 RATIO2=6 LFO2RATE=7).
    let routes = |slots: [(f64, f64, f64, f64, f64, f64, f64); 4]| {
        p.routes
            .iter()
            .zip(slots)
            .flat_map(|(r, (src, tgt, depth, via, mode, lag, quant))| {
                [
                    (&r.src as &dyn stave_core::params::AnyParam, src),
                    (&r.tgt, tgt),
                    (&r.depth, depth),
                    (&r.via, via),
                    (&r.mode, mode),
                    (&r.lag, lag),
                    (&r.quant, quant),
                ]
            })
            .collect::<alloc::vec::Vec<_>>()
    };
    let lfos = |r1: f64,
                sh1: f64,
                sy1: f64,
                ri1: f64,
                tr1: f64,
                r2: f64,
                sh2: f64,
                sy2: f64,
                ri2: f64,
                tr2: f64| {
        alloc::vec![
            (&p.lfo_rate as &dyn stave_core::params::AnyParam, r1),
            (&p.lfo_shape, sh1),
            (&p.lfo_sync, sy1),
            (&p.lfo_rise, ri1),
            (&p.lfo_trig, tr1),
            (&p.lfo2_rate, r2),
            (&p.lfo2_shape, sh2),
            (&p.lfo2_sync, sy2),
            (&p.lfo2_rise, ri2),
            (&p.lfo2_trig, tr2),
        ]
    };
    let off = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    // WOW MACHINE: tape-wowed e-piano — LFO 1 DRIFTS the pitch like
    // a worn capstan; LFO 2 breathes the brightness slowly.
    let mut wow = full(
        3.0,
        0.15,
        0.9,
        [
            (1.0, 1.0, 1.0, 1_400.0, 0.0, 400.0),
            (14.0, 0.25, 1.0, 120.0, 0.0, 120.0),
            (1.0, 0.6, 1.0, 900.0, 0.0, 350.0),
            (1.0, 0.35, 1.0, 300.0, 0.0, 200.0),
        ],
    );
    wow.extend(lfos(0.8, 5.0, 0.0, 0.0, 0.0, 0.11, 0.0, 0.0, 0.0, 0.0));
    wow.extend(routes([
        (1.0, 0.0, 0.015, 0.0, 0.0, 0.0, 0.0),  // LFO1(drift)→PITCH
        (8.0, 1.0, 0.35, 0.0, 1.0, 200.0, 0.0), // LFO2→LEVEL 2, uni, lagged
        off,
        off,
    ]));
    // FM WEATHER: a pad where nothing repeats — LFO2 wanders the
    // brightness, every note carries its own detune (SPREAD→RATIO),
    // and chords bloom the shimmer op (SYMP→LEVEL 4).
    let mut weather = full(
        4.0,
        0.55,
        0.4,
        [
            (1.0, 1.0, 900.0, 3_000.0, 0.8, 3_500.0),
            (1.5, 0.8, 1_200.0, 3_000.0, 0.75, 3_500.0),
            (2.0, 0.6, 1_500.0, 3_000.0, 0.7, 3_500.0),
            (0.5, 0.35, 600.0, 2_000.0, 0.6, 2_500.0),
        ],
    );
    weather.extend(lfos(1.2, 0.0, 0.0, 0.0, 0.0, 0.15, 5.0, 0.0, 0.0, 0.0));
    weather.extend(routes([
        (8.0, 1.0, 0.5, 0.0, 1.0, 400.0, 0.0), // LFO2(drift)→L2 uni
        (5.0, 6.0, 0.25, 0.0, 0.0, 0.0, 0.0),  // SPREAD→RATIO 2
        (7.0, 3.0, 0.4, 0.0, 1.0, 300.0, 0.0), // SYMP→L4 uni
        off,
    ]));
    // HOT CATHODE: fatigue as tone — play hard and long and the
    // bass overdrives (HEAT→brightness+feedback), velocity digs
    // vibrato in via the VIA scaler.
    let mut cathode = full(
        0.0,
        0.2,
        1.0,
        [
            (0.5, 1.0, 1.0, 500.0, 0.6, 120.0),
            (0.5, 0.7, 1.0, 90.0, 0.15, 90.0),
            (1.0, 0.5, 1.0, 60.0, 0.0, 60.0),
            (1.0, 0.4, 1.0, 40.0, 0.0, 40.0),
        ],
    );
    cathode.extend(lfos(5.5, 0.0, 0.0, 500.0, 1.0, 0.3, 0.0, 0.0, 0.0, 0.0));
    cathode.extend(routes([
        (6.0, 1.0, 0.7, 0.0, 1.0, 500.0, 0.0), // HEAT→L2 uni lag
        (6.0, 4.0, 0.5, 0.0, 1.0, 500.0, 0.0), // HEAT→FEEDBACK uni
        (1.0, 0.0, 0.015, 2.0, 0.0, 0.0, 0.0), // LFO1→PITCH via VEL
        off,
    ]));
    // RATCHET: tempo-locked rhythm from the matrix alone — a synced
    // saw, note-retriggered, QUANTIZED to 4 rungs, chopping the
    // carrier level every 8th.
    let mut ratchet = full(
        1.0,
        0.2,
        1.0,
        [
            (1.0, 1.0, 0.5, 700.0, 0.4, 250.0),
            (2.0, 0.5, 0.5, 150.0, 0.2, 100.0),
            (5.02, 0.35, 0.5, 80.0, 0.0, 60.0),
            (9.0, 0.3, 0.5, 40.0, 0.0, 40.0),
        ],
    );
    ratchet.extend(lfos(2.0, 2.0, 4.0, 0.0, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0));
    ratchet.extend(routes([
        (1.0, 5.0, -0.9, 0.0, 1.0, 0.0, 3.0), // LFO1(saw 1/8)→L1, 4 rungs
        (1.0, 1.0, 0.4, 0.0, 1.0, 0.0, 3.0),  // same clock brightens
        off,
        off,
    ]));
    // SYMPATHY BELLS: strike one bell and the others answer —
    // ensemble energy opens the shimmer op; every note is its own
    // slightly-detuned casting; vibrato only arrives if you hit it.
    let mut sympathy = full(
        2.0,
        0.0,
        0.7,
        [
            (1.0, 1.0, 1.0, 2_500.0, 0.0, 2_000.0),
            (3.53, 0.45, 1.0, 1_200.0, 0.0, 1_500.0),
            (7.11, 0.3, 1.0, 600.0, 0.0, 800.0),
            (1.0, 0.4, 1.0, 900.0, 0.0, 900.0),
        ],
    );
    sympathy.extend(lfos(5.0, 0.0, 0.0, 600.0, 1.0, 0.25, 0.0, 0.0, 0.0, 0.0));
    sympathy.extend(routes([
        (7.0, 2.0, 0.6, 0.0, 1.0, 200.0, 0.0), // SYMP→L3 uni lag
        (5.0, 0.0, 0.04, 0.0, 0.0, 0.0, 0.0),  // SPREAD→PITCH
        (1.0, 0.0, 0.02, 2.0, 0.0, 0.0, 0.0),  // LFO1→PITCH via VEL
        off,
    ]));

    alloc::vec![
        preset(
            "INIT",
            &full(
                0.0,
                0.0,
                0.5,
                [
                    (1.0, 1.0, 2.0, 600.0, 0.7, 300.0),
                    (1.0, 0.5, 2.0, 400.0, 0.5, 300.0),
                    (1.0, 0.0, 2.0, 400.0, 0.5, 300.0),
                    (1.0, 0.0, 2.0, 400.0, 0.5, 300.0),
                ]
            ),
        ),
        // The DX lineage classic: two 2-op tine pairs, mod decays
        // faster than the body — bright strike, warm ring.
        preset(
            "E PIANO",
            &full(
                3.0,
                0.15,
                0.9,
                [
                    (1.0, 1.0, 1.0, 1_400.0, 0.0, 400.0),
                    (14.0, 0.25, 1.0, 120.0, 0.0, 120.0),
                    (1.0, 0.6, 1.0, 900.0, 0.0, 350.0),
                    (1.0, 0.35, 1.0, 300.0, 0.0, 200.0),
                ]
            ),
        ),
        // Inharmonic ratio, long ring: the classic FM bell.
        preset(
            "GLASS BELL",
            &full(
                2.0,
                0.0,
                0.7,
                [
                    (1.0, 1.0, 1.0, 2_500.0, 0.0, 2_000.0),
                    (3.53, 0.45, 1.0, 1_200.0, 0.0, 1_500.0),
                    (7.11, 0.3, 1.0, 600.0, 0.0, 800.0),
                    (1.0, 0.4, 1.0, 900.0, 0.0, 900.0),
                ]
            ),
        ),
        // Chain at low ratios, fast mod decay: the growl that
        // cleans up into a round low end.
        preset(
            "FM BASS",
            &full(
                0.0,
                0.35,
                1.0,
                [
                    (0.5, 1.0, 1.0, 500.0, 0.6, 120.0),
                    (0.5, 0.7, 1.0, 90.0, 0.15, 90.0),
                    (1.0, 0.5, 1.0, 60.0, 0.0, 60.0),
                    (1.0, 0.4, 1.0, 40.0, 0.0, 40.0),
                ]
            ),
        ),
        // One modulator into three detuned-ish carriers, slow
        // attack: section brass.
        preset(
            "BRASS",
            &full(
                4.0,
                0.25,
                0.8,
                [
                    (1.0, 1.0, 60.0, 800.0, 0.8, 250.0),
                    (1.0, 0.9, 80.0, 800.0, 0.75, 250.0),
                    (2.0, 0.5, 70.0, 800.0, 0.7, 250.0),
                    (1.0, 0.55, 30.0, 300.0, 0.6, 200.0),
                ]
            ),
        ),
        // Additive drawbars, no modulation: the honesty check you
        // can HEAR (organ = pure partials at the ratios).
        preset(
            "ORGAN",
            &full(
                7.0,
                0.0,
                0.2,
                [
                    (1.0, 1.0, 3.0, 200.0, 0.9, 80.0),
                    (2.0, 0.7, 3.0, 200.0, 0.9, 80.0),
                    (3.0, 0.45, 3.0, 200.0, 0.9, 80.0),
                    (4.0, 0.3, 3.0, 200.0, 0.9, 80.0),
                ]
            ),
        ),
        // Percussive chain pluck: everything decays, mod fastest.
        preset(
            "PLUCK",
            &full(
                1.0,
                0.2,
                1.0,
                [
                    (1.0, 1.0, 0.5, 700.0, 0.0, 250.0),
                    (2.0, 0.5, 0.5, 150.0, 0.0, 100.0),
                    (5.02, 0.35, 0.5, 80.0, 0.0, 60.0),
                    (9.0, 0.3, 0.5, 40.0, 0.0, 40.0),
                ]
            ),
        ),
        // Slow everything + feedback shimmer on the top op.
        preset(
            "SPACE PAD",
            &full(
                4.0,
                0.55,
                0.4,
                [
                    (1.0, 1.0, 900.0, 3_000.0, 0.8, 3_500.0),
                    (1.5, 0.8, 1_200.0, 3_000.0, 0.75, 3_500.0),
                    (2.0, 0.6, 1_500.0, 3_000.0, 0.7, 3_500.0),
                    (0.5, 0.35, 600.0, 2_000.0, 0.6, 2_500.0),
                ]
            ),
        ),
        preset("WOW MACHINE", &wow),
        preset("FM WEATHER", &weather),
        preset("HOT CATHODE", &cathode),
        preset("RATCHET", &ratchet),
        preset("SYMPATHY BELLS", &sympathy),
    ]
}
