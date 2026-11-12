//! The Voice form's invariants, driven by the TWO
//! consumers the design demands:
//! * `SineVoice` — the simplest instrument shape (Osc + Adsr);
//! * `MiniFmVoice` — a 2-operator FM voice whose MODULATOR carries
//!   its own Adsr instance: per-voice control-plane instancing must
//!   fit the trait with no framework help, BEFORE tier 2 needs it.
//!   If this voice is awkward to write, the form is wrong — that is
//!   this file's job.

use stave_dsp::{
    hz_of_key, Adsr, AdsrConfig, Osc, Stage, Steal, Voice, VoiceEvent, VoiceRack, Waveform,
};

const SR: f32 = 48_000.0;

// ------------------------------------------------------- consumer 1
struct SineVoice {
    osc: Osc,
    env: Adsr,
    vel: f32,
}

impl SineVoice {
    fn new() -> Self {
        Self {
            osc: Osc::new(Waveform::Sine),
            env: Adsr::new(),
            vel: 0.0,
        }
    }
}

impl Voice for SineVoice {
    fn prepare(&mut self, sample_rate: f32) {
        self.osc.prepare(sample_rate);
        self.env.prepare(sample_rate);
        self.env.set(&AdsrConfig {
            attack_ms: 2.0,
            decay_ms: 30.0,
            sustain: 0.7,
            release_ms: 80.0,
        });
        self.reset();
    }
    fn reset(&mut self) {
        self.env.reset();
        self.osc.reset();
        self.vel = 0.0;
    }
    fn note_on(&mut self, hz: f32, velocity: f32) {
        self.osc.set_hz(hz);
        self.osc.reset();
        self.vel = velocity;
        self.env.gate_on();
    }
    fn note_off(&mut self) {
        self.env.gate_off();
    }
    fn choke(&mut self) {
        self.env.choke();
    }
    fn active(&self) -> bool {
        self.env.active()
    }
    fn level(&self) -> f32 {
        self.env.level() * self.vel
    }
    fn tick(&mut self) -> (f32, f32) {
        let y = self.osc.tick(0.0) * self.env.tick(0.0) * self.vel;
        (y, y)
    }
}

// ------------------------------------------------------- consumer 2
/// 2-op FM: carrier phase-modulated by a modulator whose index is
/// shaped by its OWN envelope (fast decay — the classic FM pluck:
/// bright at the attack, pure by the tail).
struct MiniFmVoice {
    carrier: f64,
    modulator: f64,
    inc: f64,
    ratio: f64,
    index: f64,
    amp_env: Adsr,
    mod_env: Adsr,
    vel: f32,
    sample_rate: f32,
}

impl MiniFmVoice {
    fn new() -> Self {
        Self {
            carrier: 0.0,
            modulator: 0.0,
            inc: 0.0,
            ratio: 2.0,
            index: 3.0,
            amp_env: Adsr::new(),
            mod_env: Adsr::new(),
            vel: 0.0,
            sample_rate: SR,
        }
    }
}

impl Voice for MiniFmVoice {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.amp_env.prepare(sample_rate);
        self.amp_env.set(&AdsrConfig {
            attack_ms: 1.0,
            decay_ms: 200.0,
            sustain: 0.6,
            release_ms: 60.0,
        });
        // The modulator's OWN envelope: no sustain — the brightness
        // is an attack transient.
        self.mod_env.prepare(sample_rate);
        self.mod_env.set(&AdsrConfig {
            attack_ms: 0.5,
            decay_ms: 40.0,
            sustain: 0.0,
            release_ms: 40.0,
        });
        self.reset();
    }
    fn reset(&mut self) {
        self.amp_env.reset();
        self.mod_env.reset();
        self.carrier = 0.0;
        self.modulator = 0.0;
        self.vel = 0.0;
    }
    fn note_on(&mut self, hz: f32, velocity: f32) {
        self.inc = f64::from(hz) / f64::from(self.sample_rate);
        self.carrier = 0.0;
        self.modulator = 0.0;
        self.vel = velocity;
        self.amp_env.gate_on();
        self.mod_env.gate_on();
    }
    fn note_off(&mut self) {
        self.amp_env.gate_off();
        self.mod_env.gate_off();
    }
    fn choke(&mut self) {
        self.amp_env.choke();
        self.mod_env.choke();
    }
    fn active(&self) -> bool {
        self.amp_env.active()
    }
    fn level(&self) -> f32 {
        self.amp_env.level() * self.vel
    }
    fn tick(&mut self) -> (f32, f32) {
        use core::f64::consts::TAU;
        let m = (TAU * self.modulator).sin() * self.index * f64::from(self.mod_env.tick(0.0));
        let y = (TAU * self.carrier + m).sin() * f64::from(self.amp_env.tick(0.0) * self.vel);
        self.carrier = (self.carrier + self.inc).rem_euclid(1.0);
        self.modulator = (self.modulator + self.inc * self.ratio).rem_euclid(1.0);
        (y as f32, y as f32)
    }
}

// ------------------------------------------------------------ tools
fn on(id: i32, key: i16, vel: f32) -> VoiceEvent {
    VoiceEvent::On {
        id,
        key,
        hz: hz_of_key(key),
        velocity: vel,
    }
}
fn off(id: i32, key: i16) -> VoiceEvent {
    VoiceEvent::Off { id, key }
}

fn run<V: Voice, const MAX: usize>(rack: &mut VoiceRack<V, MAX>, n: usize) -> Vec<f32> {
    (0..n).map(|_| rack.tick().0).collect()
}

/// Single-bin correlation magnitude at `hz` (the pitch instrument).
fn mag_at(w: &[f32], hz: f64) -> f64 {
    let n = w.len() as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &x) in w.iter().enumerate() {
        let ph = core::f64::consts::TAU * hz * i as f64 / f64::from(SR);
        re += f64::from(x) * ph.cos();
        im += f64::from(x) * ph.sin();
    }
    (re * re + im * im).sqrt() / n
}

// ------------------------------------------------------------- invariants
#[test]
fn the_sum_is_superposition() {
    // A chord through one rack ≡ the same notes through separate
    // racks, summed in slot order — BIT-identical (same adds, same
    // order).
    let keys = [60i16, 64, 67];
    let mut chord: VoiceRack<SineVoice, 8> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    chord.prepare(SR);
    for (i, &k) in keys.iter().enumerate() {
        chord.dispatch(on(i as i32, k, 0.8));
    }
    let whole = run(&mut chord, 4_800);

    let mut parts = vec![0.0f32; 4_800];
    for (i, &k) in keys.iter().enumerate() {
        let mut solo: VoiceRack<SineVoice, 8> =
            VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
        solo.prepare(SR);
        solo.dispatch(on(i as i32, k, 0.8));
        for (acc, s) in parts.iter_mut().zip(run(&mut solo, 4_800)) {
            *acc += s;
        }
    }
    for (i, (a, b)) in whole.iter().zip(&parts).enumerate() {
        assert!(a.to_bits() == b.to_bits(), "sample {i}: {a} vs {b}");
    }
}

#[test]
fn dispatch_is_sample_accurate() {
    // A note dispatched at frame 173 makes its first sound AT frame
    // 173 — zero before, nonzero after (the form's whole point).
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    rack.prepare(SR);
    let mut out = Vec::with_capacity(400);
    for i in 0..400 {
        if i == 173 {
            rack.dispatch(on(1, 69, 1.0));
        }
        out.push(rack.tick().0);
    }
    assert!(
        out[..173].iter().all(|s| *s == 0.0),
        "sound before the dispatch"
    );
    assert!(
        out[173..200].iter().any(|s| s.abs() > 0.0),
        "no sound right after the dispatch"
    );
}

#[test]
fn note_off_matches_id_then_key_and_releases() {
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    rack.prepare(SR);
    // Two voices on the SAME key with distinct ids.
    rack.dispatch(on(10, 60, 0.9));
    rack.dispatch(on(11, 60, 0.9));
    run(&mut rack, 2_400);
    assert_eq!(rack.active_voices(), 2);
    // Off by id releases exactly one…
    rack.dispatch(off(10, 60));
    run(&mut rack, SR as usize); // 1 s ≫ the ~708 ms to park (0.7·e^(−t/80ms) < 1e-4)
    assert_eq!(
        rack.active_voices(),
        1,
        "id-off must release exactly its note"
    );
    // …and an id-less off (−1) matches the key and releases the rest.
    rack.dispatch(off(-1, 60));
    run(&mut rack, SR as usize);
    assert_eq!(
        rack.active_voices(),
        0,
        "key-off must release the remainder"
    );
}

#[test]
fn released_voices_ring_then_park() {
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    rack.prepare(SR);
    rack.dispatch(on(1, 64, 1.0));
    run(&mut rack, 4_800);
    rack.dispatch(off(1, 64));
    let tail = run(&mut rack, 480); // 10 ms into an 80 ms release
    assert!(rack.active_voices() == 1, "release must keep sounding");
    assert!(tail.iter().any(|s| s.abs() > 1e-3), "silent release tail");
    run(&mut rack, SR as usize);
    assert_eq!(rack.active_voices(), 0, "release must eventually park");
}

#[test]
fn choke_is_immediate() {
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    rack.prepare(SR);
    rack.dispatch(on(1, 64, 1.0));
    run(&mut rack, 4_800);
    rack.dispatch(VoiceEvent::Choke { id: 1, key: 64 });
    let after = run(&mut rack, 64);
    assert!(
        after.iter().all(|s| *s == 0.0),
        "choke must silence immediately"
    );
    assert_eq!(rack.active_voices(), 0);
}

#[test]
fn full_pool_steals_by_policy_and_never_drops() {
    // Oldest: fill 4 slots, add a 5th — the FIRST note's pitch
    // disappears from the sustained spectrum, the 5th's appears.
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Oldest, |_| SineVoice::new());
    rack.prepare(SR);
    let keys = [48i16, 55, 60, 64, 72];
    for (i, &k) in keys.iter().enumerate() {
        rack.dispatch(on(i as i32, k, 0.8));
        run(&mut rack, 480); // stagger so ages differ
    }
    assert_eq!(rack.active_voices(), 4, "pool bounded at MAX");
    let sustained = run(&mut rack, 24_000);
    let stolen = mag_at(&sustained, f64::from(hz_of_key(48)));
    let newest = mag_at(&sustained, f64::from(hz_of_key(72)));
    assert!(
        newest > 20.0 * stolen,
        "oldest must be the victim (stolen {stolen:.2e} vs newest {newest:.2e})"
    );

    // Quietest: a loud chord + one quiet note; the next note evicts
    // the quiet one.
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Quietest, |_| SineVoice::new());
    rack.prepare(SR);
    for (i, &k) in [48i16, 55, 60].iter().enumerate() {
        rack.dispatch(on(i as i32, k, 0.9));
    }
    rack.dispatch(on(3, 64, 0.05)); // the quiet victim-to-be
    run(&mut rack, 4_800);
    rack.dispatch(on(4, 72, 0.9));
    let sustained = run(&mut rack, 24_000);
    let quiet = mag_at(&sustained, f64::from(hz_of_key(64)));
    let loud = mag_at(&sustained, f64::from(hz_of_key(48)));
    assert!(
        loud > 10.0 * quiet,
        "quietest must be the victim (quiet {quiet:.2e} loud {loud:.2e})"
    );
}

#[test]
fn mini_fm_per_voice_envelopes_shape_the_spectrum() {
    // The second consumer: the modulator's OWN envelope makes the
    // attack bright and the tail pure — sideband energy (relative to
    // the carrier) must COLLAPSE between the first 50 ms and the
    // sustained tail. Per-voice control instancing, measured.
    let mut rack: VoiceRack<MiniFmVoice, 4> =
        VoiceRack::new(Steal::Quietest, |_| MiniFmVoice::new());
    rack.prepare(SR);
    rack.dispatch(on(1, 69, 1.0));
    let out = run(&mut rack, 48_000);
    let f0 = f64::from(hz_of_key(69));
    let sideband = f0 * 3.0; // carrier + ratio-2 modulator's first upper sideband
    let early = &out[..2_400];
    let late = &out[24_000..48_000];
    let ratio_early = mag_at(early, sideband) / mag_at(early, f0).max(1e-30);
    let ratio_late = mag_at(late, sideband) / mag_at(late, f0).max(1e-30);
    assert!(
        ratio_early > 5.0 * ratio_late,
        "modulator envelope must dim the sidebands: early {ratio_early:.3} vs late {ratio_late:.3}"
    );
}

#[test]
fn hz_of_key_is_equal_temperament_at_440() {
    assert_eq!(hz_of_key(69), 440.0);
    assert!((hz_of_key(81) - 880.0).abs() < 1e-3, "octave doubles");
    assert!((hz_of_key(60) - 261.6256).abs() < 0.01, "middle C");
}

/// STEALING IS CLICK-FREE: the fifth note into a four-voice pool
/// must not step the waveform. Before the rack owned the fade, the
/// immediate phase-reset retrigger measured a 6× discontinuity over
/// the steady-state worst sample-to-sample delta; the invariant holds the
/// steal window under 2×.
#[test]
fn stealing_is_click_free() {
    let mut rack: VoiceRack<SineVoice, 4> = VoiceRack::new(Steal::Oldest, |_| SineVoice::new());
    rack.prepare(SR);
    for (i, &k) in [48i16, 55, 60, 64].iter().enumerate() {
        rack.dispatch(on(i as i32, k, 0.9));
        run(&mut rack, 480);
    }
    run(&mut rack, 9_600); // settled chord
    let worst_delta = |w: &[f32]| {
        w.windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .fold(0.0f32, f32::max)
    };
    let steady = worst_delta(&run(&mut rack, 4_800));
    rack.dispatch(on(9, 72, 0.9)); // the steal
    let steal_window = worst_delta(&run(&mut rack, 480));
    assert!(
        steal_window < 2.0 * steady,
        "steal steps the waveform: {steal_window:.4} vs steady {steady:.4} ({:.1}×)",
        steal_window / steady
    );
}
