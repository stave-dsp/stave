//! Chime's OWN laws — the promises only a synth can state, every
//! expectation DERIVED (the tilt lesson: generic gates can't know
//! which way a knob points, and they can't know note 69 is 440 Hz):
//! * PITCH: note 69 sounds at 440 Hz, note 60 at middle C — equal
//!   temperament from the declaration, measured by correlation;
//! * VELOCITY orders loudness;
//! * SILENCE: no notes in, BIT-zero out (an instrument's dry law);
//! * mid-block note-ons start at their exact sample offset;
//! * RELEASE rings then dies at the dialed τ;
//! * the pool never exceeds MAX and a 17th note still sounds.

use std::sync::Arc;

use stave_core::{
    AudioBuffer, DebugBus, Event, EventKind, NoteEvent, NoteKind, Plugin, ProcessCtx,
};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn note(frame: u32, on: bool, key: i16, vel: f64, id: i32) -> Event {
    Event {
        frame,
        kind: EventKind::Note(NoteEvent {
            kind: if on { NoteKind::On } else { NoteKind::Off },
            key,
            channel: 0,
            velocity: vel,
            note_id: id,
        }),
    }
}

/// Render `blocks` blocks; `events` are (block_index, event) —
/// delivered to that block with their intra-block frame offsets.
fn render(set: &[(&str, f64)], events: &[(usize, Event)], blocks: usize) -> Vec<f32> {
    let mut plugin = stave_chime_vst3::Chime::new();
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
        let evs: Vec<Event> = events
            .iter()
            .filter(|(eb, _)| *eb == b)
            .map(|(_, e)| *e)
            .collect();
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

fn rms(w: &[f32]) -> f64 {
    (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
}

/// Note 69 IS 440 Hz and note 60 IS middle C — the declaration's
/// equal temperament, measured. A quarter-tone probe beside each
/// must read ≥ 20 dB down.
#[test]
fn the_pitch_is_the_declared_temperament() {
    for (key, hz) in [(69i16, 440.0f64), (60, 261.6256)] {
        let out = render(&[], &[(0, note(0, true, key, 1.0, 1))], 100);
        let sustained = &out[12_000..];
        let on = mag_at(sustained, hz);
        let off = mag_at(sustained, hz * 1.0293); // +50 cents
        assert!(
            on > 10.0 * off,
            "key {key}: {hz} Hz reads {on:.2e}, detuned probe {off:.2e}"
        );
    }
}

#[test]
fn velocity_orders_loudness() {
    let quiet = render(&[], &[(0, note(0, true, 69, 0.25, 1))], 60);
    let loud = render(&[], &[(0, note(0, true, 69, 1.0, 1))], 60);
    let (q, l) = (rms(&quiet[6_000..]), rms(&loud[6_000..]));
    assert!(
        l > 2.0 * q,
        "velocity must order loudness: {q:.4} vs {l:.4}"
    );
}

/// The instrument's dry law: no notes, BIT-zero output — no idle
/// hiss, no denormal fizz, nothing.
#[test]
fn no_notes_is_bit_zero() {
    let out = render(&[], &[], 40);
    for (i, s) in out.iter().enumerate() {
        assert!(s.to_bits() == 0.0f32.to_bits(), "sample {i}: {s}");
    }
}

/// A note-on at frame 173 of block 2 makes its FIRST sound at
/// exactly sample 2·256+173 — zero before, sound after.
#[test]
fn mid_block_starts_are_sample_exact() {
    let at = 2 * BLOCK + 173;
    let out = render(&[], &[(2, note(173, true, 69, 1.0, 1))], 8);
    assert!(
        out[..at].iter().all(|s| *s == 0.0),
        "sound before the note-on offset"
    );
    assert!(
        out[at..at + 64].iter().any(|s| s.abs() > 0.0),
        "no sound at the offset"
    );
}

/// After note-off the tail decays at the DIALED release: one τ down
/// the level is e⁻¹ of the sustained level (±25% — envelope × sine
/// RMS windowing), and past the park point (τ·ln(sustain/1e-4) ≈
/// 2.7 s at these knobs) it is gone.
#[test]
fn release_rings_at_the_dialed_tau() {
    let r_ms = 300.0;
    let set = [("release", f64::from(r_ms)), ("sustain", 0.8)];
    let hold_blocks = 100; // ~0.53 s — settled at sustain
    let off_at = hold_blocks * BLOCK;
    let events = [
        (0, note(0, true, 69, 1.0, 1)),
        (hold_blocks, note(0, false, 69, 1.0, 1)),
    ];
    let out = render(&set, &events, 800);
    let sustained = rms(&out[off_at - 4_800..off_at]);
    let tau_n = (r_ms * 0.001 * FS) as usize;
    let after_tau = rms(&out[off_at + tau_n - 1_200..off_at + tau_n + 1_200]);
    let expected = sustained * (-1.0f64).exp();
    assert!(
        (after_tau - expected).abs() < 0.25 * expected,
        "one τ after off: {after_tau:.4} vs expected {expected:.4} (sustained {sustained:.4})"
    );
    let far = rms(&out[off_at + 3 * FS as usize..]);
    assert!(far < 1e-4, "the tail never dies: {far:.2e}");
}

/// Seventeen notes into a sixteen-voice pool: the newest still
/// sounds (never a dropped note) and the output stays bounded —
/// the steal, exercised through the real plugin.
#[test]
fn the_seventeenth_note_still_sounds() {
    let mut events = Vec::new();
    for i in 0..17i32 {
        // Chromatic stack from key 40, one note per block.
        events.push((i as usize, note(0, true, 40 + i as i16, 0.7, i)));
    }
    let out = render(&[], &events, 120);
    let sustained = &out[30_000..];
    let newest_hz = 440.0 * f64::powf(2.0, (40.0 + 16.0 - 69.0) / 12.0);
    let newest = mag_at(sustained, newest_hz);
    assert!(newest > 1e-4, "the 17th note was dropped ({newest:.2e})");
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak < 4.0, "17-note stack unbounded: peak {peak}");
}

/// THE MATRIX LAWS. Routes are ordinary params; these laws are what
/// make them TRUSTWORTHY:
/// * a route at depth 0 (and an OFF slot) is BIT-identical to no
///   route — the matrix at rest costs nothing and changes nothing;
/// * displacement is BOUNDED by the declared range (VELOCITY→LEVEL
///   at full depth moves exactly ±12 dB across the velocity range);
/// * per-voice sources are PER-VOICE: two held notes with different
///   velocities get measurably different treatment in ONE render;
/// * LFO→PITCH is vibrato: sidebands appear at the carrier ± the
///   dialed LFO rate (Carson says so; the correlator agrees).
#[test]
fn depth_zero_routes_are_bit_identical_to_no_routes() {
    let ev = [(0, note(0, true, 69, 0.8, 1))];
    let clean = render(&[], &ev, 60);
    let armed_but_zero = render(
        &[
            ("src 1", 1.0),
            ("tgt 1", 0.0),
            ("depth 1", 0.0),
            ("src 2", 3.0),
            ("depth 2", 0.0),
        ],
        &ev,
        60,
    );
    for (i, (a, b)) in clean.iter().zip(&armed_but_zero).enumerate() {
        assert!(a.to_bits() == b.to_bits(), "sample {i}: {a} vs {b}");
    }
}

#[test]
fn velocity_to_level_is_bounded_by_the_declared_range() {
    // Full-depth VELOCITY→LEVEL: vel 1.0 → +12 dB, vel 0.5 → 0 dB.
    // The RMS ratio between those notes must be 12 dB beyond the
    // velocity scaling itself (±0.75 dB — envelope windowing).
    let set = [
        ("src 1", 2.0),
        ("tgt 1", 1.0),
        ("depth 1", 1.0),
        ("sustain", 0.8),
    ];
    let loud = render(&set, &[(0, note(0, true, 69, 1.0, 1))], 80);
    let mid = render(&set, &[(0, note(0, true, 69, 0.5, 1))], 80);
    let (l, m) = (rms(&loud[12_000..]), rms(&mid[12_000..]));
    // Undo the plain velocity term (×2), leaving the route's dB.
    let route_db = 20.0 * (l / m / 2.0).log10();
    assert!(
        (route_db - 12.0).abs() < 0.75,
        "route moved {route_db:.2} dB; declared 12 dB"
    );
}

#[test]
fn per_voice_sources_are_per_voice() {
    // One render, two held notes, opposite velocities, full
    // VELOCITY→LEVEL: the loud note's pitch dominates its quiet
    // neighbor by MORE than velocity alone explains.
    let set = [
        ("src 1", 2.0),
        ("tgt 1", 1.0),
        ("depth 1", 1.0),
        ("sustain", 0.8),
    ];
    let ev = [
        (0, note(0, true, 60, 1.0, 1)),
        (0, note(0, true, 67, 0.25, 2)),
    ];
    let out = render(&set, &ev, 80);
    let s = &out[12_000..];
    let loud = mag_at(s, 261.6256);
    let quiet = mag_at(s, 391.9954);
    // Velocity alone: 4×. Route adds (vel 1 → +12 dB) vs (vel .25
    // → −6 dB): 18 dB ≈ 8× more. Assert well past velocity alone.
    assert!(
        loud > 12.0 * quiet,
        "per-voice level route too weak: {loud:.2e} vs {quiet:.2e}"
    );
}

#[test]
fn lfo_to_pitch_is_vibrato_with_sidebands_at_the_dialed_rate() {
    // Depth 0.02 of the ±12 st range ≈ ±0.24 st ≈ ±6 Hz at 440:
    // honest vibrato (β ≈ 1.2), so the carrier survives and the
    // first-order sidebands stand out. (0.25 is a three-semitone
    // SIREN — the fan swallows the carrier, measured.)
    let set = [
        ("src 1", 1.0),
        ("tgt 1", 0.0),
        ("depth 1", 0.02),
        ("lfo rate", 5.0),
        ("sustain", 0.8),
    ];
    let out = render(&set, &[(0, note(0, true, 69, 1.0, 1))], 200);
    let s = &out[24_000..];
    let carrier = mag_at(s, 440.0);
    let sb_up = mag_at(s, 445.0);
    let sb_down = mag_at(s, 435.0);
    let off_grid = mag_at(s, 447.5); // between FM orders
    assert!(
        sb_up > 3.0 * off_grid && sb_down > 3.0 * off_grid,
        "vibrato sidebands missing at ±5 Hz: up {sb_up:.2e} down {sb_down:.2e} vs {off_grid:.2e}"
    );
    assert!(carrier > 0.0, "carrier vanished");
}

/// THE M-MOD LAWS — the new sources and shapers, measured:
/// * RISE fades vibrato in: sidebands grow from note-on to steady
///   state (a player's vibrato, not a machine's);
/// * HEAT is fatigue: after a flurry of hard notes, a HEAT→LEVEL
///   route makes the last note ring louder than the first did.
#[test]
fn lfo_rise_fades_the_vibrato_in() {
    let set: Vec<(&str, f64)> = vec![
        ("sustain", 0.8),
        ("lfo rate", 8.0),
        ("lfo rise", 1_500.0),
        ("src 1", 1.0), // LFO
        ("tgt 1", 0.0), // PITCH
        // Depth 0.02 keeps the full-ramp modulation index under 1
        // (the vibrato law's Bessel lesson: past beta ~ 2 the FIRST
        // sideband/carrier ratio INVERTS as energy fans out).
        ("depth 1", 0.02),
    ];
    let out = render(&set, &[(0, note(0, true, 69, 1.0, 1))], 600); // 3.2 s
    let fc = 440.0;
    let sb = |w: &[f32]| (mag_at(w, fc + 8.0) + mag_at(w, fc - 8.0)) / mag_at(w, fc).max(1e-12);
    let early = sb(&out[4_800..28_800]); // 0.1–0.6 s: ramp barely open
    let late = sb(&out[120_000..144_000]); // 2.5–3.0 s: ramp at 1
    assert!(
        late > 3.0 * early,
        "rise must fade vibrato in: early {early:.4} vs late {late:.4}"
    );
}

#[test]
fn heat_is_fatigue_that_accumulates_and_brightens() {
    // Twenty fortissimo strikes, 100 ms apart; HEAT→LEVEL unipolar.
    // The pump: each strike adds ~0.12, τ = 8 s, so by the last
    // strike the integrator is hot and the route lifts its level.
    let set: Vec<(&str, f64)> = vec![
        ("decay", 200.0),
        ("src 1", 6.0), // HEAT
        ("tgt 1", 1.0), // LEVEL
        ("depth 1", 1.0),
        ("mode 1", 1.0), // UNI: heat only ever ADDS
    ];
    let blocks_per_note = 19; // ≈101 ms at 256/48k
    let events: Vec<(usize, Event)> = (0..20)
        .flat_map(|i| {
            let b = i * blocks_per_note;
            [
                (b, note(0, true, 69, 1.0, i as i32 * 2 + 1)),
                (b + 12, note(0, false, 69, 1.0, i as i32 * 2 + 1)),
            ]
        })
        .collect();
    let total_blocks = 20 * blocks_per_note + 40;
    let hot = render(&set, &events, total_blocks);
    let cold = render(&[("decay", 200.0)], &events, total_blocks);
    // Compare the LAST note's early body to the FIRST note's, in
    // the heat render normalized by the cold render (removes any
    // steal/phase artifacts common to both).
    let span = BLOCK * blocks_per_note;
    let win = 4_096;
    let first_hot = rms(&hot[1_000..1_000 + win]);
    let last_hot = rms(&hot[19 * span + 1_000..19 * span + 1_000 + win]);
    let first_cold = rms(&cold[1_000..1_000 + win]);
    let last_cold = rms(&cold[19 * span + 1_000..19 * span + 1_000 + win]);
    let gain = (last_hot / last_cold) / (first_hot / first_cold);
    assert!(
        gain > 1.6,
        "twenty hard strikes must heat the level up: relative gain {gain:.3}"
    );
}
