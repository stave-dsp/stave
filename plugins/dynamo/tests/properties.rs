//! Dynamo's OWN laws — the promises only a synth can state, every
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
    let mut plugin = stave_dynamo_vst3::Dynamo::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    // THE BENCH: laws measure a CLEAN machine unless they dial a
    // subsystem in themselves — the shipping defaults are lively
    // the measurements are surgical.
    for (n, v) in [
        ("hum", 0.0),
        ("bearing", 0.0),
        ("clank", 0.0),
        ("crackle", 0.0),
        ("tension", 0.0),
    ] {
        table.by_name(n).unwrap().set_plain(v);
    }
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

/// Both channels (the width law needs the field).
fn render_stereo(
    set: &[(&str, f64)],
    events: &[(usize, Event)],
    blocks: usize,
) -> (Vec<f32>, Vec<f32>) {
    let mut plugin = stave_dynamo_vst3::Dynamo::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    // THE BENCH: laws measure a CLEAN machine unless they dial a
    // subsystem in themselves — the shipping defaults are lively
    // the measurements are surgical.
    for (n, v) in [
        ("hum", 0.0),
        ("bearing", 0.0),
        ("clank", 0.0),
        ("crackle", 0.0),
        ("tension", 0.0),
    ] {
        table.by_name(n).unwrap().set_plain(v);
    }
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut left = Vec::with_capacity(blocks * BLOCK);
    let mut right = Vec::with_capacity(blocks * BLOCK);
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
        left.extend_from_slice(&out_l);
        right.extend_from_slice(&out_r);
    }
    let _ = Arc::strong_count(&bus);
    (left, right)
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
                                                  // The MACHINE's line is wider than an oscillator's: the
                                                  // wire pair is detuned ±4 cents by design (the chorus IS
                                                  // the product) — 5× beats the probe, 10× was osc-era.
        assert!(
            on > 5.0 * off,
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

/// A note-on at frame 173 of block 2 leaks NOTHING before sample
/// 2·256+173, and its strike reaches the bridge within one wire
/// trip (the body is the only voice — the wave must travel the
/// string first; at 440 Hz that is ~109 samples).
#[test]
fn mid_block_starts_are_sample_exact() {
    let at = 2 * BLOCK + 173;
    let out = render(&[], &[(2, note(173, true, 69, 1.0, 1))], 8);
    assert!(
        out[..at].iter().all(|s| *s == 0.0),
        "sound before the note-on offset"
    );
    assert!(
        out[at..at + 192].iter().any(|s| s.abs() > 0.0),
        "no sound within one wire trip"
    );
}

/// After note-off the tail decays at the DIALED release: one τ down
/// the level is e⁻¹ of the sustained level (±25% — envelope × sine
/// RMS windowing), and past the park point (τ·ln(sustain/1e-4) ≈
/// 2.7 s at these knobs) it is gone.
#[test]
fn release_rings_at_the_dialed_tau() {
    // The ENVELOPE's law: a FAST body (T60 50 ms) tracks the
    // exciter, so the RMS decay reads the envelope, not the ring
    // (the wire's own ring has its own law).
    let r_ms = 300.0;
    let set = [
        ("release", f64::from(r_ms)),
        ("sustain", 0.8),
        ("ring", 0.05),
        ("sag", 0.0),
    ];
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
    // Exciter path, stiff supply: the BODY compresses loud notes
    // for real (sag detunes the rotor off the wire's resonance —
    // measured 3 dB of the route eaten) — that physics gets its
    // own story; this law states the DECLARED route range.
    let set = [
        ("src 1", 2.0),
        ("tgt 1", 1.0),
        ("depth 1", 1.0),
        ("sustain", 0.8),
        ("grit", 0.0),
        ("sag", 0.0),
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
    // Exciter path, stiff supply: the BODY compresses loud notes
    // for real (sag detunes the rotor off the wire's resonance —
    // measured 3 dB of the route eaten) — that physics gets its
    // own story; this law states the DECLARED route range.
    let set = [
        ("src 1", 2.0),
        ("tgt 1", 1.0),
        ("depth 1", 1.0),
        ("sustain", 0.8),
        ("grit", 0.0),
        ("sag", 0.0),
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
    // The MACHINE's terms: a heavy rotor low-passes vibrato (mass
    // is a real pitch filter) and poles add sidebands of their own.
    let set = [
        ("src 1", 1.0),
        ("tgt 1", 0.0),
        ("depth 1", 0.02),
        ("lfo rate", 5.0),
        ("sustain", 0.8),
        ("mass", 5.0),
        ("poles", 0.0),
        ("sag", 0.0),
        ("ring", 0.1),
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
        ("mass", 5.0),
        ("poles", 0.0),
        ("sag", 0.0),
        ("ring", 0.1),
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

/// THE ROTOR LAWS — the machine's character,
/// measured:
/// * MASS: a heavy rotor SPINS UP — the pitch arrives, it doesn't
///   appear (zero-crossing rate grows from note-on to steady);
/// * SAG: the supply loads — a fortissimo note flattens ITSELF
///   (sag=1 pulls the carrier measurably below the temperament);
/// * POLES: commutation adds harmonics AT the pole count without
///   leaving the pitch (AM sidebands around the fundamental);
/// * WHEEL: the encoder pattern gates the cycle audibly.
#[test]
fn the_rotor_spins_up() {
    // Clean rotor read: grit's noise crossings would salt the
    // zero-crossing pitch estimate (measured: 1090 "Hz").
    let set = [
        ("mass", 800.0),
        ("poles", 0.0),
        ("sag", 0.0),
        ("sustain", 0.8),
        ("grit", 0.0),
        ("ring", 0.05),
    ];
    let out = render(&set, &[(0, note(0, true, 69, 1.0, 200))], 300);
    let crossings = |w: &[f32]| {
        w.windows(2).filter(|p| p[0] < 0.0 && p[1] >= 0.0).count() as f64
            / (w.len() as f64 / f64::from(FS))
    };
    let early = crossings(&out[2_400..9_600]); // 0.05–0.2 s
    let late = crossings(&out[48_000..57_600]); // 1.0–1.2 s
                                                // τ = 800 ms: at t ≈ 1.1 s the shaft sits at 1 − e^(−1.375τ)
                                                // ≈ 75% of the commanded 440 Hz — the law pins the PHYSICS,
                                                // not just "it got faster" (measured 330 Hz, the exact
                                                // one-pole prediction).
    assert!(
        late > 4.0 * early && (250.0..390.0).contains(&late),
        "an 800 ms rotor must SPIN UP on its one-pole: early {early:.0} Hz-ish vs late {late:.0}"
    );
}

#[test]
fn the_load_sags_the_pitch() {
    // steam 1: the boiler out of circuit — its starvation ADDS to
    // the load and the declared −0.6 st math is the sag term alone.
    let base = [
        ("mass", 5.0),
        ("poles", 0.0),
        ("sustain", 0.8),
        ("steam", 1.0),
    ];
    let render_sag = |sag: f64| {
        let mut set = base.to_vec();
        set.push(("sag", sag));
        render(&set, &[(0, note(0, true, 69, 1.0, 1))], 200)
    };
    let stiff = render_sag(0.0);
    let soft = render_sag(1.0);
    let (s0, s1) = (&stiff[24_000..], &soft[24_000..]);
    let on_pitch_stiff = mag_at(s0, 440.0);
    let on_pitch_soft = mag_at(s1, 440.0);
    assert!(
        on_pitch_stiff > 3.0 * on_pitch_soft,
        "a loaded supply must flatten the note: stiff {on_pitch_stiff:.2e} vs soft {on_pitch_soft:.2e}"
    );
    // And the flattened carrier is WHERE THE DECLARATION says:
    // load = level/4 = 0.2, sag range 3 st → −0.6 st ≈ 425 Hz.
    let flat_hz = 440.0 * (-0.6f64 / 12.0).exp2();
    assert!(
        mag_at(s1, flat_hz) > 3.0 * on_pitch_soft,
        "the sagged carrier must land near {flat_hz:.0} Hz"
    );
}

#[test]
fn poles_grow_commutation_harmonics() {
    let render_poles = |idx: f64| {
        let set = [
            ("mass", 5.0),
            ("sag", 0.0),
            ("sustain", 0.8),
            ("poles", idx),
        ];
        render(&set, &[(0, note(0, true, 69, 1.0, 1))], 160)
    };
    let one = render_poles(0.0); // 1 pole
    let eight = render_poles(5.0); // 8 poles
    let sidebands = |w: &[f32]| {
        let s = &w[24_000..];
        (mag_at(s, 7.0 * 440.0) + mag_at(s, 9.0 * 440.0)) / mag_at(s, 440.0).max(1e-12)
    };
    let (lo, hi) = (sidebands(&one), sidebands(&eight));
    assert!(
        hi > 5.0 * lo,
        "8 poles must put commutation energy at 7f0/9f0: {hi:.4} vs 1 pole {lo:.4}"
    );
}

#[test]
fn the_wheel_gates_audibly() {
    let render_wheel = |idx: f64| {
        let set = [
            ("mass", 5.0),
            ("sag", 0.0),
            ("sustain", 0.8),
            ("wheel", idx),
        ];
        render(&set, &[(0, note(0, true, 69, 1.0, 1))], 120)
    };
    let solid = render_wheel(0.0);
    let pcm = render_wheel(7.0);
    let rms = |w: &[f32]| {
        (w[24_000..]
            .iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum::<f64>()
            / (w.len() - 24_000) as f64)
            .sqrt()
    };
    let (full, gated) = (rms(&solid), rms(&pcm));
    assert!(
        gated < 0.85 * full,
        "the PCM wheel must bite the duty cycle: {gated:.4} vs solid {full:.4}"
    );
}

/// THE WIRE LAWS — the body, measured:
/// * the body RINGS past the envelope (a struck wire, not a VCA);
/// * the ring decays at the DIALED T60;
/// * STIFFNESS stretches the free partials sharp (dispersion);
/// * MEMORY is history: a loud past audibly shapes a quiet present
///   on the same wire, and without magnetism it doesn't.
#[test]
fn the_body_rings_past_the_envelope() {
    let strike = |ring: f64| {
        let set = [
            ("attack", 0.5),
            ("decay", 100.0),
            ("sustain", 0.0),
            ("release", 2.0),
            ("ring", ring),
            ("sag", 0.0),
        ];
        let evs = [
            (0, note(0, true, 57, 1.0, 1)),
            (20, note(0, false, 57, 1.0, 1)),
        ];
        render(&set, &evs, 300)
    };
    let long = strike(3.0);
    let short = strike(0.05);
    // 1.0–1.5 s: the exciter died before 0.4 s; only the wire rings.
    let tail = |w: &[f32]| rms(&w[48_000..72_000]);
    let (l, s) = (tail(&long), tail(&short));
    assert!(
        l > 20.0 * s.max(1e-9),
        "a 3 s wire must outring a 50 ms one: {l:.2e} vs {s:.2e}"
    );
}

#[test]
fn the_ring_decays_at_the_dialed_t60() {
    let set = [
        ("attack", 0.5),
        ("decay", 100.0),
        ("sustain", 0.0),
        ("release", 2.0),
        ("ring", 1.0),
        ("sag", 0.0),
    ];
    let evs = [
        (0, note(0, true, 57, 1.0, 1)),
        (20, note(0, false, 57, 1.0, 1)),
    ];
    let out = render(&set, &evs, 300);
    // T60 = 1 s → −30 dB per half second: windows at 0.5 s and
    // 1.0 s must sit ~0.0316× apart (loose band for the damping
    // one-pole's extra loss).
    let a = rms(&out[24_000..28_800]);
    let b = rms(&out[48_000..52_800]);
    let ratio = b / a.max(1e-12);
    assert!(
        (0.01..0.09).contains(&ratio),
        "T60=1s must decay ~-30dB per 0.5s: ratio {ratio:.4}"
    );
}

#[test]
fn stiffness_stretches_the_partials() {
    let strike = |stiff: f64| {
        let set = [
            ("attack", 0.5),
            ("decay", 100.0),
            ("sustain", 0.0),
            ("release", 2.0),
            ("ring", 2.5),
            ("sag", 0.0),
            ("stiff", stiff),
        ];
        let evs = [
            (0, note(0, true, 45, 1.0, 1)),
            (20, note(0, false, 45, 1.0, 1)),
        ];
        render(&set, &evs, 300)
    };
    // The FREE tail (0.5 s on) vibrates at the wire's own modes.
    // Dispersion is a RATIO statement: partial 2 sharpens relative
    // to the wire's own fundamental (absolute Hz confounds with
    // the loop's overall tuning shift — measured that mistake).
    let peak_near = |w: &[f32], center: f64, span: f64| {
        let tail = &w[24_000..72_000];
        let mut best = (0.0f64, center);
        let mut probe = center - span;
        while probe < center + span {
            let m = mag_at(tail, probe);
            if m > best.0 {
                best = (m, probe);
            }
            probe += 0.25;
        }
        best.1
    };
    let ratio_of = |w: &[f32]| {
        let f0 = peak_near(w, 110.0, 8.0);
        let p2 = peak_near(w, 2.0 * f0, 15.0);
        p2 / f0
    };
    let straight = ratio_of(&strike(0.0));
    let stretched = ratio_of(&strike(0.9));
    assert!(
        stretched > straight + 0.004,
        "dispersion must stretch partial 2 sharp: ratio {straight:.4} → {stretched:.4}"
    );
}

#[test]
fn memory_is_history() {
    // (a) a quiet note alone; (b) a LOUD note first, gone by the
    // time the same quiet note plays ON THE SAME WIRE (first-free
    // voice reuse). With magnetism the loud past shapes the quiet
    // present; without it the two renders' quiet notes match.
    let run = |memory: f64, with_history: bool| {
        // mass 1 ms: an instant rotor, so a REUSED voice's spin
        // state (it keeps the old note's speed — the machine
        // ethos) cannot confound the ring measurement (it did:
        // memory-0 control read 0.70 with the default mass).
        let set = [
            ("attack", 0.5),
            ("decay", 200.0),
            ("sustain", 0.0),
            ("release", 2.0),
            ("ring", 0.6),
            ("sag", 0.0),
            ("memory", memory),
            ("mass", 1.0),
        ];
        let mut evs = vec![
            (375, note(0, true, 57, 0.35, 99)),
            (395, note(0, false, 57, 0.35, 99)),
        ];
        if with_history {
            // Magnetize the WHOLE pool: sixteen fortissimo strikes
            // so whichever voice the allocator hands the quiet note,
            // its wires carry the loud past (voice allocation is not
            // first-free — measured: a single-note history landed on
            // a different voice and the effect read exactly 0.0).
            for k in 0..16 {
                evs.push((0, note(0, true, 45 + k as i16, 1.0, k + 1)));
                evs.push((94, note(0, false, 45 + k as i16, 1.0, k + 1)));
            }
        }
        render(&set, &evs, 500)
    };
    // MEASURE THE CLAIM ITSELF: a magnetized wire rings SHORTER
    // (eddy braking). Two earlier metrics failed honestly — a
    // with/without-history diff confounds the reused rotor's spin
    // state, and a memory-on/off diff drowns in the loop
    // saturation's instant shaping. The braking shows in the TAIL
    // — measured INSIDE the pair's first beat half-period (±4
    // cents at 220 Hz beats at ~1 Hz; a later window sat in the
    // cancellation null and read 0.0000, chorus physics sabotaging
    // the metric).
    let ring_ratio = |w: &[f32]| {
        let body = rms(&w[101_760..104_640]); // 2.12–2.18 s
        let tail = rms(&w[109_440..115_200]); // 2.28–2.40 s
        tail / body.max(1e-12)
    };
    let braked = ring_ratio(&run(1.0, true));
    let free = ring_ratio(&run(1.0, false));
    assert!(
        braked < 0.7 * free,
        "a magnetized wire must ring shorter: braked {braked:.4} vs free {free:.4}"
    );
    // And WITHOUT magnetism, history leaves the ring alone (the
    // rotor spin state is neutralized by the ratio metric).
    let control_hist = ring_ratio(&run(0.0, true));
    let control_alone = ring_ratio(&run(0.0, false));
    let control = control_hist / control_alone.max(1e-12);
    assert!(
        (0.8..1.25).contains(&control),
        "memory 0 must not brake: history/alone ring ratio {control:.4}"
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_memory_render() {
    let run = |memory: f64, with_history: bool| {
        let set = [
            ("attack", 0.5),
            ("decay", 200.0),
            ("sustain", 0.0),
            ("release", 2.0),
            ("ring", 0.6),
            ("sag", 0.0),
            ("memory", memory),
            ("mass", 1.0),
        ];
        let mut evs = vec![
            (375, note(0, true, 57, 0.35, 99)),
            (395, note(0, false, 57, 0.35, 99)),
        ];
        if with_history {
            for k in 0..16 {
                evs.push((0, note(0, true, 45 + k as i16, 1.0, k + 1)));
                evs.push((94, note(0, false, 45 + k as i16, 1.0, k + 1)));
            }
        }
        render(&set, &evs, 500)
    };
    for (label, mem, hist) in [
        ("mem1 hist", 1.0, true),
        ("mem1 alone", 1.0, false),
        ("mem0 hist", 0.0, true),
        ("mem0 alone", 0.0, false),
    ] {
        let w = run(mem, hist);
        let body = rms(&w[101_760..104_640]);
        let tail = rms(&w[109_440..115_200]);
        println!(
            "{label}: body {body:.6} tail {tail:.6} ratio {:.5}",
            tail / body.max(1e-12)
        );
    }
}

/// THE CONTACT + FIELD LAWS — how the exciter touches, and
/// where the body sits in space:
/// * WIDTH splits the pair: at 0 the channels are BIT-identical,
///   at 1 they audibly differ;
/// * STRIKE lets go: a sustained note DECAYS (the body sings, the
///   grind would have held it);
/// * BOW self-limits: quieter than the grind, but alive;
/// * GRIT lives in the body: noise between the partials, gated by
///   the envelope (silence stays silence).
#[test]
fn width_is_the_pair_split() {
    let render_lr = |width: f64| {
        let set = [("sustain", 0.8), ("width", width), ("sag", 0.0)];
        render_stereo(&set, &[(0, note(0, true, 57, 1.0, 1))], 100)
    };
    let (l0, r0) = render_lr(0.0);
    for (i, (l, r)) in l0.iter().zip(&r0).enumerate() {
        assert!(
            l.to_bits() == r.to_bits(),
            "width 0 must be mono: sample {i}"
        );
    }
    let (l1, r1) = render_lr(1.0);
    let diff: f64 = l1[24_000..]
        .iter()
        .zip(&r1[24_000..])
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).abs())
        .sum::<f64>()
        / (l1.len() - 24_000) as f64;
    let level: f64 =
        l1[24_000..].iter().map(|a| f64::from(a.abs())).sum::<f64>() / (l1.len() - 24_000) as f64;
    assert!(
        diff > 0.2 * level,
        "width 1 must split the pair: mean |L-R| {diff:.5} vs level {level:.5}"
    );
}

#[test]
fn strike_contact_lets_the_body_sing() {
    let render_contact = |mode: f64| {
        let set = [
            ("sustain", 0.8),
            ("ring", 1.0),
            ("sag", 0.0),
            ("contact", mode),
            ("grit", 0.0),
        ];
        render(&set, &[(0, note(0, true, 57, 1.0, 1))], 400)
    };
    let sustain_ratio = |w: &[f32]| rms(&w[72_000..96_000]) / rms(&w[4_800..14_400]).max(1e-12);
    let grind = sustain_ratio(&render_contact(0.0));
    let strike = sustain_ratio(&render_contact(1.0));
    assert!(
        strike < 0.4 * grind,
        "STRIKE must let the note decay while GRIND holds: strike {strike:.4} vs grind {grind:.4}"
    );
    // But the body DID sing: the strike's ring is not silence.
    let struck = render_contact(1.0);
    assert!(
        rms(&struck[24_000..48_000]) > 5e-4,
        "the struck body must ring"
    );
}

#[test]
fn bow_self_limits() {
    let render_contact = |mode: f64| {
        let set = [
            ("sustain", 0.8),
            ("sag", 0.0),
            ("contact", mode),
            ("grit", 0.0),
        ];
        render(&set, &[(0, note(0, true, 57, 1.0, 1))], 120)
    };
    let grind = rms(&render_contact(0.0)[24_000..]);
    let bow = rms(&render_contact(2.0)[24_000..]);
    assert!(
        bow < 0.8 * grind && bow > 0.05 * grind,
        "the bow must limit without dying: bow {bow:.4} vs grind {grind:.4}"
    );
}

#[test]
fn grit_lives_in_the_body() {
    let render_grit = |grit: f64| {
        let set = [("sustain", 0.8), ("sag", 0.0), ("grit", grit)];
        render(&set, &[(0, note(0, true, 57, 1.0, 1))], 120)
    };
    let clean = render_grit(0.0);
    let dirty = render_grit(1.0);
    // Everything but the noise is deterministic and identical, so
    // the RENDER DIFFERENCE is the grit, isolated exactly (spectral
    // probes sat on partial skirts and read nothing, twice).
    let n = clean.len();
    let diff = (clean
        .iter()
        .zip(&dirty)
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum::<f64>()
        / n as f64)
        .sqrt();
    let level = rms(&clean[24_000..]);
    assert!(
        diff > 0.05 * level,
        "grit must be audible in the body: diff {diff:.5} vs level {level:.5}"
    );
}

/// THE MACHINE LAWS — the bridge and the boiler:
/// * a small boiler DROOPS under a chord and a huge one doesn't;
/// * the coupled bridge is PASSIVE (fortissimo chords at full
///   couple decay to silence, never blow up);
/// * SYMPATHY: a struck note audibly changes what a held note's
///   wires do — and without the bridge it doesn't.
#[test]
fn the_boiler_droops_under_chords() {
    let chord = |steam: f64| {
        // ring 0.05: the droop acts on the DRIVE (0.4 + 0.6·p); a
        // long ring's integration swamps it (0.92 ratio, measured)
        // — a fast body tracks the starving exciter.
        let set = [
            ("sustain", 0.9),
            ("sag", 0.0),
            ("steam", steam),
            ("grit", 0.0),
            ("ring", 0.05),
        ];
        let evs: Vec<(usize, Event)> = (0..8)
            .map(|k| (0, note(0, true, 48 + k as i16 * 2, 1.0, k + 1)))
            .collect();
        render(&set, &evs, 300)
    };
    // Early window 0.05–0.2 s: the boiler is still pressurized
    // (at the old 0.15–0.35 s it had already drained — ratio read
    // 0.79 vs 0.93, direction right, discrimination gone).
    let droop = |w: &[f32]| rms(&w[67_200..76_800]) / rms(&w[2_400..9_600]).max(1e-12);
    let small = droop(&chord(0.0));
    let huge = droop(&chord(1.0));
    assert!(
        small < 0.7 * huge,
        "a small boiler must droop under the chord: {small:.4} vs huge {huge:.4}"
    );
}

#[test]
fn the_coupled_bridge_is_passive() {
    // release 30 ms: measure the WIRES' death, not the release
    // envelope (the 1200 ms default kept GRIND driving through the
    // tail window — 'tail 0.41 vs body 0.39' was the exciter, not
    // instability... after the REAL instability was fixed).
    let set = [
        ("sustain", 0.8),
        ("couple", 1.0),
        ("ring", 2.0),
        ("memory", 0.5),
        ("sag", 0.5),
        ("release", 30.0),
    ];
    let mut evs: Vec<(usize, Event)> = (0..8)
        .map(|k| (0, note(0, true, 48 + k as i16 * 2, 1.0, k + 1)))
        .collect();
    for k in 0..8 {
        evs.push((150, note(0, false, 48 + k as i16 * 2, 1.0, k + 1)));
    }
    let out = render(&set, &evs, 500); // offs at 0.8 s; tail to 2.7 s
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak < 4.0, "the bridge must never blow up: peak {peak}");
    let tail = rms(&out[115_200..128_000]); // 2.4–2.7 s
    let body = rms(&out[24_000..36_000]);
    assert!(
        tail < 0.02 * body.max(1e-9),
        "released coupled chord must die: tail {tail:.2e} vs body {body:.2e}"
    );
}

#[test]
fn the_bridge_is_sympathy() {
    // A held quiet note; a loud staccato strike beside it. The
    // render DIFFERENCE (held-alone vs held+strike) in the window
    // AFTER the strike is dead measures what bled into the held
    // wires — and must collapse when the bridge opens (couple 0).
    let run = |couple: f64, with_strike: bool| {
        // release 30 ms: the strike must actually DIE before the
        // window (the 1200 ms default kept its exciter driving —
        // 'sympathy' read 245 vs 248, all of it the strike itself).
        let set = [
            ("sustain", 0.9),
            ("ring", 1.5),
            ("sag", 0.0),
            ("grit", 0.0),
            ("couple", couple),
            ("steam", 1.0),
            ("release", 30.0),
        ];
        let mut evs = vec![(0, note(0, true, 57, 0.3, 1))];
        if with_strike {
            evs.push((40, note(0, true, 64, 1.0, 2)));
            evs.push((60, note(0, false, 64, 1.0, 2)));
        }
        render(&set, &evs, 300)
    };
    let bleed = |couple: f64| {
        let alone = run(couple, false);
        let beside = run(couple, true);
        // 1.2–1.6 s: strike off at 0.32 s, its ring long dead.
        alone[57_600..76_800]
            .iter()
            .zip(&beside[57_600..76_800])
            .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let coupled = bleed(1.0);
    let open = bleed(0.0);
    assert!(
        coupled > 3.0 * open.max(1e-9),
        "the bridge must carry sympathy: coupled {coupled:.3e} vs open {open:.3e}"
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_sympathy() {
    let run = |couple: f64, with_strike: bool| {
        // release 30 ms: the strike must actually DIE before the
        // window (the 1200 ms default kept its exciter driving —
        // 'sympathy' read 245 vs 248, all of it the strike itself).
        let set = [
            ("sustain", 0.9),
            ("ring", 1.5),
            ("sag", 0.0),
            ("grit", 0.0),
            ("couple", couple),
            ("steam", 1.0),
            ("release", 30.0),
        ];
        let mut evs = vec![(0, note(0, true, 57, 0.3, 1))];
        if with_strike {
            evs.push((40, note(0, true, 64, 1.0, 2)));
            evs.push((60, note(0, false, 64, 1.0, 2)));
        }
        render(&set, &evs, 300)
    };
    for (label, c, w) in [
        ("c0 alone ", 0.0, false),
        ("c0 beside", 0.0, true),
        ("c1 alone ", 1.0, false),
        ("c1 beside", 1.0, true),
    ] {
        let out = run(c, w);
        println!(
            "{label}: early {:.4} window {:.4} peak {:.3}",
            rms(&out[9_600..19_200]),
            rms(&out[57_600..76_800]),
            out.iter().fold(0.0f32, |m, x| m.max(x.abs()))
        );
    }
    let a = run(0.0, false);
    let b = run(0.0, true);
    let d: f64 = a[57_600..76_800]
        .iter()
        .zip(&b[57_600..76_800])
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>();
    println!("c0 diff-sq over window: {d:.4}");
}

/// THE ESCAPEMENT LAWS — the exciter's clock:
/// * engaged, it SUSTAINS a struck wire indefinitely (the
///   impossible EBow); disengaged, the strike dies;
/// * entrained ticks are PHASE-LOCKED: the sustained tone stays
///   harmonic (the wire is the pendulum);
/// * past the entrained regime the clock BREAKS: skipped and
///   doubled teeth spread the sustained spectrum.
#[test]
fn the_escapement_sustains_the_wire() {
    let run = |chaos: f64| {
        let set = [
            ("contact", 1.0),
            ("sustain", 0.0),
            ("decay", 150.0),
            ("release", 30.0),
            ("ring", 1.0),
            ("sag", 0.0),
            ("grit", 0.0),
            ("steam", 1.0),
            ("chaos", chaos),
        ];
        // The key stays HELD: the escapement is an EBow you lift
        // off — note-off disengages it (the anti-drone contract).
        let evs = [(0, note(0, true, 57, 1.0, 1))];
        render(&set, &evs, 600) // 3.2 s
    };
    let held = run(0.45);
    let free = run(0.0);
    let late_over_early =
        |w: &[f32]| rms(&w[120_000..144_000]) / rms(&w[24_000..48_000]).max(1e-12);
    let (sustained, dying) = (late_over_early(&held), late_over_early(&free));
    assert!(
        sustained > 0.3 && dying < 0.05,
        "the engaged escapement must sustain (got {sustained:.3}) while the free wire dies \
         (got {dying:.3})"
    );
}

#[test]
fn entrained_ticks_stay_harmonic() {
    let set = [
        ("contact", 1.0),
        ("sustain", 0.0),
        ("decay", 150.0),
        ("release", 30.0),
        ("ring", 1.0),
        ("sag", 0.0),
        ("grit", 0.0),
        ("steam", 1.0),
        ("chaos", 0.45),
        ("width", 0.0),
        ("stiff", 0.0),
    ];
    let evs = [(0, note(0, true, 57, 1.0, 1))];
    let out = render(&set, &evs, 600);
    let s = &out[120_000..150_000]; // deep in self-sustain
                                    // The escapement sustains the WIRE'S OWN mode — the pendulum's
                                    // period, not the equal-tempered command; and the ±4-cent pair
                                    // puts 220.0 exactly in the VALLEY between its two peaks
                                    // (measured 2.7× there; the peaks are clean). Scan for it.
    let mut on = 0.0f64;
    let mut probe = 210.0;
    while probe < 230.0 {
        on = on.max(mag_at(s, probe));
        probe += 0.5;
    }
    let off = mag_at(s, 220.0 * 1.06);
    assert!(
        on > 5.0 * off,
        "entrained sustain must stay harmonic: peak {on:.3e} vs off-grid {off:.3e}"
    );
}

#[test]
fn chaos_breaks_the_clock() {
    let run = |chaos: f64| {
        let set = [
            ("contact", 1.0),
            ("sustain", 0.0),
            ("decay", 150.0),
            ("release", 30.0),
            ("ring", 1.0),
            ("sag", 0.0),
            ("grit", 0.0),
            ("steam", 1.0),
            ("chaos", chaos),
            ("width", 0.0),
        ];
        let evs = [(0, note(0, true, 57, 1.0, 1))];
        render(&set, &evs, 600)
    };
    let noise_share = |w: &[f32]| {
        let s = &w[120_000..150_000];
        // Peak-scan the carrier (the pair's valley sits at 220.0).
        let mut carrier = 0.0f64;
        let mut probe = 210.0;
        while probe < 230.0 {
            carrier = carrier.max(mag_at(s, probe));
            probe += 0.5;
        }
        let floor: f64 = [341.0, 563.0, 787.0, 1_030.0]
            .iter()
            .map(|&f| mag_at(s, f))
            .sum();
        floor / carrier.max(1e-12)
    };
    let ticking = noise_share(&run(0.45));
    let broken = noise_share(&run(1.0));
    assert!(
        broken > 2.0 * ticking,
        "the broken clock must spread the spectrum: {broken:.4} vs entrained {ticking:.4}"
    );
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_spectrum() {
    let set = [
        ("contact", 1.0),
        ("sustain", 0.0),
        ("decay", 150.0),
        ("release", 30.0),
        ("ring", 1.0),
        ("sag", 0.0),
        ("grit", 0.0),
        ("steam", 1.0),
        ("chaos", 0.45),
        ("width", 0.0),
        ("stiff", 0.0),
    ];
    let evs = [(0, note(0, true, 57, 1.0, 1))];
    let out = render(&set, &evs, 600);
    let s = &out[120_000..150_000];
    for f in [
        55.0, 110.0, 165.0, 220.0, 330.0, 440.0, 660.0, 880.0, 1320.0, 1760.0,
    ] {
        println!("mag {f}: {:.6}", mag_at(s, f));
    }
    println!("rms: {:.5}", rms(s));
}

#[test]
#[ignore = "report: prints measurements, asserts nothing; run with --run-ignored only --no-capture"]
fn probe_escapement() {
    let run = |chaos: f64| {
        let set = [
            ("contact", 1.0),
            ("sustain", 0.0),
            ("decay", 150.0),
            ("release", 30.0),
            ("ring", 1.0),
            ("sag", 0.0),
            ("grit", 0.0),
            ("steam", 1.0),
            ("chaos", chaos),
        ];
        let evs = [
            (0, note(0, true, 57, 1.0, 1)),
            (20, note(0, false, 57, 1.0, 1)),
        ];
        render(&set, &evs, 600)
    };
    let held = run(0.45);
    for (label, a, b) in [
        ("0.0-0.1", 0, 4_800),
        ("0.2-0.4", 9_600, 19_200),
        ("0.5-1.0", 24_000, 48_000),
        ("1.5-2.0", 72_000, 96_000),
        ("2.5-3.0", 120_000, 144_000),
    ] {
        println!("chaos.45 {label}: {:.6}", rms(&held[a..b]));
    }
}

/// The EBow lifts off: at note-off the escapement DISENGAGES and
/// the drone dies (the field bug: chaotic voices never died, and
/// transport stop/start layered old drones under new notes).
#[test]
fn the_escapement_lifts_off_at_note_off() {
    let set = [
        ("contact", 1.0),
        ("sustain", 0.0),
        ("decay", 150.0),
        ("release", 30.0),
        ("ring", 1.0),
        ("sag", 0.0),
        ("grit", 0.0),
        ("steam", 1.0),
        ("chaos", 0.6),
    ];
    let evs = [
        (0, note(0, true, 57, 1.0, 1)),
        (280, note(0, false, 57, 1.0, 1)),
    ];
    let out = render(&set, &evs, 600);
    let sustained = rms(&out[48_000..67_200]); // held, self-sustaining
    let after = rms(&out[120_000..144_000]); // 1 s past the off
    assert!(
        sustained > 0.01 && after < 0.05 * sustained,
        "note-off must kill the drone: held {sustained:.4} vs after {after:.5}"
    );
}

/// THE MOTION LAW — a motion row IS the Atoms per-knob
/// motion: a slow square LFO into FORCE (slot 5's default target)
/// at full negative depth gates the machine on and off in time
/// with the clock. Global scope: one motion, the whole machine.
#[test]
fn motion_animates_the_macro() {
    let set = [
        ("sustain", 0.9),
        ("sag", 0.0),
        ("grit", 0.0),
        ("steam", 1.0),
        ("ring", 0.1),
        ("mass", 5.0),
        ("lfo shape", 3.0), // SQR
        ("lfo rate", 0.5),
        ("src 5", 1.0), // LFO → (default tgt 5 = FORCE)
        ("depth 5", -1.0),
        ("mode 5", 1.0), // UNI: the square's high half kills FORCE
    ];
    let out = render(&set, &[(0, note(0, true, 57, 1.0, 1))], 520);
    // Square at 0.5 Hz from phase 0: low half first (force intact),
    // high half 1–2 s (force displaced to zero).
    let loud = rms(&out[19_200..43_200]); // 0.4–0.9 s
    let gated = rms(&out[67_200..91_200]); // 1.4–1.9 s
    let back = rms(&out[105_600..129_600]); // 2.2–2.7 s
    assert!(
        loud > 5.0 * gated.max(1e-9) && back > 5.0 * gated.max(1e-9),
        "motion must gate the machine in time: loud {loud:.4} gated {gated:.4} back {back:.4}"
    );
}

/// THE LIFE LAWS — the machine's involuntary sounds, each a
/// function of STATE (never a parallel layer):
/// * HUM deepens as the boiler strains;
/// * BEARING noise follows the actual rotor speed (spin-up is
///   audible; a light rotor at speed out-hums a heavy one still
///   spinning up);
/// * CLANK marks the CONTACT — its energy lives at the note start;
/// * CRACKLE's rate rides heat (a hot machine pops more);
/// * TENSION bends a loud attack sharp, settling as it decays.
#[test]
fn hum_deepens_as_the_boiler_strains() {
    let chord = |steam: f64| {
        let set = [
            ("sustain", 0.9),
            ("sag", 0.0),
            ("hum", 1.0),
            ("steam", steam),
        ];
        let evs: Vec<(usize, Event)> = (0..8)
            .map(|k| (0, note(0, true, 48 + k as i16 * 2, 1.0, k + 1)))
            .collect();
        render(&set, &evs, 600)
    };
    // The 53 Hz leak tone, late in the chord (boiler settled).
    let leak = |w: &[f32]| mag_at(&w[96_000..144_000], 53.0);
    let strained = leak(&chord(0.0));
    let easy = leak(&chord(1.0));
    assert!(
        strained > 2.0 * easy.max(1e-9),
        "a strained boiler must hum louder: {strained:.3e} vs {easy:.3e}"
    );
}

#[test]
fn bearing_noise_follows_the_spin() {
    // A HEAVY rotor (2 s spin-up): the bearing gets louder as the
    // shaft comes up to speed — the render's noise floor grows
    // from the early window to the late one.
    let set = [
        ("sustain", 0.9),
        ("sag", 0.0),
        ("bearing", 1.0),
        ("mass", 2_000.0),
        ("poles", 0.0),
        ("grit", 0.0),
    ];
    let bare = [
        ("sustain", 0.9),
        ("sag", 0.0),
        ("bearing", 0.0),
        ("mass", 2_000.0),
        ("poles", 0.0),
        ("grit", 0.0),
    ];
    let evs = [(0, note(0, true, 57, 1.0, 1))];
    let with_bearing = render(&set, &evs, 500);
    let without = render(&bare, &evs, 500);
    let noise = |a: &[f32], b: &[f32], r: core::ops::Range<usize>| {
        a[r.clone()]
            .iter()
            .zip(&b[r])
            .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let early = noise(&with_bearing, &without, 4_800..24_000); // shaft slow
    let late = noise(&with_bearing, &without, 96_000..115_200); // at speed
    assert!(
        late > 2.0 * early.max(1e-9),
        "the bearing must speak with the spin: early {early:.3e} vs late {late:.3e}"
    );
}

#[test]
fn clank_marks_the_contact() {
    let strike = |clank: f64| {
        let set = [
            ("sustain", 0.0),
            ("decay", 200.0),
            ("sag", 0.0),
            ("clank", clank),
            ("contact", 1.0),
        ];
        render(&set, &[(0, note(0, true, 57, 1.0, 1))], 200)
    };
    let with_clank = strike(1.0);
    let without = strike(0.0);
    let diff = |r: core::ops::Range<usize>| {
        with_clank[r.clone()]
            .iter()
            .zip(&without[r])
            .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let at_contact = diff(0..1_440); // first 30 ms
    let later = diff(24_000..25_440); // same width at 0.5 s
    assert!(
        at_contact > 10.0 * later.max(1e-9),
        "the clank lives at the contact: {at_contact:.3e} vs later {later:.3e}"
    );
}

#[test]
fn crackle_rate_rides_the_heat() {
    // The same held note; before it, a flurry pumps the heat. The
    // crackle DIFFERENCE (crackle 1 vs 0, same events) is larger on
    // the hot machine.
    let run = |crackle: f64, hot: bool| {
        let set = [
            ("sustain", 0.9),
            ("sag", 0.0),
            ("crackle", crackle),
            ("steam", 1.0),
        ];
        let mut evs = vec![(190, note(0, true, 57, 0.6, 99))];
        if hot {
            for k in 0..12 {
                evs.push((k * 8, note(0, true, 45 + k as i16, 1.0, k as i32 + 1)));
                evs.push((k * 8 + 6, note(0, false, 45 + k as i16, 1.0, k as i32 + 1)));
            }
        }
        render(&set, &evs, 800)
    };
    let window = 102_400..201_600; // the held note's body
    let pops = |hot: bool| {
        let on = run(1.0, hot);
        let off = run(0.0, hot);
        on[window.clone()]
            .iter()
            .zip(&off[window.clone()])
            .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let hot = pops(true);
    let cold = pops(false);
    assert!(
        hot > 1.6 * cold.max(1e-9),
        "a hot machine must crackle more: hot {hot:.3e} vs cold {cold:.3e}"
    );
}

#[test]
fn tension_bends_the_attack_sharp() {
    let ff = |tension: f64| {
        let set = [
            ("sustain", 0.0),
            ("decay", 600.0),
            ("sag", 0.0),
            ("tension", tension),
            ("ring", 2.0),
            ("mass", 5.0),
            ("poles", 0.0),
        ];
        render(&set, &[(0, note(0, true, 69, 1.0, 1))], 600)
    };
    let bent = ff(1.0);
    let straight = ff(0.0);
    // The loud attack's energy sits ABOVE 440 with tension dialed
    // (and ON 440 without): scan for the early peak. (A settle-
    // back assertion was wrong-headed — tension detunes the drive
    // off the wire's resonance, so the 440 tail legitimately
    // charges less; the machine bends, it doesn't snap back.)
    let early_peak = |w: &[f32]| {
        let s = &w[4_800..28_800];
        let mut best = (0.0f64, 440.0f64);
        let mut probe = 436.0;
        while probe < 462.0 {
            let m = mag_at(s, probe);
            if m > best.0 {
                best = (m, probe);
            }
            probe += 0.5;
        }
        best.1
    };
    let bent_hz = early_peak(&bent);
    let straight_hz = early_peak(&straight);
    assert!(
        bent_hz > straight_hz + 2.0,
        "tension must bend the loud attack sharp: {straight_hz:.1} Hz → {bent_hz:.1} Hz"
    );
}

/// THE ANTI-DRONE LAW (field report: background tones rang forever
/// regardless of what was playing). With EVERY life dial wide open
/// the machine still falls SILENT after release: the motor powers
/// down with the envelope (bearing noise follows the dying spin),
/// crackle pops only on a powered voice, and the wires' ring runs
/// out. No input, eventually no output — a machine, not a drone.
#[test]
fn a_released_machine_falls_silent() {
    let set = [
        ("hum", 1.0),
        ("bearing", 1.0),
        ("clank", 1.0),
        ("crackle", 1.0),
        ("tension", 1.0),
        ("grit", 0.8),
        ("sustain", 0.8),
        ("release", 200.0),
        ("ring", 1.0),
    ];
    let evs = [
        (0, note(0, true, 57, 1.0, 1)),
        (150, note(0, false, 57, 1.0, 1)),
    ];
    let out = render(&set, &evs, 800); // off at 0.8 s; tail to 4.27 s
    let body = rms(&out[24_000..36_000]);
    let tail = rms(&out[178_000..204_800]); // 3.7–4.27 s
    assert!(
        body > 1e-3,
        "the lively machine must sound: body {body:.2e}"
    );
    assert!(
        tail < 1e-4,
        "a released machine must fall silent: tail {tail:.2e} vs body {body:.2e}"
    );
}
