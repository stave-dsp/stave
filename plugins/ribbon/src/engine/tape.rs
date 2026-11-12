//! `Tape` — the loop, modelled in TAPE coordinates, not time. The
//! ribbon is a ring of cells (one cell = one sample of tape at nominal
//! speed; 4 s of loop); the heads are fixed points it is dragged past:
//!
//! ```text
//!   tape travels →
//!   … ERASE ─ RECORD ─────── PLAY 1 ─────── PLAY 2 ─────── PLAY 3 …
//!               0           90 ms          180 ms         270 ms
//! ```
//!
//! Each frame the motor advances the ribbon by `v` cells. The ERASE
//! head scales every cell that crosses it by (1 − erase) — and, on a
//! worn tape, smears it into its neighbour (each generation duller) and
//! saturates it (tape cannot hold more than it can hold). The RECORD
//! head lays the frame's signal over the cells it sweeps, weighted by
//! overlap, so a cell holds the AVERAGE of what passed over it at any
//! speed. The PLAY heads read the ribbon where it is under them.
//!
//! Consequences, for free and exactly: speed IS pitch (what was written
//! at one speed plays at another); a stopped tape holds still; an
//! unwiped loop comes back around one loop later, under the new pass.
//!
//! The MOTOR: RATE sets the target speed and the capstan follows with
//! inertia (an exponential glide); the BRAKE decelerates it linearly
//! to a dead stop, and release spins it back up. WEAR adds wow (a slow
//! off-centre wobble) and flutter (a fast scrape) to the speed.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use core::f32::consts::TAU;

use stave_dsp::Xorshift32;

/// The loop's length in seconds of tape at nominal speed.
pub(super) const LOOP_S: f32 = 4.0;
/// Playback heads' distances from the record head, in seconds of tape.
pub(super) const HEAD_S: [f32; 3] = [0.090, 0.180, 0.270];
/// The erase head sits this far before the record head.
const ERASE_S: f32 = 0.020;
/// The capstan's inertia (seconds to ~63% of a speed change).
const INERTIA_S: f32 = 0.25;
/// Brake: seconds from full speed to a stop; and to spin back up.
const BRAKE_S: f32 = 1.3;
const START_S: f32 = 0.6;
/// WEAR 1: wow and flutter depths (fraction of speed) and rates.
const WOW: f32 = 0.006;
const WOW_HZ: f32 = 0.55;
const FLUTTER: f32 = 0.0025;
const FLUTTER_HZ: f32 = 7.3;
/// What a cell can hold: the tape's soft ceiling, applied once a pass.
const TAPE_CEILING: f32 = 1.2;

#[inline]
fn soft(x: f32) -> f32 {
    let x = (x / TAPE_CEILING).clamp(-3.0, 3.0);
    TAPE_CEILING * x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

pub(super) struct Tape {
    cells: [Vec<f32>; 2],
    len: usize,
    /// The record head's position in tape coordinates (cells, wrapped
    /// to the loop).
    pos: f64,
    /// Head offsets in cells.
    heads: [f64; 3],
    erase_at: f64,
    /// The motor: current speed, and the motion it adds.
    speed: f32,
    wow_phase: f32,
    flutter_phase: f32,
    jitter: f32,
    rng: Xorshift32,
    inertia: f32,
    brake_step: f32,
    start_step: f32,
    host_hz: f32,
}

/// Per-block settings.
#[derive(Copy, Clone)]
pub(super) struct Transport {
    pub(super) rate: f32,
    pub(super) brake: bool,
    pub(super) erase: f32,
    pub(super) wear: f32,
}

impl Tape {
    pub(super) fn new() -> Self {
        Self {
            cells: [Vec::new(), Vec::new()],
            len: 1,
            pos: 0.0,
            heads: [0.0; 3],
            erase_at: 0.0,
            speed: 1.0,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            jitter: 0.0,
            rng: Xorshift32::new(0x7A9E_0001),
            inertia: 0.0,
            brake_step: 0.0,
            start_step: 0.0,
            host_hz: 48_000.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32, rate: f32) {
        self.host_hz = sr;
        self.len = (LOOP_S * sr) as usize;
        self.cells = [vec![0.0; self.len], vec![0.0; self.len]];
        self.pos = 0.0;
        self.heads = HEAD_S.map(|s| f64::from(s * sr));
        self.erase_at = f64::from(ERASE_S * sr);
        self.speed = rate;
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.jitter = 0.0;
        self.rng = Xorshift32::new(0x7A9E_0001);
        self.inertia = 1.0 - (-1.0 / (INERTIA_S * sr)).exp();
        self.brake_step = 1.0 / (BRAKE_S * sr);
        self.start_step = 1.0 / (START_S * sr);
    }

    /// The motor, one frame: the speed the ribbon moves at now.
    #[inline]
    fn motor(&mut self, t: Transport) -> f32 {
        if t.brake {
            self.speed = (self.speed - self.brake_step).max(0.0);
        } else if self.speed < t.rate * 0.5 {
            // Spinning up from (near) a stop: the motor's torque, not
            // the capstan's glide, sets the pace.
            self.speed = (self.speed + self.start_step).min(t.rate);
        } else {
            self.speed += (t.rate - self.speed) * self.inertia;
        }
        if t.wear <= 0.0 || self.speed <= 0.0 {
            return self.speed;
        }
        self.wow_phase = (self.wow_phase + WOW_HZ * self.speed / self.host_hz).fract();
        self.flutter_phase = (self.flutter_phase + FLUTTER_HZ / self.host_hz).fract();
        self.jitter += (self.rng.next_bipolar() - self.jitter) * (8.0 / self.host_hz);
        let wobble = WOW * (TAU * self.wow_phase).sin() + FLUTTER * (TAU * self.flutter_phase).sin() * (1.0 + 2.0 * self.jitter);
        (self.speed * (1.0 + t.wear * wobble)).max(0.0)
    }

    #[inline]
    fn wrap(&self, i: i64) -> usize {
        i.rem_euclid(self.len as i64) as usize
    }

    /// Cubic read of track `ch` at tape coordinate `q`.
    #[inline]
    fn read(&self, ch: usize, q: f64) -> f32 {
        let i = q.floor() as i64;
        let f = (q - q.floor()) as f32;
        let c = &self.cells[ch];
        let (xm1, x0, x1, x2) = (c[self.wrap(i - 1)], c[self.wrap(i)], c[self.wrap(i + 1)], c[self.wrap(i + 2)]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    /// One frame: advance the ribbon, erase what crosses the erase
    /// head, record `rec` (per track), read the three heads. Returns
    /// each head's [L, R].
    #[inline]
    pub(super) fn tick(&mut self, rec: [f32; 2], t: Transport) -> [[f32; 2]; 3] {
        let v = f64::from(self.motor(t));
        let (a, b) = (self.pos, self.pos + v);

        // ERASE: every cell whose start crosses the erase head this
        // frame is wiped by `erase`; what survives is a generation
        // older (smeared, saturated).
        if v > 0.0 {
            let keep = 1.0 - t.erase;
            let smear = 0.45 * t.wear;
            let (e0, e1) = ((a + self.erase_at).floor() as i64, (b + self.erase_at).floor() as i64);
            for i in (e0 + 1)..=e1 {
                let (k, prev) = (self.wrap(i), self.wrap(i - 1));
                for c in &mut self.cells {
                    c[k] = if keep <= 0.0 { 0.0 } else { soft(keep * (c[k] + smear * (c[prev] - c[k]))) };
                }
            }
        }

        // RECORD: lay the signal over the swept cells, by overlap.
        if v > 0.0 {
            let mut k = a.floor();
            while k < b {
                let overlap = (b.min(k + 1.0) - a.max(k)) as f32;
                if overlap > 0.0 {
                    let idx = self.wrap(k as i64);
                    for (c, x) in self.cells.iter_mut().zip(rec) {
                        c[idx] += x * overlap;
                    }
                }
                k += 1.0;
            }
        }
        self.pos = b % self.len as f64;

        // PLAY: each head reads where the ribbon is under it.
        let mut out = [[0.0f32; 2]; 3];
        for (h, o) in self.heads.iter().zip(out.iter_mut()) {
            let q = self.pos - h;
            *o = [self.read(0, q), self.read(1, q)];
        }
        out
    }

    /// Where the loop is (0..1 of a turn) — the face draws the ribbon.
    pub(super) fn loop_phase(&self) -> f64 {
        self.pos / self.len as f64
    }

    pub(super) fn speed(&self) -> f32 {
        self.speed
    }
}
