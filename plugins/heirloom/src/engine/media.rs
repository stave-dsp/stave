//! `Media` — the worn record the WET path plays off, at the host rate:
//! the dry and the reverse machine go in together, and MIX blends
//! what comes out against the clean dry. Two halves:
//!
//! * WOBBLE — the platter: a modulated read off a short line. WOBBLE
//!   sets the excursion, RPM the speed, WARP the shape (an even sine,
//!   a warped platter's lurch-and-sag, smoothed random drift). The
//!   excursion shrinks as RPM rises so the pitch swing stays musical
//!   (pitch deviation ∝ depth × rate). AGE adds the medium's own
//!   transport faults on top: vinyl's 33⅓ off-centre wow, tape's
//!   flutter, VHS's jitter.
//! * AGE — the wear: band-limiting, saturation, a noise floor
//!   (crackle + rumble / hiss / hiss + line whine) and failures
//!   (dropouts), each voiced per MEDIA.
//!
//! The stage's laws: WOBBLE 0 + AGE 0 is BIT-EXACT through (the read
//! lands at zero delay and the wear blend multiplies by exactly 0),
//! and AGE 0 adds no noise — silence in, silence out.

use core::f32::consts::TAU;

use stave_dsp::{DelayLine, Svf, Xorshift32};

/// The longest excursion either side, ms (WOBBLE 1, slowest RPM).
const WOBBLE_MS: f32 = 6.0;
/// Room for the excursion plus every AGE transport fault.
const LINE_MS: f32 = 40.0;
/// 33⅓ rpm — the off-centre record's once-per-revolution wow.
const VINYL_REV_HZ: f32 = 33.333 / 60.0;
/// The wear blend reaches full by this much AGE (then the knob keeps
/// deepening the wear itself).
const BLEND_FULL: f32 = 0.125;

/// One medium's wear at AGE = 1. Everything scales from clean at 0.
struct Voice {
    /// Low-pass corner at full age, Hz (20 kHz at age 0, log glide).
    lp_hz: f32,
    /// High-pass corner at full age, Hz (inaudible at age 0).
    hp_hz: f32,
    /// Saturation drive at full age (1 = clean).
    drive: f32,
    /// Broadband hiss at full age, linear.
    hiss: f32,
    /// Low rumble at full age, linear.
    rumble: f32,
    /// Crackle events per second at full age.
    crackle_hz: f32,
    /// Dropouts per second at full age.
    dropout_hz: f32,
    /// How far a dropout dips (0 = to silence).
    dropout_floor: f32,
    /// Off-centre wow depth at full age, ms.
    wow_ms: f32,
    /// Flutter depth at full age, ms, and its rate.
    flutter_ms: f32,
    flutter_hz: f32,
    /// The 15.734 kHz line whine (VHS only), linear.
    whine: f32,
}

const VOICES: [Voice; 3] = [
    // VINYL
    Voice {
        lp_hz: 6_500.0,
        hp_hz: 25.0,
        drive: 1.6,
        hiss: 0.004,
        rumble: 0.03,
        crackle_hz: 45.0,
        dropout_hz: 0.0,
        dropout_floor: 1.0,
        wow_ms: 1.4,
        flutter_ms: 0.03,
        flutter_hz: 11.0,
        whine: 0.0,
    },
    // TAPE
    Voice {
        lp_hz: 4_800.0,
        hp_hz: 45.0,
        drive: 4.0,
        hiss: 0.02,
        rumble: 0.004,
        crackle_hz: 0.0,
        dropout_hz: 0.6,
        dropout_floor: 0.25,
        wow_ms: 0.5,
        flutter_ms: 0.12,
        flutter_hz: 8.5,
        whine: 0.0,
    },
    // VHS
    Voice {
        lp_hz: 2_800.0,
        hp_hz: 90.0,
        drive: 2.6,
        hiss: 0.035,
        rumble: 0.002,
        crackle_hz: 0.0,
        dropout_hz: 1.4,
        dropout_floor: 0.05,
        wow_ms: 0.7,
        flutter_ms: 0.22,
        flutter_hz: 14.0,
        whine: 0.004,
    },
];

/// Per-block settings (AGE and MEDIA move the filter corners, which
/// are set once a block — the knob is smoothed, the steps are tiny).
#[derive(Copy, Clone)]
pub(super) struct Block {
    pub(super) media: usize,
    pub(super) warp: usize,
}

pub(super) struct Media {
    host_hz: f32,
    line: [DelayLine; 2],
    rng: Xorshift32,
    /// Wobble LFO phase 0..1; DRIFT's random segment endpoints.
    phase: f32,
    drift_from: f32,
    drift_to: f32,
    /// The record's rotation and the flutter's phase.
    rev: f32,
    flutter: f32,
    /// Slow random wander riding the flutter (no transport is a clock).
    jitter: f32,
    lp: Svf,
    hp: Svf,
    /// Shapes hiss (tilts it toward the top) and rumble (a low band).
    hiss_tilt: Svf,
    rumble_lp: Svf,
    /// A crackle in flight: amplitude, decay per sample, its pan.
    click: f32,
    click_decay: f32,
    click_pan: f32,
    /// Dropout envelope: current gain and where it is heading.
    drop_gain: f32,
    drop_target: f32,
    drop_left: u32,
    whine_phase: f32,
    /// Last frame's read delay (samples) and the summed playback
    /// speed since the last `take_speed` — the platter the eye sees.
    last_d: f32,
    speed_sum: f32,
    speed_n: u32,
}

#[inline]
fn soft(x: f32) -> f32 {
    // tanh-shaped, cheap: x / (1 + |x|) blended toward cubic near 0.
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

impl Media {
    pub(super) fn new() -> Self {
        Self {
            host_hz: 48_000.0,
            line: [DelayLine::new(), DelayLine::new()],
            rng: Xorshift32::new(0x51A6_E10F),
            phase: 0.0,
            drift_from: 0.0,
            drift_to: 0.0,
            rev: 0.0,
            flutter: 0.0,
            jitter: 0.0,
            lp: Svf::new(),
            hp: Svf::new(),
            hiss_tilt: Svf::new(),
            rumble_lp: Svf::new(),
            click: 0.0,
            click_decay: 0.0,
            click_pan: 0.5,
            drop_gain: 1.0,
            drop_target: 1.0,
            drop_left: 0,
            whine_phase: 0.0,
            last_d: 0.0,
            speed_sum: 0.0,
            speed_n: 0,
        }
    }

    pub(super) fn prepare(&mut self, host_hz: f32) {
        self.host_hz = host_hz;
        let n = (LINE_MS * 0.001 * host_hz) as usize + 8;
        for l in &mut self.line {
            l.prepare(n);
        }
        for f in [&mut self.lp, &mut self.hp, &mut self.hiss_tilt, &mut self.rumble_lp] {
            f.prepare(host_hz);
        }
        self.hiss_tilt.set(2_500.0, 0.0);
        self.rumble_lp.set(60.0, 0.4);
        self.rng = Xorshift32::new(0x51A6_E10F);
        self.phase = 0.0;
        self.drift_from = 0.0;
        self.drift_to = 0.0;
        self.rev = 0.0;
        self.flutter = 0.0;
        self.jitter = 0.0;
        self.click = 0.0;
        self.drop_gain = 1.0;
        self.drop_target = 1.0;
        self.drop_left = 0;
        self.whine_phase = 0.0;
        self.last_d = 0.0;
        self.speed_sum = 0.0;
        self.speed_n = 0;
    }

    /// The mean playback speed since the last call (1 = nominal; the
    /// read head moving away from the write head plays slow).
    pub(super) fn take_speed(&mut self) -> f32 {
        let s = if self.speed_n > 0 { self.speed_sum / self.speed_n as f32 } else { 1.0 };
        self.speed_sum = 0.0;
        self.speed_n = 0;
        s
    }

    /// Move the filter corners to this block's AGE.
    pub(super) fn begin_block(&mut self, age: f32, b: Block) {
        let v = &VOICES[b.media];
        let nyq = 0.45 * self.host_hz;
        let lp = 20_000.0f32.min(nyq) * (v.lp_hz / 20_000.0).powf(age);
        self.lp.set(lp, 0.15 * age);
        self.hp.set(10.0 * (v.hp_hz / 10.0).powf(age), 0.1);
    }

    /// The wobble LFO, −1…1, advanced one frame at `rpm` Hz.
    #[inline]
    fn wobble_lfo(&mut self, rpm: f32, warp: usize) -> f32 {
        self.phase += rpm / self.host_hz;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= 1.0;
        }
        let p = self.phase;
        match warp {
            0 => (TAU * p).sin(),
            1 => {
                // Phase distortion: the first 30% of the turn lurches
                // up, the rest sags back; a second harmonic bends it.
                let q = if p < 0.3 { p / 0.3 * 0.5 } else { 0.5 + (p - 0.3) / 0.7 * 0.5 };
                0.85 * (TAU * q).sin() + 0.15 * (2.0 * TAU * p + 1.0).sin()
            }
            _ => {
                if wrapped {
                    self.drift_from = self.drift_to;
                    self.drift_to = self.rng.next_bipolar();
                }
                let s = 0.5 - 0.5 * (core::f32::consts::PI * p).cos();
                self.drift_from + (self.drift_to - self.drift_from) * s
            }
        }
    }

    /// One frame. `wobble`, `rpm`, `age` are the smoothed knobs.
    #[inline]
    pub(super) fn process(&mut self, x: [f32; 2], wobble: f32, rpm: f32, age: f32, b: Block) -> [f32; 2] {
        let v = &VOICES[b.media];
        let ms = 0.001 * self.host_hz;

        // ---- WOBBLE: the platter + the medium's transport faults.
        let lfo = self.wobble_lfo(rpm, b.warp);
        let depth = WOBBLE_MS * wobble * wobble.sqrt() / (1.0 + 0.5 * rpm);
        self.rev = (self.rev + VINYL_REV_HZ / self.host_hz).fract();
        self.flutter = (self.flutter + v.flutter_hz * (1.0 + 0.3 * self.jitter) / self.host_hz).fract();
        self.jitter += (self.rng.next_bipolar() - self.jitter) * (3.0 / self.host_hz);
        let wow = v.wow_ms * age * (TAU * self.rev).sin();
        let flut = v.flutter_ms * age * (TAU * self.flutter).sin() * (1.0 + 2.0 * self.jitter);
        // Excursions ride on their own depth, so zero wobble + zero
        // age is a read at exactly zero delay.
        let d = ((depth * (1.0 + lfo) + v.wow_ms * age + wow + v.flutter_ms * age + flut) * ms).max(0.0);
        self.speed_sum += 1.0 - (d - self.last_d);
        self.speed_n += 1;
        self.last_d = d;
        let mut y = [0.0; 2];
        for (ch, l) in self.line.iter_mut().enumerate() {
            l.write(x[ch]);
            y[ch] = if d < 1.0 { l.read(d) } else { l.read_cubic(d) };
            l.advance();
        }

        // ---- AGE: the wear, blended in from exactly 0.
        let blend = (age / BLEND_FULL).min(1.0);
        if blend == 0.0 {
            return y;
        }
        let drive = 1.0 + (v.drive - 1.0) * age;
        let a2 = age * age;

        // Failures: a dropout picks a depth and length, glides there.
        if self.drop_left == 0 {
            if self.rng.next_unit() < v.dropout_hz * a2 / self.host_hz {
                let deep = self.rng.next_unit();
                self.drop_target = v.dropout_floor + (1.0 - v.dropout_floor) * (1.0 - deep) * 0.6;
                self.drop_left = ((0.03 + 0.25 * self.rng.next_unit()) * self.host_hz) as u32;
            } else {
                self.drop_target = 1.0;
            }
        } else {
            self.drop_left -= 1;
        }
        self.drop_gain += (self.drop_target - self.drop_gain) * (200.0 / self.host_hz);

        // The noise floor: hiss (tilted up), rumble (a low band),
        // crackle (decaying ticks), the line whine.
        let hiss_amt = v.hiss * a2;
        let white = [self.rng.next_bipolar(), self.rng.next_bipolar()];
        let hiss = [
            hiss_amt * self.hiss_tilt.process(white[0], 0).hp,
            hiss_amt * self.hiss_tilt.process(white[1], 1).hp,
        ];
        let rum = v.rumble * a2 * 6.0 * self.rumble_lp.process_lp(self.rng.next_bipolar(), 0);
        if self.click.abs() < 1e-4 && self.rng.next_unit() < v.crackle_hz * age / self.host_hz {
            let big = self.rng.next_unit();
            self.click = (0.02 + 0.25 * big * big * big) * age * self.rng.next_bipolar().signum();
            self.click_decay = 0.35 + 0.5 * self.rng.next_unit();
            self.click_pan = self.rng.next_unit();
        } else {
            self.click *= -self.click_decay;
        }
        self.whine_phase = (self.whine_phase + 15_734.0 / self.host_hz).fract();
        let whine = if 15_734.0 < 0.45 * self.host_hz { v.whine * a2 * (TAU * self.whine_phase).sin() } else { 0.0 };

        let mut out = [0.0; 2];
        for ch in 0..2 {
            let s = soft(y[ch] * drive) / drive.clamp(1.0, 2.0);
            let s = self.lp.process_lp(s, ch);
            let s = self.hp.process(s, ch).hp;
            let pan = if ch == 0 { 1.0 - self.click_pan } else { self.click_pan };
            let worn = s * self.drop_gain + hiss[ch] + rum + self.click * (0.4 + 0.6 * pan) + whine;
            out[ch] = y[ch] + blend * (worn - y[ch]);
        }
        out
    }
}
