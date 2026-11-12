//! Voice — the cone's modal fingerprint.
//! Above the piston band a cone is a bending-wave system; the corpus
//! says exactly what that means and this component is built to it:
//!
//! * a FIXED modal set per speaker build (§1: identity is modal —
//!   frequencies and Qs are the fingerprint; deterministic hash
//!   jitter, never RNG), realized as a SERIES peaking chain: real
//!   IRs are minimum-phase (§5), and a min-phase magnitude implies
//!   its ring — the curve IS the clock;
//! * mode Qs drawn from the measured T(f) ring table (§2) — the
//!   1–1.3 kHz depth band rings ~2× its neighbors; NEVER uniform Q;
//! * mic position morphs mode WEIGHTS and the cap-cavity comb, not
//!   the frequencies (§1: ~69% of peaks persist cap→edge — a law
//!   brackets it), plus the corpus-calibrated macro walk (§7:
//!   −4.6 dB tilt, corner 6.8→5.8 kHz);
//! * excursion softens the song (drive-dependent breakup — the
//!   living-leg behavior no IR can render): the motor's excursion
//!   envelope scales mode gain and Q at control rate;
//! * the piston cliff stays the staggered LP pair voiced onto the
//!   population median (fidelity report).

use stave_dsp::primitives::random::{splitmix64, unit_f32};
use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, DelayLine};

const N_MODES: usize = 40;
/// Same size axis as motor::SIZES — the cliff base per driver size,
/// voiced so the −12 dB point lands on the York median.
const CLIFF_HZ: [f32; 4] = [5_700.0, 4_700.0, 3_950.0, 3_150.0];

/// The measured ring law (plugins/tolex/docs/corpus.md §2, trusted rows):
/// (Hz, T60 ms). Between rows: log-f interpolation. The 1–1.3 kHz
/// bump is the depth band.
const RING_LAW: [(f32, f32); 8] = [
    (700.0, 50.0),
    (1_000.0, 75.0),
    (1_300.0, 77.0),
    (1_600.0, 35.0),
    (2_500.0, 24.0),
    (4_000.0, 19.0),
    (6_300.0, 20.0),
    (9_000.0, 12.0),
];

fn ring_t60_ms(f: f32) -> f32 {
    let mut prev = RING_LAW[0];
    for row in RING_LAW {
        if f <= row.0 {
            if row.0 == prev.0 {
                return row.1;
            }
            let t = (f / prev.0).ln() / (row.0 / prev.0).ln();
            return prev.1 + (row.1 - prev.1) * t;
        }
        prev = row;
    }
    RING_LAW[RING_LAW.len() - 1].1
}

/// Deterministic jitter — splitmix64 folded to [0, 1). Modes must be
/// reproducible from the build sheet alone (no RNG in an engine).
fn hash01(seed: u64) -> f32 {
    unit_f32((splitmix64(seed.wrapping_add(0x9E37_79B9_7F4A_7C15)) >> 32) as u32)
}

#[derive(Clone, Copy)]
pub(crate) struct ModeSpec {
    hz: f32,
    q: f32,
    /// Base prominence, dB, signed (peaks AND dips — interference
    /// carves both into a real curve).
    db: f32,
    /// Position sensitivity: phase and rate of the weight walk.
    walk_phase: f32,
    walk_rate: f32,
    /// 0 at the band's bottom, 1 at its top — doping reaches the
    /// top of the song first, like real surround treatment.
    t: f32,
    /// < 0: absolute walk (invented cones). >= 0: the capture
    /// position a MEASURED anchor is exact at — the walk becomes a
    /// ratio, unity there, morphing away (the fit exposed this: an
    /// absolute walk scrambles measured amplitudes).
    walk_ref: f32,
}

// ANCHOR: YORK 212 M65, 57 on cap — 39 measured modes (analysis
// peak/Q extraction on the reference capture;
// anchors are MEASURED fingerprints, hash variants are invented
// ones, one law serves both).
pub(crate) const ANCHOR_M65: [(f32, f32, f32); 39] = [
    (591.3, 26.7, -13.54),
    (614.7, 7.9, 8.13),
    (767.1, 12.7, -4.60),
    (813.2, 7.6, 4.60),
    (983.8, 10.3, -8.13),
    (1030.8, 8.8, 16.35),
    (1109.7, 37.1, -6.40),
    (1135.9, 23.8, 6.40),
    (1190.2, 37.6, -7.71),
    (1208.8, 50.6, 4.80),
    (1237.3, 41.8, -4.80),
    (1281.4, 14.4, 7.71),
    (1353.0, 13.4, -5.24),
    (1445.4, 13.8, 1.55),
    (1514.5, 20.2, -1.55),
    (1649.6, 7.0, 3.00),
    (1776.1, 43.6, -1.03),
    (1810.9, 60.0, 1.03),
    (1846.5, 31.7, -3.00),
    (1980.3, 33.7, 2.53),
    (2019.1, 16.9, -2.53),
    (2295.4, 9.4, 3.87),
    (2490.6, 12.9, -3.87),
    (2734.1, 12.0, 5.24),
    (2955.1, 12.1, -8.02),
    (3385.7, 8.3, 4.94),
    (3673.7, 16.5, -4.94),
    (3819.3, 13.5, 3.95),
    (4064.3, 42.9, -1.20),
    (4209.0, 26.0, 1.20),
    (4566.9, 10.1, -3.95),
    (4936.1, 3.2, 8.02),
    (5546.5, 16.4, -1.27),
    (5677.4, 18.0, 1.27),
    (6088.8, 29.7, -1.34),
    (6208.2, 46.4, 1.34),
    (6658.1, 22.8, -9.52),
    (7366.1, 8.8, 9.52),
    (7838.7, 44.6, -6.46),
];
/// First anchor slot on the cone axis (0..31 = hash variants).
pub(crate) const ANCHOR_BASE: usize = 32;

/// The cliff base for a build sheet — shared by the audio path and
/// the display twin (curve.rs).
pub(crate) fn cliff_hz(size: usize, mass: f32) -> f32 {
    CLIFF_HZ[size.min(CLIFF_HZ.len() - 1)] / mass.max(0.25).sqrt()
}

/// The fixed modal fingerprint for a build sheet. Pure — identical
/// specs on the audio and display sides by construction.
pub(crate) fn spec_for(size: usize, mass: f32, variant: usize, rate: f32) -> [ModeSpec; N_MODES] {
    let size = size.min(CLIFF_HZ.len() - 1);
    // Anchor slots serve MEASURED fingerprints; the walk/doping
    // fields still come from the hash so an anchored speaker keeps
    // physical position behavior. Mass slides the measured teeth
    // like any cone's.
    if variant >= ANCHOR_BASE {
        let table = &ANCHOR_M65; // slot 32 (more anchors join here)
        let f_scale = 1.0 / mass.max(0.25).sqrt();
        let (f_lo, f_hi) = (table[0].0 * f_scale, table[table.len() - 1].0 * f_scale);
        let span = (f_hi / f_lo).ln().max(0.1);
        let mut spec = [ModeSpec {
            hz: 1000.0,
            q: 10.0,
            db: 0.0,
            walk_phase: 0.0,
            walk_rate: 1.0,
            t: 0.0,
            walk_ref: -1.0,
        }; N_MODES];
        for (k, slot) in spec.iter_mut().enumerate() {
            let (hz, q, db) = table[k.min(table.len() - 1)];
            let hz = hz * f_scale;
            let seed = 0xA0u64 << 16 | (size as u64) << 8 | k as u64;
            *slot = ModeSpec {
                hz,
                q,
                // Neutral at the DEFAULT breakup (0.6) and doping-free:
                // the sheet reproduces the capture untouched.
                db: if k < table.len() {
                    db * (0.54 / 0.6)
                } else {
                    0.0
                },
                walk_phase: hash01(seed.wrapping_mul(13) + 5),
                walk_rate: 0.35 + 1.1 * hash01(seed.wrapping_mul(17) + 6),
                t: ((hz / f_lo).ln() / span).clamp(0.0, 1.0),
                walk_ref: 0.1, // the 57-on-cap capture position
            };
        }
        return spec;
    }
    // The CONE axis: variant selects a fingerprint from the
    // same physical law — 0 is the original voice, bit-exactly.
    let vseed = (variant as u64) << 16;
    let cliff = cliff_hz(size, mass);
    let f_lo = cliff * 0.18;
    let f_hi = (cliff * 2.0).min(rate * 0.42);
    let span = (f_hi / f_lo).ln();
    let mut spec = [ModeSpec {
        hz: 1000.0,
        q: 10.0,
        db: 0.0,
        walk_phase: 0.0,
        walk_rate: 1.0,
        t: 0.0,
        walk_ref: -1.0,
    }; N_MODES];
    for (k, slot) in spec.iter_mut().enumerate() {
        let seed = vseed | (size as u64) << 8 | k as u64;
        let jf = hash01(seed.wrapping_mul(3) + 1) - 0.5;
        let t = (k as f32 + 0.5 + 0.8 * jf) / N_MODES as f32;
        let hz = f_lo * (span * t).exp();
        let t60 = ring_t60_ms(hz) * 1e-3;
        let jq = 0.6 + 1.3 * hash01(seed.wrapping_mul(5) + 2);
        let q = (core::f32::consts::PI * hz * t60 / 6.91 * jq).clamp(2.0, 60.0);
        let jp = hash01(seed.wrapping_mul(7) + 3);
        let sign = if hash01(seed.wrapping_mul(11) + 4) < 0.62 {
            1.0
        } else {
            -1.0
        };
        let db = sign * (2.0 + 7.0 * jp * jp);
        *slot = ModeSpec {
            hz,
            q,
            db,
            // Rates must be NON-integer: an integer rate makes
            // cos(2π(φ + rate·pos)) identical at pos 0 and 1 —
            // a knob that walks in a perfect circle (the
            // persistence law's upper bound caught exactly this).
            walk_phase: hash01(seed.wrapping_mul(13) + 5),
            walk_rate: 0.35 + 1.1 * hash01(seed.wrapping_mul(17) + 6),
            t,
            walk_ref: -1.0,
        };
    }
    spec
}

/// One mode's live (gain dB, Q) under the weights — position walk,
/// doping, breakup level, softening. Shared with the display twin.
pub(crate) fn mode_voicing(
    s: &ModeSpec,
    breakup: f32,
    doping: f32,
    position: f32,
    soften: f32,
) -> (f32, f32) {
    let level = breakup / 0.54; // default sits at corpus ripple
    let soft_g = 1.0 / (1.0 + 0.35 * soften);
    let soft_q = 1.0 / (1.0 + 0.50 * soften);
    let swing =
        |p: f32| 1.0 + 0.85 * (core::f32::consts::TAU * (s.walk_phase + s.walk_rate * p)).cos();
    let w = if s.walk_ref < 0.0 {
        swing(position)
    } else {
        // Anchored: exact at the capture position, morphing away.
        (swing(position) / swing(s.walk_ref).max(0.15)).clamp(0.15, 3.0)
    };
    // Anchored modes carry their own damping story; doping starts
    // from neutral there (0.35 default) rather than absolute.
    let dope = if s.walk_ref < 0.0 {
        1.0 - doping * (0.25 + 0.75 * s.t)
    } else {
        1.0 - (doping - 0.35) * (0.25 + 0.75 * s.t)
    };
    let db = s.db * level * w * dope.max(0.0) * soft_g;
    let q = (s.q * (1.0 - 0.5 * doping * s.t) * soft_q).max(1.5);
    (db, q)
}

/// The macro tilt walk: +0.9 dB at the cap to −3.7 at the edge
/// (the corpus calibration, plugins/tolex/docs/corpus.md §7).
pub(crate) fn macro_tilt(position: f32) -> f32 {
    0.9 - 4.6 * position
}

/// The cap-cavity comb geometry: (delay s, gain).
pub(crate) fn comb_geometry(position: f32, breakup: f32) -> (f32, f32) {
    let level = breakup / 0.54;
    ((0.10 + 0.30 * position) * 1e-3, 0.18 * level.min(1.5))
}

/// The display twin: the voice's EXACT magnitude (dB, added into
/// `out`) at each `hz`, assembled from the same spec and voicing
/// math as the audio path — same file, same functions, so the
/// picture cannot drift from the sound (and a law pins it anyway).
#[allow(clippy::too_many_arguments)]
pub(crate) fn response_add(
    size: usize,
    variant: usize,
    mass: f32,
    breakup: f32,
    doping: f32,
    position: f32,
    soften: f32,
    rate: f32,
    hz: &[f32],
    out: &mut [f32],
) {
    let spec = spec_for(size, mass, variant, rate);
    let mut bq = Biquad::new();
    let add_current = |bq: &Biquad, out: &mut [f32]| {
        for (i, f) in hz.iter().enumerate() {
            out[i] += 20.0 * bq.magnitude_at(*f, rate).max(1e-12).log10();
        }
    };
    for s in &spec {
        let (db, q) = mode_voicing(s, breakup, doping, position, soften);
        bq.set(FilterSpec::Peak { hz: s.hz, db, q }, rate);
        add_current(&bq, out);
    }
    let cliff = cliff_hz(size, mass) * (1.0 - 0.15 * position);
    bq.set(FilterSpec::Lowpass { hz: cliff, q: 0.62 }, rate);
    add_current(&bq, out);
    bq.set(
        FilterSpec::Lowpass {
            hz: cliff * 2.1,
            q: 0.62,
        },
        rate,
    );
    add_current(&bq, out);
    let tilt = macro_tilt(position);
    bq.set(
        FilterSpec::LowShelfRbj {
            hz: 500.0,
            db: -tilt * 0.5,
        },
        rate,
    );
    add_current(&bq, out);
    bq.set(
        FilterSpec::HighShelfRbj {
            hz: 2_500.0,
            db: tilt * 0.5,
        },
        rate,
    );
    add_current(&bq, out);
    let (tau, g) = comb_geometry(position, breakup);
    for (i, f) in hz.iter().enumerate() {
        let w = core::f32::consts::TAU * *f * tau;
        let (re, im) = (1.0 - g * w.cos(), g * w.sin());
        out[i] += 10.0 * (re * re + im * im).max(1e-24).log10();
    }
}

pub struct Voice {
    spec: [ModeSpec; N_MODES],
    modes: [Biquad; N_MODES],
    lp_a: Biquad,
    lp_b: Biquad,
    tilt_lo: Biquad,
    tilt_hi: Biquad,
    comb: DelayLine,
    comb_samples: f32,
    comb_g: f32,
    rate: f32,
    // Build sheet (cached for control-rate re-voicing):
    size: usize,
    variant: usize,
    mass: f32,
    breakup: f32,
    doping: f32,
    position: f32,
    soften: f32,
}

impl Voice {
    pub fn new() -> Self {
        Self {
            spec: [ModeSpec {
                hz: 1000.0,
                q: 10.0,
                db: 0.0,
                walk_phase: 0.0,
                walk_rate: 1.0,
                t: 0.0,
                walk_ref: -1.0,
            }; N_MODES],
            modes: [Biquad::new(); N_MODES],
            lp_a: Biquad::new(),
            lp_b: Biquad::new(),
            tilt_lo: Biquad::new(),
            tilt_hi: Biquad::new(),
            comb: DelayLine::new(),
            comb_samples: 0.0,
            comb_g: 0.0,
            rate: 0.0,
            size: 2,
            variant: 0,
            mass: 1.0,
            breakup: 0.6,
            doping: 0.35,
            position: 0.25,
            soften: 0.0,
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        // Longest cap-cavity path with headroom.
        self.comb.prepare((self.rate * 0.001) as usize + 8);
        self.comb.reset();
        for m in self.modes.iter_mut() {
            m.reset();
        }
        self.lp_a.reset();
        self.lp_b.reset();
        self.tilt_lo.reset();
        self.tilt_hi.reset();
        self.rebuild_spec();
        self.apply();
    }

    /// The build sheet changed (block rate, Watch-gated).
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self,
        size: usize,
        variant: usize,
        mass: f32,
        breakup: f32,
        doping: f32,
        position: f32,
    ) {
        self.size = size.min(CLIFF_HZ.len() - 1);
        self.variant = variant;
        self.mass = mass;
        self.breakup = breakup;
        self.doping = doping;
        self.position = position;
        if self.rate > 0.0 {
            self.rebuild_spec();
            self.apply();
        }
    }

    /// Excursion feedback (control rate): the cone living at the
    /// Bl-droop scale plays its breakup softer and rounder.
    pub fn soften(&mut self, excursion: f32) {
        let s = excursion.clamp(0.0, 3.0);
        if (s - self.soften).abs() > 0.03 {
            self.soften = s;
            self.apply();
        }
    }

    /// Frequencies and Qs — the speaker's IDENTITY. A function of
    /// the physical sheet only (size, mass), never position or
    /// drive: the corpus says the fingerprint stays put.
    fn rebuild_spec(&mut self) {
        self.spec = spec_for(self.size, self.mass, self.variant, self.rate);
    }

    /// Gains/Qs onto the biquads — position, doping, breakup, and
    /// softening are all WEIGHTS over the fixed spec. Filter state
    /// is kept (a knob move must not cough).
    fn apply(&mut self) {
        // The position walk swings weights ±85% — deep enough that
        // a radiation null can swallow a mode outright, so peaks
        // genuinely come and go with the mic (the persistence law's
        // bracket tuned this depth). All voicing math is shared with
        // the display twin (mode_voicing/macro_tilt/comb_geometry) —
        // the picture cannot drift from the sound.
        for k in 0..N_MODES {
            let s = self.spec[k];
            let (db, q) = mode_voicing(&s, self.breakup, self.doping, self.position, self.soften);
            self.modes[k].set(FilterSpec::Peak { hz: s.hz, db, q }, self.rate);
        }
        // The cliff, with the corpus corner walk (6.8 -> 5.8 kHz).
        let cliff = cliff_hz(self.size, self.mass) * (1.0 - 0.15 * self.position);
        self.lp_a
            .set(FilterSpec::Lowpass { hz: cliff, q: 0.62 }, self.rate);
        self.lp_b.set(
            FilterSpec::Lowpass {
                hz: cliff * 2.1,
                q: 0.62,
            },
            self.rate,
        );
        let tilt = macro_tilt(self.position);
        self.tilt_lo.set(
            FilterSpec::LowShelfRbj {
                hz: 500.0,
                db: -tilt * 0.5,
            },
            self.rate,
        );
        self.tilt_hi.set(
            FilterSpec::HighShelfRbj {
                hz: 2_500.0,
                db: tilt * 0.5,
            },
            self.rate,
        );
        // The cap-cavity comb (corpus §4: dominant lags 0.1-0.3 ms,
        // walking longer toward the edge).
        let (tau_s, g) = comb_geometry(self.position, self.breakup);
        self.comb_samples = tau_s * self.rate;
        self.comb_g = g;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.comb.write(x);
        let c = x - self.comb_g * self.comb.read(self.comb_samples);
        self.comb.advance();
        let mut y = c;
        for m in self.modes.iter_mut() {
            y = m.process(y);
        }
        let y = self.lp_b.process(self.lp_a.process(y));
        self.tilt_hi.process(self.tilt_lo.process(y))
    }
}
