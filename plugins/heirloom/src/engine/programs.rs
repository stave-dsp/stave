//! `Programs` — the wet machine, running on the core CLOCK (32 kHz
//! nominal; clock.rs owns the bridge to the host rate). Two machines
//! in series, each with its own feedback path through `AnalogPath`:
//! FEEDBACK is the first path's gain, REGEN the second's. The five
//! programs are five wirings of the same cells:
//!
//! ```text
//! I    a = rev-delay(x)            ⟲ FEEDBACK;  X blends forward→reversed
//!      r = reverse(tank(a))        ⟲ REGEN
//! II   a = rev-delay(x)            ⟲ FEEDBACK
//!      t = tank(a)  ⟲ REGEN · +12(delay(t))         X = decay
//! III  as II with −12, heavy damping, driven tank   X = decay
//! IV   r = reverse(tank(x))  ⟲ FEEDBACK·(r+−12 r) + REGEN·(r++12 r)
//!      t = tank(r)                                  X = decay
//! V    a = shift↑ₛ(delay(x))       ⟲ FEEDBACK
//!      b = shift↓ₛ(delay(a))       ⟲ REGEN          X = interval order
//! ```
//!
//! Every cell is sized and timed in TICKS, so when the clock moves,
//! everything in the buffers changes pitch and every time scales —
//! the pedal's DIVE/RISE and program IV's stretch are the same act.

use stave_dsp::DelayLine;

use super::path::{soft, AnalogPath};
use super::reverse::ReverseLine;
use super::tank::Tank;
use super::transpose::Transposer;

/// The nominal core tick rate — the pedal's wet path runs at 32 kHz.
pub(super) const CORE_HZ: f32 = 32_000.0;
/// TIME's ceiling in ms (params.rs) — every line is sized for it.
pub(super) const MAX_TIME_MS: f32 = 1_200.0;
/// Program IV's reverse-reverb segment (it stretches with the clock).
const IV_SEGMENT_MS: f32 = 700.0;
/// Program I's reverse-reverb segment follows TIME, inside this floor.
const I_SEGMENT_FLOOR_MS: f32 = 250.0;

/// Program V's interval orders — X walks the six permutations of
/// fourth, fifth, octave (semitones).
const ORDERS: [[f32; 3]; 6] =
    [[5.0, 7.0, 12.0], [5.0, 12.0, 7.0], [7.0, 5.0, 12.0], [7.0, 12.0, 5.0], [12.0, 5.0, 7.0], [12.0, 7.0, 5.0]];

/// The per-tick knob values (already glided at the host rate).
#[derive(Copy, Clone)]
pub(super) struct Knobs {
    pub feedback: f32,
    pub regen: f32,
    pub x: f32,
    /// TIME in ticks.
    pub len: f32,
}

pub(super) struct Programs {
    mode: usize,
    /// Machine one's line: the reverse delay (I–III), the first pitch
    /// delay (V).
    a: [ReverseLine; 2],
    /// The reverse-reverb reverser (I, IV).
    rr: [ReverseLine; 2],
    /// Machine two's line: the octave path's delay (II, III), the
    /// second pitch delay (V).
    b: [DelayLine; 2],
    tank_a: Tank,
    tank_b: Tank,
    up: Transposer,
    down: Transposer,
    seq_a: Transposer,
    seq_b: Transposer,
    path1: AnalogPath,
    path2: AnalogPath,
    /// Program V's sequencer: ticks into the current step, and the step.
    step_ticks: f32,
    step: usize,
}

impl Programs {
    pub(super) fn new() -> Self {
        Self {
            mode: 0,
            a: [ReverseLine::new(), ReverseLine::new()],
            rr: [ReverseLine::new(), ReverseLine::new()],
            b: [DelayLine::new(), DelayLine::new()],
            tank_a: Tank::new(),
            tank_b: Tank::new(),
            up: Transposer::new(1_400.0),
            down: Transposer::new(2_400.0),
            seq_a: Transposer::new(1_800.0),
            seq_b: Transposer::new(1_800.0),
            path1: AnalogPath::new(),
            path2: AnalogPath::new(),
            step_ticks: 0.0,
            step: 0,
        }
    }

    pub(super) fn prepare(&mut self) {
        let max = ms_to_ticks(MAX_TIME_MS) as usize + 64;
        for (c, l) in self.a.iter_mut().enumerate() {
            l.prepare(max, c as f32 * 0.25);
        }
        for (c, l) in self.rr.iter_mut().enumerate() {
            l.prepare(max, c as f32 * 0.25);
        }
        for l in &mut self.b {
            l.prepare(max);
        }
        self.tank_a.prepare(CORE_HZ);
        self.tank_b.prepare(CORE_HZ);
        for t in [&mut self.up, &mut self.down, &mut self.seq_a, &mut self.seq_b] {
            t.prepare();
        }
        self.reset();
    }

    /// Silence every buffer — called on a program change, behind the
    /// wet fade, so the new wiring never replays the old one's state.
    pub(super) fn reset(&mut self) {
        for (c, l) in self.a.iter_mut().enumerate() {
            l.reset(c as f32 * 0.25);
        }
        for (c, l) in self.rr.iter_mut().enumerate() {
            l.reset(c as f32 * 0.25);
        }
        for l in &mut self.b {
            l.reset();
        }
        self.tank_a.reset();
        self.tank_b.reset();
        for t in [&mut self.up, &mut self.down, &mut self.seq_a, &mut self.seq_b] {
            t.reset();
        }
        self.path1.reset();
        self.path2.reset();
        self.step_ticks = 0.0;
        self.step = 0;
    }

    /// Block rate: the program and its voicing (decay from X).
    pub(super) fn voice(&mut self, mode: usize, x: f32) {
        self.mode = mode;
        let fs = CORE_HZ;
        let decay = 0.6 * (25.0f32 / 0.6).powf(x);
        // The first path: delay repeats through a warm analog loop.
        self.path1.voice(fs, 60.0, 5_500.0, 1.3);
        match mode {
            0 => {
                self.tank_a.voice(fs, 2.2, 6_500.0);
                self.path2.voice(fs, 80.0, 7_000.0, 1.2);
            }
            1 => {
                self.tank_b.voice(fs, decay, 9_000.0);
                self.path2.voice(fs, 120.0, 8_000.0, 1.2);
            }
            2 => {
                // The dark program: the repeats themselves are filtered
                // and driven, not just the tank.
                self.path1.voice(fs, 40.0, 2_200.0, 2.2);
                self.tank_b.voice(fs, decay, 2_000.0);
                self.path2.voice(fs, 40.0, 1_500.0, 2.5);
            }
            3 => {
                self.tank_a.voice(fs, 2.6, 7_000.0);
                self.tank_b.voice(fs, decay, 7_500.0);
                self.path1.voice(fs, 40.0, 4_500.0, 1.3);
                self.path2.voice(fs, 120.0, 7_000.0, 1.3);
            }
            _ => {
                self.path2.voice(fs, 60.0, 5_500.0, 1.3);
            }
        }
    }

    /// One core tick: stereo in, stereo wet out.
    #[inline]
    pub(super) fn tick(&mut self, x: [f32; 2], k: Knobs) -> [f32; 2] {
        match self.mode {
            0 => self.reverse_into_reverse(x, k),
            1 => self.reverse_into_octave(x, k, 2.0),
            2 => self.reverse_into_octave(x, k, 0.5),
            3 => self.reverse_reverb_into_reverb(x, k),
            _ => self.pitch_into_pitch(x, k),
        }
    }

    /// Machine one of I–III: the reverse delay. `blend` 0 = forward
    /// tap, 1 = fully reversed (equal power between).
    #[inline]
    fn reverse_delay(&mut self, x: [f32; 2], len: f32, feedback: f32, blend: f32) -> [f32; 2] {
        let (gf, gr) = equal_power(blend);
        let mut a = [0.0; 2];
        for c in 0..2 {
            let rev = self.a[c].reverse(len);
            a[c] = if gf > 0.0 { gf * self.a[c].forward(len) + gr * rev } else { rev };
        }
        let back = self.path1.process(a);
        for c in 0..2 {
            self.a[c].write(x[c] + feedback * back[c]);
        }
        a
    }

    fn reverse_into_reverse(&mut self, x: [f32; 2], k: Knobs) -> [f32; 2] {
        let a = self.reverse_delay(x, k.len, k.feedback, k.x);
        let seg = k.len.max(ms_to_ticks(I_SEGMENT_FLOOR_MS));
        let r = [self.rr[0].reverse(seg), self.rr[1].reverse(seg)];
        let back = self.path2.process(r);
        let t = self.tank_a.process([a[0] + k.regen * back[0], a[1] + k.regen * back[1]]);
        self.rr[0].write(t[0]);
        self.rr[1].write(t[1]);
        [0.5 * a[0] + 1.0 * r[0], 0.5 * a[1] + 1.0 * r[1]]
    }

    fn reverse_into_octave(&mut self, x: [f32; 2], k: Knobs, ratio: f32) -> [f32; 2] {
        let a = self.reverse_delay(x, k.len, k.feedback, 1.0);
        let dark = ratio < 1.0;
        let delayed = [self.b[0].read(k.len), self.b[1].read(k.len)];
        let shifted = if dark { self.down.process(delayed, ratio) } else { self.up.process(delayed, ratio) };
        let back = self.path2.process(shifted);
        let mut send = [a[0] + k.regen * back[0], a[1] + k.regen * back[1]];
        if dark {
            // The dark program's harmonic distortion: the tank is fed hot.
            send = [soft(send[0] * 2.0) * 0.5, soft(send[1] * 2.0) * 0.5];
        }
        let t = self.tank_b.process(send);
        for c in 0..2 {
            self.b[c].write(t[c]);
            self.b[c].advance();
        }
        // Light lets the reverse delay sit forward; dark buries it in the murk.
        let (ga, gt) = if dark { (0.25, 0.95) } else { (0.45, 0.8) };
        [ga * a[0] + gt * t[0], ga * a[1] + gt * t[1]]
    }

    fn reverse_reverb_into_reverb(&mut self, x: [f32; 2], k: Knobs) -> [f32; 2] {
        let seg = ms_to_ticks(IV_SEGMENT_MS);
        let r = [self.rr[0].reverse(seg), self.rr[1].reverse(seg)];
        let lo = self.down.process(r, 0.5);
        let hi = self.up.process(r, 2.0);
        let p1 = self.path1.process([0.4 * r[0] + 0.8 * lo[0], 0.4 * r[1] + 0.8 * lo[1]]);
        let p2 = self.path2.process([0.4 * r[0] + 0.8 * hi[0], 0.4 * r[1] + 0.8 * hi[1]]);
        let t = self.tank_a.process([
            x[0] + k.feedback * p1[0] + k.regen * p2[0],
            x[1] + k.feedback * p1[1] + k.regen * p2[1],
        ]);
        self.rr[0].write(t[0]);
        self.rr[1].write(t[1]);
        let f = self.tank_b.process(r);
        [0.6 * r[0] + 0.9 * f[0], 0.6 * r[1] + 0.9 * f[1]]
    }

    fn pitch_into_pitch(&mut self, x: [f32; 2], k: Knobs) -> [f32; 2] {
        // The sequencer: one step per repeat.
        self.step_ticks += 1.0;
        if self.step_ticks >= k.len {
            self.step_ticks -= k.len;
            self.step = (self.step + 1) % 3;
        }
        let order = ORDERS[((k.x * 6.0) as usize).min(5)];
        let up = semis(order[self.step]);
        let down = semis(-order[2 - self.step]);

        let da = [self.a[0].forward(k.len), self.a[1].forward(k.len)];
        let sa = self.seq_a.process(da, up);
        let back_a = self.path1.process(sa);
        for c in 0..2 {
            self.a[c].write(x[c] + k.feedback * back_a[c]);
        }
        let db = [self.b[0].read(k.len), self.b[1].read(k.len)];
        let sb = self.seq_b.process(db, down);
        let back_b = self.path2.process(sb);
        for c in 0..2 {
            self.b[c].write(sa[c] + k.regen * back_b[c]);
            self.b[c].advance();
        }
        // The duel: machine one leans left, machine two right.
        [0.9 * sa[0] + 0.45 * sb[0], 0.45 * sa[1] + 0.9 * sb[1]]
    }
}

#[inline]
pub(super) fn ms_to_ticks(ms: f32) -> f32 {
    ms * 0.001 * CORE_HZ
}

#[inline]
fn semis(s: f32) -> f32 {
    (s / 12.0).exp2()
}

/// (forward, reversed) gains for a 0..1 blend, equal power.
#[inline]
fn equal_power(blend: f32) -> (f32, f32) {
    let t = blend * core::f32::consts::FRAC_PI_2;
    if blend >= 1.0 { (0.0, 1.0) } else { (t.cos(), t.sin()) }
}
