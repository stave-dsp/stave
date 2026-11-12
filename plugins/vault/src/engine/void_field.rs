//! THE VOID — the wall-less topology: no early reflections, no
//! geometry, no modes — a vast statistical field and nothing
//! else. Sixteen lines far longer than any room's (55-150 ms),
//! heavy diffusion in and in-loop, and DAWN: a send-side swell
//! (slow-attack envelope on the injection) so transients RISE
//! INTO the field instead of splashing against it. At dawn 0 the
//! swell is bit-absent.
//!
//! Shared dials: DECAY is the field's T60 (to 30 s — near-
//! infinite), LOW/HIGH ENERGY bend it, TONE tilts the pickups,
//! DIFFUSE the wash density, SIZE the field's vastness, WIDTH the
//! pickup spread (0 = mono).

use stave_dsp::primitives::random::{unit_f32, wang_hash};
use stave_dsp::{t60_gain, DelayLine};

use super::tank::hadamard16;

const LINES: usize = 16;

/// 55-150 ms geometric ladder — beyond any room.
const BASE_MS: [f32; LINES] = [
    55.13, 60.91, 67.23, 74.19, 81.87, 90.31, 99.67, 109.93, 121.24, 129.47, 136.73, 141.79,
    145.31, 147.83, 149.21, 150.67,
];
const LOOP_AP_MS: [f32; LINES] = [
    4.31, 6.17, 5.03, 7.91, 4.03, 6.43, 5.87, 7.67, 4.53, 6.29, 5.97, 7.79, 4.23, 6.53, 5.67, 7.97,
];
const LOOP_AP_G: f32 = 0.55;

const IN_DIFF: usize = 6;
const IN_DIFF_MS: [f32; IN_DIFF] = [2.13, 4.87, 8.31, 12.71, 17.93, 23.39];
const IN_DIFF_G: f32 = 0.68;

const TAP_POS: [f32; LINES] = [
    0.618, 0.382, 0.786, 0.472, 0.694, 0.528, 0.847, 0.361, 0.573, 0.729, 0.417, 0.653, 0.809,
    0.446, 0.567, 0.712,
];
const TAP_GAIN: f32 = 0.177;

/// A whisper of motion, always on (the anti-metallic bake).
const SHEEN: f32 = 0.8;

const AP_DC_FACTOR: f32 = 0.55;
const MAX_SLEW: f32 = 0.02;
const BASS_XOVER_HZ: f32 = 250.0;
const HI_XOVER_HZ: f32 = 2_500.0;

struct VLine {
    delay: DelayLine,
    len: f32,
    step: f32,
    g: f32,
    g_step: f32,
    lp: f32,
    dc_x1: f32,
    dc_y1: f32,
    low: f32,
    ratio_low: f32,
    hi: f32,
    ratio_hi: f32,
    ap: DelayLine,
    ap_len: usize,
    ap2: DelayLine,
    ap2_len: usize,
    sheen_ph: f32,
    sheen_rate: f32,
    w_l: f32,
    w_l_step: f32,
    w_r: f32,
    w_r_step: f32,
}

pub(super) struct Void {
    lines: [VLine; LINES],
    in_diff: [[DelayLine; IN_DIFF]; 2],
    in_len: [usize; IN_DIFF],
    in_g: f32,
    dc_r: f32,
    k_bass: f32,
    k_hi: f32,
    tilt_lp: [f32; 2],
    k_tilt: f32,
    tilt_b: f32,
    /// THE DAWN: send-side swell state.
    dawn_env: f32,
    k_dawn_a: f32,
    k_dawn_r: f32,
    dawn_amt: f32,
    sr: f32,
    cold: bool,
}

impl Void {
    pub fn new() -> Self {
        Self {
            lines: core::array::from_fn(|_| VLine {
                delay: DelayLine::new(),
                len: 4_800.0,
                step: 0.0,
                g: 0.5,
                g_step: 0.0,
                lp: 0.0,
                dc_x1: 0.0,
                dc_y1: 0.0,
                low: 0.0,
                ratio_low: 1.0,
                hi: 0.0,
                ratio_hi: 1.0,
                ap: DelayLine::new(),
                ap_len: 96,
                ap2: DelayLine::new(),
                ap2_len: 192,
                sheen_ph: 0.0,
                sheen_rate: 0.3,
                w_l: 0.7,
                w_l_step: 0.0,
                w_r: 0.7,
                w_r_step: 0.0,
            }),
            in_diff: core::array::from_fn(|_| core::array::from_fn(|_| DelayLine::new())),
            in_len: [96; IN_DIFF],
            in_g: IN_DIFF_G,
            dc_r: 0.9974,
            k_bass: 0.05,
            k_hi: 0.25,
            tilt_lp: [0.0; 2],
            k_tilt: 0.15,
            tilt_b: 1.0,
            dawn_env: 0.0,
            k_dawn_a: 0.001,
            k_dawn_r: 0.001,
            dawn_amt: 0.0,
            sr: 48_000.0,
            cold: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.dc_r = 1.0 - core::f32::consts::TAU * 20.0 / sr;
        self.k_bass = 1.0 - (-core::f32::consts::TAU * BASS_XOVER_HZ / sr).exp();
        self.k_hi = 1.0 - (-core::f32::consts::TAU * HI_XOVER_HZ / sr).exp();
        self.k_tilt = 1.0 - (-core::f32::consts::TAU * 1_600.0 / sr).exp();
        self.k_dawn_r = 1.0 - (-1.0 / (0.25 * sr)).exp();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let cap = (BASE_MS[i] * 0.001 * sr * 1.8) as usize + 16;
            line.delay.prepare(cap);
            line.len = BASE_MS[i] * 0.001 * sr;
            line.ap_len = (LOOP_AP_MS[i] * 0.001 * sr).round() as usize;
            line.ap.prepare(line.ap_len + 8);
            line.ap2_len = (LOOP_AP_MS[(i + 7) % LINES] * 1.7 * 0.001 * sr).round() as usize;
            line.ap2.prepare(line.ap2_len + 8);
            line.sheen_ph = (i as f32) * 2.399_963;
            line.sheen_rate = 0.3 * (0.55 + 0.9 * ((i as f32 * 0.618_034) % 1.0));
        }
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            for (k, d) in chain.iter_mut().enumerate() {
                let det = if ch == 0 { 1.0 } else { 1.07 };
                self.in_len[k] = (IN_DIFF_MS[k] * det * 0.001 * sr).round() as usize;
                d.prepare(self.in_len[k] + 8);
            }
        }
        self.reset();
        self.cold = true;
    }

    pub fn reset(&mut self) {
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.delay.reset();
            line.ap.reset();
            line.ap2.reset();
            line.step = 0.0;
            line.g_step = 0.0;
            line.lp = 0.0;
            line.dc_x1 = 0.0;
            line.dc_y1 = 0.0;
            line.low = 0.0;
            line.hi = 0.0;
            line.sheen_ph = (i as f32) * 2.399_963;
        }
        for chain in self.in_diff.iter_mut() {
            for d in chain.iter_mut() {
                d.reset();
            }
        }
        self.tilt_lp = [0.0; 2];
        self.dawn_env = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        t60: f32,
        lo: f32,
        hi_mult: f32,
        tone: f32,
        diffuse: f32,
        width: f32,
        dawn: f32,
        frames: usize,
    ) {
        let n = frames.max(1) as f32;
        let block_s = n / self.sr;
        self.in_g = (IN_DIFF_G * diffuse / 0.7).clamp(0.0, 0.72);
        self.tilt_b = 2f32.powf(tone.clamp(-1.0, 1.0) * 1.2);
        // THE DAWN: attack 30 ms..0.8 s with the dial; 0 = absent.
        self.dawn_amt = dawn.clamp(0.0, 1.0);
        let att = 0.03 + 0.77 * self.dawn_amt;
        self.k_dawn_a = 1.0 - (-1.0 / (att * self.sr)).exp();
        let scale = size.clamp(0.3, 3.0).sqrt();
        let spread = (width * 0.5).clamp(0.0, 1.0);
        let ap_dc = AP_DC_FACTOR * (1.0 + LOOP_AP_G) / (1.0 - LOOP_AP_G);
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.sheen_ph = (line.sheen_ph + core::f32::consts::TAU * line.sheen_rate * block_s)
                % core::f32::consts::TAU;
            let base = BASE_MS[i] * 0.001 * self.sr * scale + SHEEN * line.sheen_ph.sin();
            let slew = MAX_SLEW * n;
            let loop_len = base + (line.ap_len + line.ap2_len) as f32 * ap_dc;
            let g_new = t60_gain(loop_len, t60 * self.sr);
            let g_low = t60_gain(loop_len, t60 * lo * self.sr);
            let g_hi = t60_gain(loop_len, t60 * hi_mult * self.sr);
            let pan = (unit_f32(wang_hash(i as u32 ^ 0xF01D)) * 2.0 - 1.0) * spread;
            let wl = (0.5 * (1.0 + pan)).sqrt();
            let wr = (0.5 * (1.0 - pan)).sqrt();
            if self.cold {
                line.len = base;
                line.step = 0.0;
                line.g = g_new;
                line.g_step = 0.0;
                line.ratio_low = g_low / g_new.max(1e-6);
                line.ratio_hi = g_hi / g_new.max(1e-6);
                line.w_l = wl;
                line.w_l_step = 0.0;
                line.w_r = wr;
                line.w_r_step = 0.0;
            } else {
                line.step = (base - line.len).clamp(-slew, slew) / n;
                line.g_step = (g_new - line.g) / n;
                line.ratio_low = g_low / g_new.max(1e-6);
                line.ratio_hi = g_hi / g_new.max(1e-6);
                line.w_l_step = (wl - line.w_l) / n;
                line.w_r_step = (wr - line.w_r) / n;
            }
        }
        self.cold = false;
    }

    #[inline]
    pub fn tick(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        // THE DAWN: the send swells in (bit-absent at 0).
        let (xl0, xr0) = if self.dawn_amt > 1e-4 {
            let x = 0.5 * (in_l.abs() + in_r.abs());
            let k = if x > self.dawn_env {
                self.k_dawn_a
            } else {
                self.k_dawn_r
            };
            self.dawn_env += k * (x - self.dawn_env);
            let g = (self.dawn_env * 3.0).min(1.0);
            let g = 1.0 - self.dawn_amt * (1.0 - g);
            (in_l * g, in_r * g)
        } else {
            (in_l, in_r)
        };
        let mut inj = [xl0, xr0];
        for (ch, chain) in self.in_diff.iter_mut().enumerate() {
            let mut v = inj[ch];
            for (k, d) in chain.iter_mut().enumerate() {
                let y = d.read_int(self.in_len[k]) - self.in_g * v;
                d.write(v + self.in_g * y);
                d.advance();
                v = y;
            }
            inj[ch] = v;
        }
        let mut fed = [0.0f32; LINES];
        let mut wet_l = 0.0f32;
        let mut wet_r = 0.0f32;
        for (i, line) in self.lines.iter_mut().enumerate() {
            let y = line.delay.read_cubic(line.len);
            let yo = line.delay.read(line.len * TAP_POS[i]);
            line.len += line.step;
            wet_l += line.w_l * yo;
            wet_r += line.w_r * yo;
            line.w_l += line.w_l_step;
            line.w_r += line.w_r_step;
            let dc = y - line.dc_x1 + self.dc_r * line.dc_y1;
            line.dc_x1 = y;
            line.dc_y1 = dc;
            line.low += self.k_bass * (dc - line.low);
            let x1 = dc + (line.ratio_low - 1.0) * line.low;
            line.hi += self.k_hi * (x1 - line.hi);
            let x2 = x1 + (line.ratio_hi - 1.0) * (x1 - line.hi);
            fed[i] = line.g * x2;
            line.g += line.g_step;
        }
        hadamard16(&mut fed);
        for (i, line) in self.lines.iter_mut().enumerate() {
            let fb0 = fed[i];
            let ap_out = line.ap.read_int(line.ap_len);
            let y1 = ap_out - LOOP_AP_G * fb0;
            line.ap.write(fb0 + LOOP_AP_G * y1);
            line.ap.advance();
            let ap2_out = line.ap2.read_int(line.ap2_len);
            let y2 = ap2_out - LOOP_AP_G * y1;
            line.ap2.write(y1 + LOOP_AP_G * y2);
            line.ap2.advance();
            let inject = if i % 2 == 0 { inj[0] } else { inj[1] };
            line.delay.write(inject + y2);
            line.delay.advance();
        }
        let mut out = [wet_l * TAP_GAIN, wet_r * TAP_GAIN];
        for ch in 0..2 {
            self.tilt_lp[ch] += self.k_tilt * (out[ch] - self.tilt_lp[ch]);
            out[ch] = self.tilt_lp[ch] + self.tilt_b * (out[ch] - self.tilt_lp[ch]);
        }
        (out[0], out[1])
    }
}
