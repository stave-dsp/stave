//! Built-in program material: synthesised, deterministic, and musical
//! enough to judge tone and distortion without a corpus. Plucked strings
//! are Karplus–Strong (noise-burst excitation, pick-position comb,
//! two-point loop filter, allpass fine tuning); keys are additive with
//! per-partial decay; drums are the classic analog recipes. Every item
//! is the same samples at a given rate on every machine (a seeded
//! generator, no clocks), so a session log can name it by checksum.

use std::f64::consts::TAU;

use super::rng::Rng;

/// One built-in item: its name, what it is for, and how it is made.
pub struct Builtin {
    pub name: &'static str,
    pub about: &'static str,
    make: fn(f64) -> Vec<Vec<f32>>,
}

impl Builtin {
    /// The item at `sample_rate`: one or two channels, peak −6 dBFS.
    pub fn make(&self, sample_rate: f64) -> Vec<Vec<f32>> {
        let mut channels = (self.make)(sample_rate);
        normalise(&mut channels, ITEM_PEAK);
        channels
    }
}

/// Built-in items peak here, leaving the input trim to set the drive.
pub const ITEM_PEAK: f32 = 0.5;

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "guitar riff",
        about: "single-note pentatonic riff with slides of dynamics; picked string, DI level",
        make: guitar_riff,
    },
    Builtin {
        name: "power chords",
        about: "palm-muted eighths then ringing root-fifth-octave chords",
        make: power_chords,
    },
    Builtin {
        name: "open chords",
        about: "strummed six-string G C D Em, down and up strokes",
        make: open_chords,
    },
    Builtin {
        name: "keys",
        about: "sustained extended chords (additive electric piano): tone and intermodulation",
        make: keys,
    },
    Builtin {
        name: "bass line",
        about: "plucked bass, low E to A, with octave jumps",
        make: bass_line,
    },
    Builtin {
        name: "drum loop",
        about: "kick, snare and hats, two bars at 100 bpm, stereo hats",
        make: drum_loop,
    },
    Builtin {
        name: "band mix",
        about: "drums, bass and keys together: the mix-bus check",
        make: band_mix,
    },
    Builtin {
        name: "level steps",
        about: "220 Hz tone stepping −36 to 0 dBFS in 6 dB steps: where it breaks up",
        make: level_steps,
    },
    Builtin {
        name: "noise bursts",
        about: "white and pink bursts, 5 to 500 ms: attack, release, blocking",
        make: noise_bursts,
    },
    Builtin {
        name: "log sweep",
        about: "20 Hz to 20 kHz in 4 s: response and aliasing by ear",
        make: log_sweep,
    },
];

pub fn find(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(name))
}

const BPM: f64 = 100.0;

fn beat_seconds() -> f64 {
    60.0 / BPM
}

fn midi_hz(note: f64) -> f64 {
    440.0 * 2f64.powf((note - 69.0) / 12.0)
}

fn normalise(channels: &mut [Vec<f32>], peak: f32) {
    let max = channels
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |m, v| m.max(v.abs()));
    if max > 0.0 {
        let g = peak / max;
        channels.iter_mut().flatten().for_each(|v| *v *= g);
    }
}

fn add_at(out: &mut [f32], start: usize, note: &[f32], gain: f64) {
    for (o, v) in out.iter_mut().skip(start).zip(note) {
        *o += (f64::from(*v) * gain) as f32;
    }
}

/// How a string is plucked.
#[derive(Copy, Clone)]
struct Pluck {
    note: f64,
    velocity: f64,
    /// Seconds the note lasts before a short release (it may die sooner).
    hold: f64,
    /// T60 of the fundamental, seconds.
    decay: f64,
    /// 0 dark … 1 bright excitation.
    brightness: f64,
    /// Pick position as a fraction of the string (comb notch).
    position: f64,
    /// The loop's extra one-pole low-pass coefficient: 1 leaves the
    /// string bright, lower damps the highs faster (a palm mute, a
    /// round bass).
    tone: f64,
}

/// One Karplus–Strong note.
fn pluck(sample_rate: f64, p: Pluck, rng: &mut Rng) -> Vec<f32> {
    let hz = midi_hz(p.note);
    let period = sample_rate / hz;
    // The two-point loop filter (weight s on the older sample) delays
    // by s, the tone one-pole by (1 − a)/a at low frequencies; the
    // allpass supplies the fractional rest, kept in [0.1, 1.1).
    let s = 0.5;
    let a = p.tone.clamp(0.05, 1.0);
    let filters = s + (1.0 - a) / a;
    let mut delay = (period - filters).floor() as usize;
    let mut frac = period - filters - delay as f64;
    if frac < 0.1 && delay > 2 {
        delay -= 1;
        frac += 1.0;
    }
    let ap = (1.0 - frac) / (1.0 + frac);
    let loop_gain = 10f64.powf(-3.0 / (p.decay * hz));
    let len = ((p.hold + 0.02) * sample_rate) as usize;

    // Excitation: one period of noise, low-passed by brightness and
    // combed at the pick position.
    let coef = 0.15 + 0.8 * p.brightness;
    let mut lp = 0.0;
    let mut exc: Vec<f64> = (0..delay)
        .map(|_| {
            lp += coef * (rng.bipolar() - lp);
            lp
        })
        .collect();
    let notch = ((p.position * delay as f64).round() as usize).clamp(1, delay.max(2) - 1);
    for i in (notch..exc.len()).rev() {
        exc[i] -= exc[i - notch];
    }

    let mut line = vec![0.0f64; delay.max(1)];
    let (mut idx, mut prev, mut ap_x, mut ap_y, mut tone) = (0usize, 0.0, 0.0, 0.0, 0.0);
    let release = (0.01 * sample_rate) as usize;
    let hold = (p.hold * sample_rate) as usize;
    let mut dc_x = 0.0;
    let mut dc_y = 0.0;
    (0..len)
        .map(|n| {
            let d = line[idx];
            let avg = (1.0 - s) * d + s * prev;
            prev = d;
            tone += a * (avg - tone);
            let y = ap * tone + ap_x - ap * ap_y;
            ap_x = tone;
            ap_y = y;
            let mut v = loop_gain * y;
            if let Some(e) = exc.get(n) {
                v += e * p.velocity;
            }
            line[idx] = v;
            idx = (idx + 1) % line.len();
            // DC blocker, then the release ramp past the hold.
            dc_y = v - dc_x + 0.995 * dc_y;
            dc_x = v;
            let env = if n < hold {
                1.0
            } else {
                1.0 - ((n - hold) as f64 / release as f64).min(1.0)
            };
            (dc_y * env) as f32
        })
        .collect()
}

fn strings(sample_rate: f64, seconds: f64, notes: &[(f64, Pluck)], seed: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; (seconds * sample_rate) as usize];
    let mut rng = Rng::new(seed);
    for &(at, p) in notes {
        let note = pluck(sample_rate, p, &mut rng);
        add_at(&mut out, (at * sample_rate) as usize, &note, 1.0);
    }
    out
}

fn guitar_riff(sample_rate: f64) -> Vec<Vec<f32>> {
    // (beat, note, beats held, velocity)
    const RIFF: [(f64, f64, f64, f64); 12] = [
        (0.0, 40.0, 0.5, 1.0),
        (0.5, 43.0, 0.5, 0.7),
        (1.0, 45.0, 0.5, 0.8),
        (1.5, 47.0, 0.25, 0.6),
        (1.75, 45.0, 0.25, 0.6),
        (2.0, 50.0, 1.0, 0.95),
        (3.0, 52.0, 0.5, 0.75),
        (3.5, 50.0, 0.5, 0.6),
        (4.0, 47.0, 0.75, 0.9),
        (4.75, 45.0, 0.25, 0.5),
        (5.0, 43.0, 1.0, 0.8),
        (6.0, 40.0, 2.0, 1.0),
    ];
    let b = beat_seconds();
    let mut notes = Vec::new();
    for rep in 0..2 {
        let offset = f64::from(rep) * 8.0;
        for &(beat, note, held, vel) in &RIFF {
            // The second time round an octave up on the last phrase.
            let note = if rep == 1 && beat >= 4.0 { note + 12.0 } else { note };
            notes.push((
                (offset + beat) * b,
                Pluck {
                    note,
                    velocity: vel,
                    hold: held * b,
                    decay: 2.5,
                    brightness: 0.55 + 0.4 * vel,
                    position: 0.13,
                    tone: 0.75,
                },
            ));
        }
    }
    vec![strings(sample_rate, 16.0 * b + 0.6, &notes, 0x5157)]
}

fn chord(at: f64, notes: &[f64], spread: f64, down: bool, base: Pluck) -> Vec<(f64, Pluck)> {
    let order: Vec<f64> = if down {
        notes.to_vec()
    } else {
        notes.iter().rev().copied().collect()
    };
    order
        .into_iter()
        .enumerate()
        .map(|(i, note)| {
            (
                at + i as f64 * spread,
                Pluck {
                    note,
                    velocity: base.velocity * (1.0 - 0.06 * i as f64),
                    ..base
                },
            )
        })
        .collect()
}

fn power_chords(sample_rate: f64) -> Vec<Vec<f32>> {
    let b = beat_seconds();
    let muted = Pluck {
        note: 0.0,
        velocity: 0.9,
        hold: 0.45 * b,
        decay: 0.18,
        brightness: 0.3,
        position: 0.08,
        tone: 0.3,
    };
    let open = Pluck {
        hold: 1.9 * b,
        decay: 3.0,
        brightness: 0.85,
        position: 0.12,
        tone: 0.8,
        ..muted
    };
    let mut notes = Vec::new();
    // Two bars of palm-muted E5 eighths, accents on the beat.
    for i in 0..16 {
        let accent = if i % 2 == 0 { 1.0 } else { 0.75 };
        let base = Pluck {
            velocity: 0.9 * accent,
            ..muted
        };
        notes.extend(chord(f64::from(i) * 0.5 * b, &[40.0, 47.0, 52.0], 0.004, true, base));
    }
    // Then ringing G5, A5, C5, D5.
    for (i, root) in [43.0, 45.0, 48.0, 50.0].into_iter().enumerate() {
        notes.extend(chord(
            (8.0 + 2.0 * i as f64) * b,
            &[root, root + 7.0, root + 12.0],
            0.008,
            true,
            open,
        ));
    }
    vec![strings(sample_rate, 16.0 * b + 1.0, &notes, 0x9C)]
}

fn open_chords(sample_rate: f64) -> Vec<Vec<f32>> {
    let b = beat_seconds();
    const SHAPES: [&[f64]; 4] = [
        &[43.0, 47.0, 50.0, 55.0, 59.0, 67.0],
        &[48.0, 52.0, 55.0, 60.0, 64.0],
        &[50.0, 57.0, 62.0, 66.0],
        &[40.0, 47.0, 52.0, 55.0, 59.0, 64.0],
    ];
    // Down, down-up, up-down-up per bar (in beats; true = down stroke).
    const STRUM: [(f64, bool, f64); 6] = [
        (0.0, true, 1.0),
        (1.0, true, 0.8),
        (1.5, false, 0.55),
        (2.5, false, 0.6),
        (3.0, true, 0.85),
        (3.5, false, 0.55),
    ];
    let base = Pluck {
        note: 0.0,
        velocity: 0.8,
        hold: 0.0,
        decay: 3.5,
        brightness: 0.7,
        position: 0.18,
        tone: 0.7,
    };
    let mut notes = Vec::new();
    for (bar, shape) in SHAPES.iter().enumerate() {
        for (k, &(beat, down, vel)) in STRUM.iter().enumerate() {
            let next = STRUM.get(k + 1).map_or(4.0, |s| s.0);
            let p = Pluck {
                velocity: 0.8 * vel,
                hold: (next - beat) * b,
                brightness: if down { 0.7 } else { 0.85 },
                ..base
            };
            let at = (bar as f64 * 4.0 + beat) * b;
            notes.extend(chord(at, shape, 0.012, down, p));
        }
    }
    vec![strings(sample_rate, 16.0 * b + 1.5, &notes, 0x0C)]
}

fn keys_into(out: &mut [f32], sample_rate: f64, at: f64, notes: &[f64], hold: f64, gain: f64) {
    let attack = 0.004 * sample_rate;
    let release = 0.25 * sample_rate;
    let hold_n = hold * sample_rate;
    let len = (hold_n + release) as usize;
    let start = (at * sample_rate) as usize;
    for &note in notes {
        let hz = midi_hz(note);
        for k in 1..=8u32 {
            let kf = f64::from(k);
            // Slightly stretched partials, darker and faster-decaying up high.
            let f = hz * kf * (1.0 + 0.0004 * kf * kf);
            if f > 0.45 * sample_rate {
                break;
            }
            let amp = gain / kf.powf(1.4);
            let tau = 2.2 / kf.sqrt();
            for n in 0..len {
                let Some(o) = out.get_mut(start + n) else { break };
                let t = n as f64 / sample_rate;
                let nf = n as f64;
                let env = (nf / attack).min(1.0)
                    * (-t / tau).exp()
                    * if nf < hold_n {
                        1.0
                    } else {
                        (1.0 - (nf - hold_n) / release).max(0.0)
                    };
                *o += (amp * env * (TAU * f * t).sin()) as f32;
            }
        }
    }
}

fn keys(sample_rate: f64) -> Vec<Vec<f32>> {
    let b = beat_seconds();
    const CHORDS: [&[f64]; 4] = [
        &[48.0, 55.0, 59.0, 64.0, 67.0],
        &[45.0, 52.0, 55.0, 60.0, 71.0],
        &[41.0, 48.0, 52.0, 57.0, 64.0],
        &[43.0, 50.0, 53.0, 59.0, 62.0],
    ];
    let mut out = vec![0.0f32; ((16.0 * b + 1.0) * sample_rate) as usize];
    for (i, c) in CHORDS.iter().enumerate() {
        keys_into(&mut out, sample_rate, i as f64 * 4.0 * b, c, 3.6 * b, 0.2);
    }
    vec![out]
}

fn bass_notes() -> Vec<(f64, f64, f64)> {
    // (beat, note, beats held)
    let bar = [
        (0.0, 28.0, 0.9),
        (1.0, 28.0, 0.4),
        (1.5, 40.0, 0.4),
        (2.0, 31.0, 0.9),
        (3.0, 33.0, 0.4),
        (3.5, 35.0, 0.4),
    ];
    (0..4)
        .flat_map(|rep| {
            bar.iter().map(move |&(beat, note, held)| {
                let lift = if rep == 2 { 5.0 } else { 0.0 };
                (f64::from(rep) * 4.0 + beat, note + lift, held)
            })
        })
        .collect()
}

fn bass_line(sample_rate: f64) -> Vec<Vec<f32>> {
    let b = beat_seconds();
    let notes: Vec<(f64, Pluck)> = bass_notes()
        .into_iter()
        .map(|(beat, note, held)| {
            (
                beat * b,
                Pluck {
                    note,
                    velocity: 1.0,
                    hold: held * b,
                    decay: 1.8,
                    brightness: 0.2,
                    position: 0.2,
                    tone: 0.22,
                },
            )
        })
        .collect();
    vec![strings(sample_rate, 16.0 * b + 0.5, &notes, 0xBA55)]
}

fn kick(sample_rate: f64, rng: &mut Rng) -> Vec<f32> {
    let len = (0.4 * sample_rate) as usize;
    let mut phase = 0.0;
    (0..len)
        .map(|n| {
            let t = n as f64 / sample_rate;
            let hz = 48.0 + 110.0 * (-t / 0.03).exp();
            phase += TAU * hz / sample_rate;
            let body = phase.sin() * (-t / 0.22).exp();
            let click = if t < 0.003 { 0.4 * rng.bipolar() * (1.0 - t / 0.003) } else { 0.0 };
            (body + click) as f32
        })
        .collect()
}

fn snare(sample_rate: f64, rng: &mut Rng) -> Vec<f32> {
    let len = (0.3 * sample_rate) as usize;
    let (mut prev, mut hp) = (0.0, 0.0);
    (0..len)
        .map(|n| {
            let t = n as f64 / sample_rate;
            let x = rng.bipolar();
            hp = 0.7 * (hp + x - prev);
            prev = x;
            let noise = hp * (-t / 0.12).exp();
            let tone = 0.6 * (TAU * 185.0 * t).sin() * (-t / 0.06).exp();
            (0.8 * noise + tone) as f32
        })
        .collect()
}

fn hat(sample_rate: f64, rng: &mut Rng, decay: f64) -> Vec<f32> {
    let len = ((decay * 5.0).min(0.6) * sample_rate) as usize;
    let (mut x1, mut x2) = (0.0, 0.0);
    (0..len)
        .map(|n| {
            let t = n as f64 / sample_rate;
            let x = rng.bipolar();
            // Second difference: a steep high-pass on white noise.
            let y = x - 2.0 * x1 + x2;
            x2 = x1;
            x1 = x;
            (0.25 * y * (-t / decay).exp()) as f32
        })
        .collect()
}

fn drums_into(left: &mut [f32], right: &mut [f32], sample_rate: f64, bars: usize) {
    let b = beat_seconds();
    let mut rng = Rng::new(0xD2);
    let k = kick(sample_rate, &mut rng);
    let s = snare(sample_rate, &mut rng);
    let closed = hat(sample_rate, &mut rng, 0.035);
    let open = hat(sample_rate, &mut rng, 0.22);
    let at = |beat: f64| (beat * b * sample_rate) as usize;
    for bar in 0..bars {
        let o = bar as f64 * 4.0;
        for beat in [0.0, 1.5, 2.0, 2.75] {
            if bar % 2 == 0 && beat == 2.75 {
                continue;
            }
            for ch in [&mut *left, &mut *right] {
                add_at(ch, at(o + beat), &k, 0.9);
            }
        }
        for beat in [1.0, 3.0] {
            for ch in [&mut *left, &mut *right] {
                add_at(ch, at(o + beat), &s, 0.75);
            }
        }
        for i in 0..8 {
            let beat = f64::from(i) * 0.5;
            let (sample, vel) = if i == 7 { (&open, 0.8) } else { (&closed, if i % 2 == 0 { 0.8 } else { 0.55 }) };
            // Hats sit right of centre.
            add_at(left, at(o + beat), sample, vel * 0.7);
            add_at(right, at(o + beat), sample, vel);
        }
    }
}

fn drum_loop(sample_rate: f64) -> Vec<Vec<f32>> {
    let len = ((8.0 * beat_seconds() + 0.5) * sample_rate) as usize;
    let (mut l, mut r) = (vec![0.0f32; len], vec![0.0f32; len]);
    drums_into(&mut l, &mut r, sample_rate, 2);
    // Two bars, twice: a loop long enough to settle into.
    let take = (8.0 * beat_seconds() * sample_rate) as usize;
    let double = |c: &[f32]| -> Vec<f32> {
        let mut out = vec![0.0f32; take + len];
        add_at(&mut out, 0, c, 1.0);
        add_at(&mut out, take, c, 1.0);
        out
    };
    vec![double(&l), double(&r)]
}

fn band_mix(sample_rate: f64) -> Vec<Vec<f32>> {
    let b = beat_seconds();
    let len = ((16.0 * b + 1.0) * sample_rate) as usize;
    let (mut l, mut r) = (vec![0.0f32; len], vec![0.0f32; len]);
    drums_into(&mut l, &mut r, sample_rate, 4);
    let mut bass = bass_line(sample_rate).remove(0);
    normalise(std::slice::from_mut(&mut bass), 0.6);
    let mut keys_ch = vec![0.0f32; len];
    // Keys stabs on the off-beats.
    for bar in 0..4 {
        for beat in [0.5, 2.5] {
            let chord: &[f64] = if bar % 2 == 0 { &[52.0, 55.0, 59.0, 62.0] } else { &[55.0, 59.0, 62.0, 64.0] };
            keys_into(&mut keys_ch, sample_rate, (f64::from(bar) * 4.0 + beat) * b, chord, 0.4 * b, 0.12);
        }
    }
    add_at(&mut l, 0, &bass, 1.0);
    add_at(&mut r, 0, &bass, 1.0);
    add_at(&mut l, 0, &keys_ch, 1.0);
    add_at(&mut r, 0, &keys_ch, 0.7);
    vec![l, r]
}

fn level_steps(sample_rate: f64) -> Vec<Vec<f32>> {
    let step = 0.6;
    let ramp = 0.005 * sample_rate;
    let per = (step * sample_rate) as usize;
    let mut out = Vec::with_capacity(per * 8);
    for (i, db) in (0..7u32).map(|i| (i as usize, -36.0 + 6.0 * f64::from(i))) {
        let amp = 10f64.powf(db / 20.0);
        let on = (0.5 * sample_rate) as usize;
        for n in 0..per {
            let nf = n as f64;
            let env = if n < on {
                (nf / ramp).min(1.0) * ((on - n) as f64 / ramp).min(1.0)
            } else {
                0.0
            };
            let t = (i * per + n) as f64 / sample_rate;
            out.push((amp * env * (TAU * 220.0 * t).sin()) as f32);
        }
    }
    // Peak normalisation would undo the absolute levels: the loudest
    // step is 0 dBFS before the item peak scales it with the rest.
    vec![out]
}

fn noise_bursts(sample_rate: f64) -> Vec<Vec<f32>> {
    let mut rng = Rng::new(0xB0);
    let mut out = Vec::new();
    let gap = (0.3 * sample_rate) as usize;
    for pink in [false, true] {
        // Paul Kellet's economy pink filter.
        let (mut b0, mut b1, mut b2) = (0.0, 0.0, 0.0);
        for ms in [5.0, 20.0, 50.0, 200.0, 500.0] {
            let len = (ms / 1000.0 * sample_rate) as usize;
            let edge = (0.001 * sample_rate).max(1.0);
            for n in 0..len {
                let w = rng.bipolar();
                let v = if pink {
                    b0 = 0.99765 * b0 + w * 0.099_046;
                    b1 = 0.963 * b1 + w * 0.296_516_4;
                    b2 = 0.57 * b2 + w * 1.052_691_3;
                    0.25 * (b0 + b1 + b2 + w * 0.1848)
                } else {
                    w
                };
                let env = (n as f64 / edge).min(1.0) * ((len - n) as f64 / edge).min(1.0);
                out.push((0.5 * v * env) as f32);
            }
            out.extend(std::iter::repeat_n(0.0, gap));
        }
    }
    vec![out]
}

fn log_sweep(sample_rate: f64) -> Vec<Vec<f32>> {
    let seconds = 4.0;
    let (f1, f2) = (20.0f64, 20_000.0f64.min(0.45 * sample_rate));
    let len = (seconds * sample_rate) as usize;
    let k = (f2 / f1).ln();
    let fade = 0.01 * sample_rate;
    let mut out: Vec<f32> = (0..len)
        .map(|n| {
            let t = n as f64 / sample_rate;
            let phase = TAU * f1 * seconds / k * ((t / seconds * k).exp() - 1.0);
            let env = (n as f64 / fade).min(1.0) * ((len - n) as f64 / fade).min(1.0);
            (env * phase.sin()) as f32
        })
        .collect();
    out.extend(std::iter::repeat_n(0.0, (0.5 * sample_rate) as usize));
    vec![out]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_deterministic_and_peaks_at_the_item_level() {
        for b in BUILTINS {
            let a = b.make(48_000.0);
            assert!(!a.is_empty() && a.len() <= 2, "{}", b.name);
            let frames = a[0].len();
            assert!(a.iter().all(|c| c.len() == frames), "{}", b.name);
            let secs = frames as f64 / 48_000.0;
            // Long enough for integrated loudness, short enough to loop.
            assert!((2.0..=14.0).contains(&secs), "{}: {secs} s", b.name);
            let peak = a.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - ITEM_PEAK).abs() < 1e-6, "{}: {peak}", b.name);
            assert!(a.iter().flatten().all(|v| v.is_finite()));
            assert_eq!(a, b.make(48_000.0), "{} is not deterministic", b.name);
        }
    }

    #[test]
    fn a_plucked_string_sounds_at_its_pitch() {
        let sr = 48_000.0;
        let mut rng = Rng::new(1);
        let p = Pluck {
            note: 45.0,
            velocity: 1.0,
            hold: 1.0,
            decay: 3.0,
            brightness: 0.6,
            position: 0.13,
            tone: 0.5,
        };
        let x = pluck(sr, p, &mut rng);
        // The period by autocorrelation over the settled part.
        let seg = &x[4_800..14_400];
        let corr = |lag: usize| -> f64 {
            seg.iter()
                .zip(&seg[lag..])
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum()
        };
        let expect = sr / 110.0;
        let best = (300..600).max_by(|&a, &b| corr(a).total_cmp(&corr(b))).unwrap();
        assert!((best as f64 - expect).abs() <= 1.0, "{best} vs {expect}");
    }

    #[test]
    fn items_are_found_by_name() {
        assert!(find("Guitar Riff").is_some() && find("nope").is_none());
    }
}
