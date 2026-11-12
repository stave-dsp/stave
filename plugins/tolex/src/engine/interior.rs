//! Interior — the box's own voice.
//! Two layers, both scaled by the cabinet's physical size ∛V:
//!
//! * SHORT TAPS — the corpus shows close-mic'd reflections are
//!   baffle-edge and side-wall scale (0.3–1.0 ms; essentially no
//!   discrete energy beyond 1 ms — back-wall energy arrives diffuse
//!   and small). Three taps: baffle edge, side wall, and a weak
//!   lowpassed depth return, each through its own diffraction/
//!   through-cone lowpass.
//! * AIR MODES — the interior's axial standing waves at c/(2·dim)
//!   for each box dimension: RESONATORS, not combs — this is where
//!   the population's low-band fine structure actually lives (a
//!   sub-millisecond comb's ripple sits above 600 Hz; the corpus
//!   band 100–600 Hz demands poles).
//! * PANEL MODES — the material knob: each material is a small set
//!   of low-mid resonators (series peaking, ±dB) whose frequencies
//!   ride 1/∛V (bigger boxes, lower panels). Birch ply is stiff and
//!   ringy, pine is loud and low, MDF is dead — the classic lore,
//!   here as numbers the laws can hold.

use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, DelayLine};

const C_AIR: f32 = 343.0;
/// Reference box scale: ∛(60 L).
const S_REF: f32 = 0.391_487;

/// (width, height, depth) as multiples of the box scale ∛V — a
/// classic guitar-cab aspect.
const ASPECT: (f32, f32, f32) = (1.3, 1.0, 0.55);

/// Tap geometry: (path fraction of scale, gain, lowpass Hz).
/// Path = fraction·∛V; delays land 0.3–1.0 ms across the cab range.
const TAPS: [(f32, f32, f32); 3] = [
    (ASPECT.0 * 0.5, -0.22, 3_200.0), // baffle edge (half width)
    (ASPECT.1 * 0.5, 0.12, 1_200.0),  // side wall (half height)
    (ASPECT.2 * 2.0, -0.07, 900.0),   // depth round trip, weak
];

/// Axial air modes: per box dimension (aspect fraction), signed
/// level dB and Q. f = c/(2·dim) — 337/438/797 Hz at 60 L.
const AIR_MODES: [(f32, f32, f32); 3] = [
    (ASPECT.0, 4.0, 16.0),
    (ASPECT.1, -3.4, 14.0),
    (ASPECT.2, 3.0, 12.0),
];

/// Panel modes per material: (Hz at the 60 L reference, dB, Q).
const PANELS: [[(f32, f32, f32); 3]; 3] = [
    // BIRCH — stiff ply: higher, tighter, modest.
    [(170.0, 3.7, 11.0), (310.0, -2.9, 9.0), (455.0, 2.6, 10.0)],
    // PINE — loud, low, loose: the vintage rattle.
    [(135.0, 6.0, 8.0), (240.0, 4.2, 7.0), (380.0, -3.6, 8.0)],
    // MDF — dead: low Q, low level.
    [(150.0, 1.3, 4.0), (280.0, -1.0, 4.0), (420.0, 0.8, 4.0)],
];

/// Shared-interior cross-talk: neighbor cones are acoustic openings
/// in the same box; their paths arrive through the near cone at
/// lattice spacing (0.33 m, and the diagonal). Active per layout —
/// this is the mechanism behind the corpus's config ordering (412s
/// are notchier than 212s even close-mic'd, where EXTERIOR neighbor
/// radiation is buried by directivity).
const CROSS_TAPS: [(f32, f32, usize); 2] = [
    (0.33, -0.14, 1),  // adjacent, active from 2x12
    (0.4667, 0.13, 2), // diagonal, active from 4x12
];
/// Adjacent-neighbor count by layout: a 4x12 cone has TWO adjacent
/// neighbors (energy-summed weight), a 2x12 one, a 1x12 none.
const ADJ_WEIGHT: [f32; 3] = [0.0, 1.0, 1.6];

pub struct Interior {
    delay: DelayLine,
    /// The depth path RECIRCULATES — repeated wall bounces are the
    /// box's diffuse tail (the corpus's late-energy fraction, and
    /// what makes a big box's t90 longer; single taps move t90 the
    /// WRONG way by front-loading energy). A leaf-internal feedback
    /// ring, g < 0.6 by construction.
    ring: DelayLine,
    ring_samples: f32,
    ring_fb: f32,
    ring_out: f32,
    ring_lp: Biquad,
    tap_samples: [f32; 3],
    tap_gain: [f32; 3],
    tap_lp: [Biquad; 3],
    cross_samples: [f32; 2],
    cross_gain: [f32; 2],
    air: [Biquad; 3],
    panels: [Biquad; 3],
    rate: f32,
}

impl Interior {
    pub fn new() -> Self {
        Self {
            delay: DelayLine::new(),
            ring: DelayLine::new(),
            ring_samples: 0.0,
            ring_fb: 0.0,
            ring_out: 0.0,
            ring_lp: Biquad::new(),
            tap_samples: [0.0; 3],
            tap_gain: [0.0; 3],
            tap_lp: [Biquad::new(); 3],
            cross_samples: [0.0; 2],
            cross_gain: [0.0; 2],
            air: [Biquad::new(); 3],
            panels: [Biquad::new(); 3],
            rate: 0.0,
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        // Longest tap (depth round trip at 200 L) with headroom.
        self.delay.prepare((self.rate * 0.003) as usize + 8);
        self.delay.reset();
        self.ring.prepare((self.rate * 0.003) as usize + 8);
        self.ring.reset();
        self.ring_lp.reset();
        for lp in self.tap_lp.iter_mut() {
            lp.reset();
        }
        for a in self.air.iter_mut() {
            a.reset();
        }
        for p in self.panels.iter_mut() {
            p.reset();
        }
    }

    /// Re-voice at block rate; state kept. `layout` is the driver
    /// count selector (0 = 1x12) — cross-talk taps arm with it.
    pub fn set(&mut self, vb_liters: f32, material: usize, open: bool, layout: usize, rate: f32) {
        let s = (vb_liters.max(1.0) * 1e-3).cbrt();
        // Open backs vent the interior: the trapped paths lose level
        // (the edge tap is exterior geometry and keeps its own).
        let vent = if open { 0.7 } else { 1.0 };
        // Bigger boxes diffract and reflect HARDER, not just later —
        // edge extent and wall area grow with the box (the corpus's
        // config ordering law pinned this: 4x12-class boxes carry
        // more deep notches than 2x12-class).
        let tap_scale = (s / S_REF).sqrt().min(1.4);
        for (k, (frac, gain, lp_hz)) in TAPS.iter().enumerate() {
            self.tap_samples[k] = frac * s / C_AIR * rate;
            self.tap_gain[k] = gain * tap_scale * if k == 0 { 1.0 } else { vent };
            self.tap_lp[k].set(FilterSpec::Lowpass { hz: *lp_hz, q: 0.6 }, rate);
        }
        // The diffuse tail: depth-path recirculation, heavier and
        // longer as the box grows (T ~ V/A ~ s).
        self.ring_samples = ASPECT.2 * 2.0 * s / C_AIR * rate;
        self.ring_fb = (0.30 * (s / S_REF)).clamp(0.0, 0.55) * vent;
        self.ring_out = 0.20 * vent;
        self.ring_lp.set(
            FilterSpec::Lowpass {
                hz: 1_000.0,
                q: 0.6,
            },
            rate,
        );
        let lay = layout.min(2);
        for (k, (dist, gain, min_layout)) in CROSS_TAPS.iter().enumerate() {
            self.cross_samples[k] = dist / C_AIR * rate;
            let w = if k == 0 {
                ADJ_WEIGHT[lay]
            } else if lay >= *min_layout {
                1.0
            } else {
                0.0
            };
            self.cross_gain[k] = gain * vent * w;
        }
        // Decay scales with the box: T ~ V/A ~ linear dimension, so
        // both mode Q AND level ride s — a big box's tail is heavier,
        // which is what moves t90 (early taps move it the other way;
        // the config law's t90 ordering pinned this the hard way).
        let q_scale = (s / S_REF).clamp(0.6, 1.8);
        let g_scale = (s / S_REF).powf(0.7).clamp(0.7, 1.5);
        for (k, (frac, db, q)) in AIR_MODES.iter().enumerate() {
            let f = C_AIR / (2.0 * frac * s);
            self.air[k].set(
                FilterSpec::Peak {
                    hz: f,
                    db: db * vent * g_scale,
                    q: q * q_scale,
                },
                rate,
            );
        }
        // Panel AREA rides the box: a 4x12's walls ring louder and
        // carry real late energy (with air-mode level, the honest
        // carrier of the corpus's config tail ordering).
        let panel_scale = (s / S_REF).powf(0.7).clamp(0.7, 1.4);
        let modes = &PANELS[material.min(PANELS.len() - 1)];
        let f_scale = S_REF / s;
        for (k, (hz, db, q)) in modes.iter().enumerate() {
            self.panels[k].set(
                FilterSpec::Peak {
                    hz: hz * f_scale,
                    db: db * panel_scale,
                    q: *q,
                },
                rate,
            );
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.delay.write(x);
        let mut y = x;
        for k in 0..3 {
            y += self.tap_gain[k] * self.tap_lp[k].process(self.delay.read(self.tap_samples[k]));
        }
        for k in 0..2 {
            y += self.cross_gain[k] * self.delay.read(self.cross_samples[k]);
        }
        self.delay.advance();
        let r = self.ring_lp.process(self.ring.read(self.ring_samples));
        y += self.ring_out * r;
        self.ring.write(x + self.ring_fb * r);
        self.ring.advance();
        for a in self.air.iter_mut() {
            y = a.process(y);
        }
        for p in self.panels.iter_mut() {
            y = p.process(y);
        }
        y
    }
}

/// The display twin: |H| in dB — taps as a complex sum, panels as
/// magnitude products, from the SAME tables as tick().
pub(crate) fn response_db(
    vb_liters: f32,
    material: usize,
    open: bool,
    layout: usize,
    hz: f32,
    rate: f32,
) -> f32 {
    let s = (vb_liters.max(1.0) * 1e-3).cbrt();
    let vent = if open { 0.7 } else { 1.0 };
    let tap_scale = (s / S_REF).sqrt().min(1.4);
    let (mut re, mut im) = (1.0f32, 0.0f32);
    let mut lp = Biquad::new();
    for (k, (frac, gain, lp_hz)) in TAPS.iter().enumerate() {
        let tau = frac * s / C_AIR;
        let g = gain * tap_scale * if k == 0 { 1.0 } else { vent };
        lp.set(FilterSpec::Lowpass { hz: *lp_hz, q: 0.6 }, rate);
        let (lr, li) = lp.response_at(hz, rate);
        let th = core::f32::consts::TAU * hz * tau;
        let (c, sn) = (th.cos(), th.sin());
        // g·H_lp·e^{-jθ}
        re += g * (lr * c + li * sn);
        im += g * (li * c - lr * sn);
    }
    let lay = layout.min(2);
    for (k, (dist, gain, min_layout)) in CROSS_TAPS.iter().enumerate() {
        let w = if k == 0 {
            ADJ_WEIGHT[lay]
        } else if lay >= *min_layout {
            1.0
        } else {
            0.0
        };
        let th = core::f32::consts::TAU * hz * dist / C_AIR;
        re += gain * vent * w * th.cos();
        im -= gain * vent * w * th.sin();
    }
    // Ring: g_out·H·e^{-jθ} / (1 − g_fb·H·e^{-jθ}) into the sum.
    {
        let tau = ASPECT.2 * 2.0 * s / C_AIR;
        let g_fb = (0.30 * (s / S_REF)).clamp(0.0, 0.55) * vent;
        let g_out = 0.20 * vent;
        lp.set(
            FilterSpec::Lowpass {
                hz: 1_000.0,
                q: 0.6,
            },
            rate,
        );
        let (hr, hi) = lp.response_at(hz, rate);
        let th = core::f32::consts::TAU * hz * tau;
        let (c, sn) = (th.cos(), th.sin());
        let (er, ei) = (hr * c + hi * sn, hi * c - hr * sn);
        let (dr, di) = (1.0 - g_fb * er, -g_fb * ei);
        let den = (dr * dr + di * di).max(1e-12);
        re += g_out * (er * dr + ei * di) / den;
        im += g_out * (ei * dr - er * di) / den;
    }
    let mut db = 10.0 * (re * re + im * im).max(1e-24).log10();
    let q_scale = (s / S_REF).clamp(0.6, 1.8);
    let g_scale = (s / S_REF).powf(0.7).clamp(0.7, 1.5);
    for (frac, db_m, q) in &AIR_MODES {
        let f = C_AIR / (2.0 * frac * s);
        lp.set(
            FilterSpec::Peak {
                hz: f,
                db: db_m * vent * g_scale,
                q: q * q_scale,
            },
            rate,
        );
        db += 20.0 * lp.magnitude_at(hz, rate).max(1e-12).log10();
    }
    let panel_scale = (s / S_REF).powf(0.7).clamp(0.7, 1.4);
    let modes = &PANELS[material.min(PANELS.len() - 1)];
    let f_scale = S_REF / s;
    for (hz_m, db_m, q) in modes {
        lp.set(
            FilterSpec::Peak {
                hz: hz_m * f_scale,
                db: db_m * panel_scale,
                q: *q,
            },
            rate,
        );
        db += 20.0 * lp.magnitude_at(hz, rate).max(1e-12).log10();
    }
    db
}
