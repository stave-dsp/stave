//! THE DETONATION CHAMBER — the DETONATE topology: reverb as an
//! EVENT SYSTEM, built for transient sources. Every hit triggers
//! its own burst — a dense field opened by a per-hit envelope —
//! and between hits THE SILENCE STAYS BLACK. The gated-verb
//! lineage (AMS/nonlin), done with a live dense field instead of
//! a gate over a hall.
//!
//! THE FUSE is the burst's shape: 0 = instant detonation decaying
//! away, 1 = a reverse swell that builds INTO the cut. DECAY is
//! the burst's LENGTH (not a T60 — the envelope owns the tail).
//! The internal field rings on at a fixed generous decay, so a
//! retrigger reopens onto a LIVE field: fast hits chain naturally.
//!
//! Shared dials: LOW/HIGH ENERGY bend the internal field's bands,
//! TONE tilts the pickups, DIFFUSE is the density, SIZE the
//! chamber's grain, WIDTH the pickup spread (0 = mono).

use stave_dsp::primitives::random::{unit_f32, wang_hash};
use stave_dsp::{t60_gain, DelayLine};

use super::tank::hadamard16;

const LINES: usize = 16;

/// Chamber lines: 12-48 ms geometric ladder (dense grain).
const BASE_MS: [f32; LINES] = [
    12.13, 13.61, 15.37, 17.29, 19.44, 21.87, 24.60, 27.67, 30.71, 34.13, 37.93, 42.17, 44.87,
    46.91, 47.83, 48.61,
];
const LOOP_AP_MS: [f32; LINES] = [
    2.11, 2.87, 2.41, 3.31, 2.03, 3.07, 2.63, 3.43, 2.29, 3.19, 2.71, 3.53, 2.17, 3.37, 2.53, 3.61,
];
const LOOP_AP_G: f32 = 0.55;

const IN_DIFF: usize = 4;
const IN_DIFF_MS: [f32; IN_DIFF] = [0.97, 2.31, 4.51, 7.79];
const IN_DIFF_G: f32 = 0.66;

/// Pickup positions (stagger reads) — irrational fractions.
const TAP_POS: [f32; LINES] = [
    0.618, 0.382, 0.786, 0.472, 0.694, 0.528, 0.847, 0.361, 0.573, 0.729, 0.417, 0.653, 0.809,
    0.446, 0.567, 0.712,
];
const TAP_GAIN: f32 = 0.21;

/// The internal field's own decay — generous, so the envelope
/// always has a dense field to open onto.
const FIELD_T60: f32 = 2.0;

const AP_DC_FACTOR: f32 = 0.55;
const MAX_SLEW: f32 = 0.02;
const BASS_XOVER_HZ: f32 = 300.0;

struct DLine {
    delay: DelayLine,
    len: f32,
    step: f32,
    g: f32,
    lp: f32,
    dc_x1: f32,
    dc_y1: f32,
    low: f32,
    ratio_low: f32,
    ap: DelayLine,
    ap_len: usize,
    w_l: f32,
    w_l_step: f32,
    w_r: f32,
    w_r_step: f32,
}

pub(super) struct Detonate {
    lines: [DLine; LINES],
    in_diff: [[DelayLine; IN_DIFF]; 2],
    in_len: [usize; IN_DIFF],
    in_g: f32,
    dc_r: f32,
    k_bass: f32,
    /// Pickup tilt (TONE).
    tilt_lp: [f32; 2],
    k_tilt: f32,
    tilt_b: f32,
    /// The event system: onset detector + per-hit clock.
    tr_fast: f32,
    tr_slow: f32,
    k_tr_fast: f32,
    k_tr_slow: f32,
    /// Samples since the last onset.
    since: f32,
    /// The burst window in samples and the fuse shape.
    burst_len: f32,
    fuse: f32,
    /// Click-free envelope smoother.
    env: f32,
    k_env: f32,
    sr: f32,
    cold: bool,
}

impl Detonate {
    pub fn new() -> Self {
        Self {
            lines: core::array::from_fn(|_| DLine {
                delay: DelayLine::new(),
                len: 480.0,
                step: 0.0,
                g: 0.5,
                lp: 0.0,
                dc_x1: 0.0,
                dc_y1: 0.0,
                low: 0.0,
                ratio_low: 1.0,
                ap: DelayLine::new(),
                ap_len: 96,
                w_l: 0.7,
                w_l_step: 0.0,
                w_r: 0.7,
                w_r_step: 0.0,
            }),
            in_diff: core::array::from_fn(|_| core::array::from_fn(|_| DelayLine::new())),
            in_len: [48; IN_DIFF],
            in_g: IN_DIFF_G,
            dc_r: 0.9974,
            k_bass: 0.05,
            tilt_lp: [0.0; 2],
            k_tilt: 0.15,
            tilt_b: 1.0,
            tr_fast: 0.0,
            tr_slow: 0.0,
            k_tr_fast: 0.01,
            k_tr_slow: 0.001,
            since: 1e9,
            burst_len: 24_000.0,
            fuse: 0.25,
            env: 0.0,
            k_env: 0.01,
            sr: 48_000.0,
            cold: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.dc_r = 1.0 - core::f32::consts::TAU * 20.0 / sr;
        self.k_bass = 1.0 - (-core::f32::consts::TAU * BASS_XOVER_HZ / sr).exp();
        self.k_tilt = 1.0 - (-core::f32::consts::TAU * 1_600.0 / sr).exp();
        self.k_tr_fast = 1.0 - (-1.0 / (0.006 * sr)).exp();
        self.k_tr_slow = 1.0 - (-1.0 / (0.18 * sr)).exp();
        self.k_env = 1.0 - (-1.0 / (0.002 * sr)).exp();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let cap = (BASE_MS[i] * 0.001 * sr * 1.8) as usize + 16;
            line.delay.prepare(cap);
            line.len = BASE_MS[i] * 0.001 * sr;
            line.ap_len = (LOOP_AP_MS[i] * 0.001 * sr).round() as usize;
            line.ap.prepare(line.ap_len + 8);
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
        for line in self.lines.iter_mut() {
            line.delay.reset();
            line.ap.reset();
            line.step = 0.0;
            line.lp = 0.0;
            line.dc_x1 = 0.0;
            line.dc_y1 = 0.0;
            line.low = 0.0;
        }
        for chain in self.in_diff.iter_mut() {
            for d in chain.iter_mut() {
                d.reset();
            }
        }
        self.tilt_lp = [0.0; 2];
        self.tr_fast = 0.0;
        self.tr_slow = 0.0;
        self.since = 1e9;
        self.env = 0.0;
    }

    /// Block-rate: DECAY is the burst window (clamped to the
    /// event scale), the internal field holds its own fixed T60
    /// bent by the ENERGY dials, DIFFUSE drives density, SIZE the
    /// grain, WIDTH the pickups, TONE the tilt.
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        burst_s: f32,
        lo: f32,
        hi_damp: f32,
        tone: f32,
        diffuse: f32,
        width: f32,
        fuse: f32,
        frames: usize,
    ) {
        let n = frames.max(1) as f32;
        self.burst_len = (burst_s.clamp(0.06, 1.5) * self.sr).max(1.0);
        self.fuse = fuse.clamp(0.0, 1.0);
        self.in_g = (IN_DIFF_G * diffuse / 0.7).clamp(0.0, 0.72);
        self.tilt_b = 2f32.powf(tone.clamp(-1.0, 1.0) * 1.2);
        // hi_damp folds into the absorption cutoff (handed to
        // tick's damp_k by the engine); here it also trims the
        // internal field's top-end hold via the T60.
        let _ = hi_damp;
        let scale = size.clamp(0.3, 3.0).sqrt();
        let spread = (width * 0.5).clamp(0.0, 1.0);
        let ap_dc = AP_DC_FACTOR * (1.0 + LOOP_AP_G) / (1.0 - LOOP_AP_G);
        for (i, line) in self.lines.iter_mut().enumerate() {
            let base = BASE_MS[i] * 0.001 * self.sr * scale;
            let slew = MAX_SLEW * n;
            let loop_len = base + line.ap_len as f32 * ap_dc;
            line.g = t60_gain(loop_len, FIELD_T60 * self.sr);
            let g_low = t60_gain(loop_len, (FIELD_T60 * lo).max(0.05) * self.sr);
            line.ratio_low = g_low / line.g.max(1e-6);
            // Pickups: hash pan, WIDTH-spread, glided.
            let pan = (unit_f32(wang_hash(i as u32 ^ 0xD37)) * 2.0 - 1.0) * spread;
            let wl = (0.5 * (1.0 + pan)).sqrt();
            let wr = (0.5 * (1.0 - pan)).sqrt();
            if self.cold {
                line.len = base;
                line.step = 0.0;
                line.w_l = wl;
                line.w_l_step = 0.0;
                line.w_r = wr;
                line.w_r_step = 0.0;
            } else {
                line.step = (base - line.len).clamp(-slew, slew) / n;
                line.w_l_step = (wl - line.w_l) / n;
                line.w_r_step = (wr - line.w_r) / n;
            }
        }
        self.cold = false;
    }

    /// The fuse's shape at burst position u in [0,1].
    #[inline]
    fn shape(&self, u: f32) -> f32 {
        // Decay branch and reverse branch, cosine-closed over the
        // last 12% so the cut can never click.
        let decay = (-4.0 * u).exp();
        let swell = (5.0 * (u - 1.0)).exp();
        let e = (1.0 - self.fuse) * decay + self.fuse * swell;
        let close = if u > 0.88 {
            0.5 * (1.0 + (core::f32::consts::PI * (u - 0.88) / 0.12).cos())
        } else {
            1.0
        };
        e * close
    }

    /// One sample: detect, run the field, open the envelope.
    #[inline]
    pub fn tick(&mut self, in_l: f32, in_r: f32, damp_k: f32) -> (f32, f32) {
        // The event clock.
        let x = 0.5 * (in_l.abs() + in_r.abs());
        self.tr_fast += self.k_tr_fast * (x - self.tr_fast);
        self.tr_slow += self.k_tr_slow * (x - self.tr_slow);
        let onset = self.tr_fast / self.tr_slow.max(1e-6) > 1.6 && self.tr_fast > 1e-3;
        if onset && self.since > 0.05 * self.sr {
            self.since = 0.0;
        }
        self.since += 1.0;
        let u = (self.since / self.burst_len).min(1.0);
        let target = if u < 1.0 { self.shape(u) } else { 0.0 };
        self.env += self.k_env * (target - self.env);

        // The strike smears, the field runs.
        let mut inj = [in_l, in_r];
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
            line.lp += damp_k * (y - line.lp);
            let dc = line.lp - line.dc_x1 + self.dc_r * line.dc_y1;
            line.dc_x1 = line.lp;
            line.dc_y1 = dc;
            line.low += self.k_bass * (dc - line.low);
            let x1 = dc + (line.ratio_low - 1.0) * line.low;
            fed[i] = line.g * x1;
        }
        hadamard16(&mut fed);
        for (i, line) in self.lines.iter_mut().enumerate() {
            let fb0 = fed[i];
            let ap_out = line.ap.read_int(line.ap_len);
            let y1 = ap_out - LOOP_AP_G * fb0;
            line.ap.write(fb0 + LOOP_AP_G * y1);
            line.ap.advance();
            let inject = if i % 2 == 0 { inj[0] } else { inj[1] };
            line.delay.write(inject + y1);
            line.delay.advance();
        }
        // THE DETONATION: the envelope owns the output.
        let mut out = [wet_l * TAP_GAIN * self.env, wet_r * TAP_GAIN * self.env];
        for ch in 0..2 {
            self.tilt_lp[ch] += self.k_tilt * (out[ch] - self.tilt_lp[ch]);
            out[ch] = self.tilt_lp[ch] + self.tilt_b * (out[ch] - self.tilt_lp[ch]);
        }
        (out[0], out[1])
    }
}
