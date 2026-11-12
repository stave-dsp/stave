//! Opal's OWN laws — FM physics as derived expectations (Chowning,
//! 1973: a modulator at fm puts sidebands at fc ± k·fm, nowhere
//! else), plus the synth-class laws every instrument states:
//! * SIDEBANDS land exactly at fc ± k·fm and vanish at level 0;
//! * ORGAN (additive) is PURE partials at the dialed ratios —
//!   the wiring table's null case, audible;
//! * FEEDBACK grows harmonics on a lone carrier (sine → saw-ward);
//! * VELOCITY tracks brightness when vel_mod is up, and does NOT
//!   when it is down;
//! * pitch, bit-zero silence — the chime-class base.

use std::sync::Arc;

use stave_core::{
    AudioBuffer, DebugBus, Event, EventKind, NoteEvent, NoteKind, Plugin, ProcessCtx,
};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn note_on(key: i16, vel: f64) -> Event {
    Event {
        frame: 0,
        kind: EventKind::Note(NoteEvent {
            kind: NoteKind::On,
            key,
            channel: 0,
            velocity: vel,
            note_id: 1,
        }),
    }
}

fn render(set: &[(&str, f64)], key: i16, vel: f64, blocks: usize) -> Vec<f32> {
    let mut plugin = stave_opal_vst3::Opal::new();
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
    let mut out = Vec::with_capacity(blocks * BLOCK);
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for b in 0..blocks {
        let evs = if b == 0 && key >= 0 {
            vec![note_on(key, vel)]
        } else {
            vec![]
        };
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let mut in_ptrs: [*mut f32; 0] = [];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [] as [&mut [f32]; 0], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &evs,
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    out
}

/// Single-bin correlation magnitude at `hz`.
fn mag_at(w: &[f32], hz: f64) -> f64 {
    let n = w.len() as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &x) in w.iter().enumerate() {
        let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(FS);
        re += f64::from(x) * ph.cos();
        im += f64::from(x) * ph.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// A sustained window well past the attack (0.25 s .. 1.0 s).
fn sustained(w: &[f32]) -> &[f32] {
    &w[12_000..48_000]
}

/// A 2-op pair (STACK2, only ops 1←2 sounding) with ratio2 = 2:
/// sidebands at fc ± k·2fc — energy at 3fc and 5fc, and NONE of it
/// when the modulator's level is zero. Chowning, measured.
#[test]
fn sidebands_land_where_chowning_says() {
    let fc = 220.0f64; // key 57
    let base: Vec<(&str, f64)> = vec![
        ("algo", 3.0), // STACK2: 2→1, 4→3
        ("ratio 2", 2.0),
        ("sustain 1", 0.8),
        ("sustain 2", 0.8),
        ("level 3", 0.0),
        ("level 4", 0.0),
        ("vel mod", 0.0),
    ];
    let mut on = base.clone();
    on.push(("level 2", 0.8));
    let mut off = base.clone();
    off.push(("level 2", 0.0));
    let bright = render(&on, 57, 1.0, 200);
    let pure = render(&off, 57, 1.0, 200);
    let (b, p) = (sustained(&bright), sustained(&pure));
    for k in [1.0f64, 2.0] {
        let sb = fc + k * 2.0 * fc; // 660, 1100 Hz
        let with_mod = mag_at(b, sb);
        let without = mag_at(p, sb);
        assert!(
            with_mod > 20.0 * without.max(1e-9),
            "sideband fc+{k}·fm: {with_mod:.2e} with mod vs {without:.2e} without"
        );
    }
    // And the pure render IS pure: second harmonic at the floor.
    let h1 = mag_at(p, fc);
    let h2 = mag_at(p, 2.0 * fc);
    assert!(
        h2 < 1e-3 * h1,
        "level-2-at-zero must be a sine: h2/h1 = {:.2e}",
        h2 / h1
    );
}

/// ORGAN is additive truth: partials at exactly the dialed ratios,
/// nothing between them (the wiring table's null case, audible).
#[test]
fn organ_is_pure_partials() {
    let fc = 220.0f64;
    let set: Vec<(&str, f64)> = vec![
        ("algo", 7.0),
        ("ratio 2", 2.0),
        ("ratio 3", 3.0),
        ("ratio 4", 4.0),
        ("level 2", 0.7),
        ("level 3", 0.45),
        ("level 4", 0.3),
        ("sustain 1", 0.9),
        ("sustain 2", 0.9),
        ("sustain 3", 0.9),
        ("sustain 4", 0.9),
        ("vel mod", 0.0),
    ];
    let out = render(&set, 57, 1.0, 200);
    let s = sustained(&out);
    let fundamental = mag_at(s, fc);
    for r in [2.0f64, 3.0, 4.0] {
        let partial = mag_at(s, r * fc);
        assert!(
            partial > 0.1 * fundamental,
            "partial at ratio {r} missing: {partial:.2e} vs f0 {fundamental:.2e}"
        );
    }
    // Between the partials: silence (no modulation happened).
    let between = mag_at(s, 2.5 * fc);
    assert!(
        between < 1e-2 * fundamental,
        "additive organ has energy between partials: {between:.2e}"
    );
}

/// Feedback grows teeth: a LONE carrier op 4 with feedback up gains
/// harmonics a pure sine cannot have.
#[test]
fn feedback_grows_harmonics_on_a_lone_carrier() {
    let fc = 220.0f64;
    let base: Vec<(&str, f64)> = vec![
        ("algo", 7.0), // ORGAN: op 4 is a carrier
        ("level 1", 0.0),
        ("level 2", 0.0),
        ("level 3", 0.0),
        ("level 4", 1.0),
        ("sustain 4", 0.9),
        ("vel mod", 0.0),
    ];
    let mut fb_on = base.clone();
    fb_on.push(("feedback", 0.9));
    let mut fb_off = base.clone();
    fb_off.push(("feedback", 0.0));
    let bright = render(&fb_on, 57, 1.0, 200);
    let pure = render(&fb_off, 57, 1.0, 200);
    let ratio = |w: &[f32]| {
        let s = sustained(w);
        (mag_at(s, 2.0 * fc) + mag_at(s, 3.0 * fc)) / mag_at(s, fc).max(1e-12)
    };
    let (with_fb, without) = (ratio(&bright), ratio(&pure));
    assert!(
        with_fb > 10.0 * without.max(1e-6),
        "feedback must brighten a lone carrier: {with_fb:.4} vs {without:.6}"
    );
}

/// Velocity IS brightness when vel_mod is up — the FM
/// expressiveness contract: soft notes keep their sidebands down
/// FURTHER than mere loudness explains; with vel_mod at 0 the
/// spectrum is velocity-invariant (normalized).
#[test]
fn velocity_tracks_brightness_only_when_dialed() {
    let fc = 220.0f64;
    let set = |vm: f64| -> Vec<(&str, f64)> {
        vec![
            ("algo", 3.0),
            ("ratio 2", 2.0),
            ("level 2", 0.8),
            ("sustain 1", 0.8),
            ("sustain 2", 0.8),
            ("level 3", 0.0),
            ("level 4", 0.0),
            ("vel mod", vm),
        ]
    };
    let sb_ratio = |set: &[(&str, f64)], vel: f64| {
        let out = render(set, 57, vel, 200);
        let s = sustained(&out);
        mag_at(s, 3.0 * fc) / mag_at(s, fc).max(1e-12)
    };
    // vel_mod 1: soft is DARKER (relative sideband energy drops).
    let expressive = set(1.0);
    let (soft, hard) = (sb_ratio(&expressive, 0.25), sb_ratio(&expressive, 1.0));
    assert!(
        hard > 2.0 * soft,
        "vel_mod 1: hard {hard:.3} must out-brighten soft {soft:.3}"
    );
    // vel_mod 0: same relative spectrum at any velocity.
    let flat = set(0.0);
    let (soft0, hard0) = (sb_ratio(&flat, 0.25), sb_ratio(&flat, 1.0));
    assert!(
        (soft0 - hard0).abs() < 0.15 * hard0.max(1e-9),
        "vel_mod 0: spectrum must be velocity-invariant ({soft0:.3} vs {hard0:.3})"
    );
}

/// The chime-class base: key 69 with ratio 1 sounds 440 Hz, and no
/// notes is BIT-zero.
#[test]
fn pitch_and_silence() {
    let out = render(&[("vel mod", 0.0)], 69, 1.0, 100);
    let s = &out[12_000..];
    let on = mag_at(s, 440.0);
    let detuned = mag_at(s, 440.0 * 1.0293);
    assert!(
        on > 10.0 * detuned,
        "carrier off-pitch: {on:.2e} vs {detuned:.2e}"
    );

    let silent = render(&[], -1, 0.0, 40);
    for (i, v) in silent.iter().enumerate() {
        assert!(v.to_bits() == 0.0f32.to_bits(), "sample {i}: {v}");
    }
}

/// THE MATRIX LAWS (per-op destinations):
/// * depth-0/OFF routes are BIT-identical to no routes;
/// * KEY→LEVEL 2 is keyboard-tracked BRIGHTNESS: in one render,
///   a high note carries proportionally more sideband energy than
///   a low note — per-voice modulation of the FM index, measured.
#[test]
fn matrix_depth_zero_is_bit_identical() {
    let clean = render(&[("level 2", 0.6)], 69, 1.0, 60);
    let armed = render(
        &[
            ("level 2", 0.6),
            ("src 1", 1.0),
            ("tgt 1", 1.0),
            ("depth 1", 0.0),
            ("src 3", 3.0),
            ("depth 3", 0.0),
        ],
        69,
        1.0,
        60,
    );
    for (i, (a, b)) in clean.iter().zip(&armed).enumerate() {
        assert!(a.to_bits() == b.to_bits(), "sample {i}: {a} vs {b}");
    }
}

#[test]
fn key_to_level_is_keyboard_tracked_brightness() {
    // STACK2, op2 modulating op1 at base level 0.3; KEY→LEVEL 2 at
    // full depth: a note an octave up gets a hotter modulator.
    // Sideband/carrier ratios must order by key.
    let set: Vec<(&str, f64)> = vec![
        ("algo", 3.0),
        ("ratio 2", 2.0),
        ("level 2", 0.3),
        ("level 3", 0.0),
        ("level 4", 0.0),
        ("sustain 1", 0.8),
        ("sustain 2", 0.8),
        ("vel mod", 0.0),
        ("src 1", 3.0), // KEY
        ("tgt 1", 1.0), // LEVEL 2
        ("depth 1", 1.0),
    ];
    let ratio_at = |key: i16, hz: f64| {
        let out = render(&set, key, 1.0, 160);
        let s = &out[24_000..];
        mag_at(s, 3.0 * hz) / mag_at(s, hz).max(1e-12)
    };
    let low = ratio_at(45, 110.0);
    let high = ratio_at(81, 880.0);
    assert!(
        high > 2.0 * low,
        "KEY→LEVEL 2 must brighten up the keyboard: low {low:.3} vs high {high:.3}"
    );
}

/// THE SECOND LFO is its own clock: routed to PITCH it puts
/// sidebands at ITS rate, not LFO 1's.
#[test]
fn lfo2_is_its_own_clock() {
    let set: Vec<(&str, f64)> = vec![
        ("sustain 1", 0.8),
        ("level 2", 0.0),
        ("level 3", 0.0),
        ("level 4", 0.0),
        ("vel mod", 0.0),
        ("lfo rate", 2.0),
        ("lfo2 rate", 8.0),
        ("src 1", 8.0), // LFO 2
        ("tgt 1", 0.0), // PITCH
        ("depth 1", 0.02),
    ];
    let out = render(&set, 69, 1.0, 200);
    let s = sustained(&out);
    let fc = 440.0;
    // ±2 Hz sits inside the carrier's own spectral lobe at this
    // window length — probe off-grid instead (5.5 Hz matches
    // neither LFO and clears the lobe).
    let off = mag_at(s, fc + 5.5) + mag_at(s, fc - 5.5);
    let at8 = mag_at(s, fc + 8.0) + mag_at(s, fc - 8.0);
    assert!(
        at8 > 3.0 * off,
        "LFO 2 must modulate at its own 8 Hz: ±8 {at8:.3e} vs off-grid {off:.3e}"
    );
}

/// RATIO 2 is an inharmonicity destination: a full bend (+1 st,
/// unipolar velocity at ff) moves the sideband off the harmonic —
/// the modulator sounds at 2·2^(1/12)·fc, so the upper sideband
/// lands at fc·(1 + 2·2^(1/12)), measurably not at 3·fc.
#[test]
fn ratio2_target_bends_the_sideband() {
    let fc = 220.0f64;
    let base: Vec<(&str, f64)> = vec![
        ("algo", 3.0), // STACK2
        ("ratio 2", 2.0),
        ("level 2", 0.5),
        ("sustain 1", 0.8),
        ("sustain 2", 0.8),
        ("level 3", 0.0),
        ("level 4", 0.0),
        ("vel mod", 0.0),
    ];
    let mut bent = base.clone();
    bent.extend([
        ("src 1", 2.0),
        ("tgt 1", 6.0),
        ("depth 1", 1.0),
        ("mode 1", 1.0),
    ]);
    let straight = render(&base, 57, 1.0, 200);
    let curved = render(&bent, 57, 1.0, 200);
    let bent_sb = fc * (1.0 + 2.0 * 2f64.powf(1.0 / 12.0)); // ≈ 686 Hz
    let straight_sb = 3.0 * fc; // 660 Hz
    let on_bent = mag_at(sustained(&curved), bent_sb);
    let on_straight = mag_at(sustained(&curved), straight_sb);
    assert!(
        on_bent > 3.0 * on_straight,
        "bent modulator must move the sideband: {bent_sb:.0} Hz {on_bent:.3e} vs \
         {straight_sb:.0} Hz {on_straight:.3e}"
    );
    // And the unbent render keeps it harmonic.
    let off_bent = mag_at(sustained(&straight), bent_sb);
    let off_straight = mag_at(sustained(&straight), straight_sb);
    assert!(
        off_straight > 3.0 * off_bent,
        "control render must stay harmonic"
    );
}

/// LEVEL 1 is a destination too (the Operator lesson: carrier
/// volume belongs in the matrix): full negative unipolar velocity
/// depth silences a fortissimo ORGAN carrier.
#[test]
fn level1_is_a_matrix_destination() {
    let base: Vec<(&str, f64)> = vec![
        ("algo", 7.0), // ORGAN
        ("level 1", 1.0),
        ("level 2", 0.0),
        ("level 3", 0.0),
        ("level 4", 0.0),
        ("sustain 1", 0.9),
        ("vel mod", 0.0),
    ];
    let mut cut = base.clone();
    cut.extend([
        ("src 1", 2.0),
        ("tgt 1", 5.0),
        ("depth 1", -1.0),
        ("mode 1", 1.0),
    ]);
    let loud = render(&base, 57, 1.0, 100);
    let quiet = render(&cut, 57, 1.0, 100);
    let (l, q) = (rms_win(&loud[12_000..]), rms_win(&quiet[12_000..]));
    assert!(
        l > 5.0 * q,
        "VEL→LEVEL 1 at −1 must gut the carrier: {l:.4} vs {q:.4}"
    );
}

fn rms_win(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}
