//! THE MODES — the room below its Schroeder frequency, where
//! geometry stops being ray paths and becomes STANDING WAVES. The
//! low end of the machine was a static comb network (the in-loop
//! diffusers are transparent below ~150 Hz and cent-based chorus
//! is frozen at LF — the field heard it exactly: "static,
//! non-moving, one-dimensional"). This bank is the physics that
//! actually governs that region:
//! * 24 FIXED mode identities (axial + tangential orders of the
//!   shoebox) whose FREQUENCIES are derived from the dims —
//!   f = (c/2)·√((nx/Lx)² + (ny/Ly)² + (nz/Lz)²). Identity never
//!   switches with size; a mode just slides (and fades at the band
//!   edges), so SIZE turns never click.
//! * POSITION-DEPENDENT coupling: each mode speaks as
//!   cos(nπx/L)-products at BOTH the source and the listener —
//!   STAND selects which modes you hear (stand at a node and a
//!   mode goes silent: a law).
//! * per-mode T60 spread (real rooms' neighbors decay wildly
//!   differently) — the low tail's timbre EVOLVES as it dies;
//! * slow per-mode DRIFT (±0.4%, hash-rated) — neighboring modes
//!   beat and the beating itself moves;
//! * a resonator charges over its own Q — the LF bloom under
//!   sustained input is free physics.
//!
//! Big rooms slide their modes below audibility (their audible LF
//! is statistical — the tank's job); small rooms go boxy and
//! specific. Both are the truth.

/// Fixed mode identities (nx, ny, nz) — never re-selected.
const MODE_IDX: [(u8, u8, u8); 24] = [
    (1, 0, 0),
    (2, 0, 0),
    (3, 0, 0),
    (0, 1, 0),
    (0, 2, 0),
    (0, 3, 0),
    (0, 4, 0),
    (0, 0, 1),
    (0, 0, 2),
    (1, 1, 0),
    (2, 1, 0),
    (1, 2, 0),
    (2, 2, 0),
    (3, 1, 0),
    (1, 3, 0),
    (1, 0, 1),
    (0, 1, 1),
    (2, 0, 1),
    (0, 2, 1),
    (1, 1, 1),
    (2, 1, 1),
    (1, 2, 1),
    (3, 2, 0),
    (0, 3, 1),
];
pub(super) const MODE_COUNT: usize = MODE_IDX.len();

use super::scene::{Scene, SPEED_M_S};
use stave_dsp::primitives::random::{unit_f32, wang_hash};
use stave_dsp::t60_gain;

/// The modal band: below ~26 Hz is sub-rumble, above ~215 Hz the
/// tank's statistics take over (edge-faded, never switched).
const BAND_LO: f32 = 26.0;
const BAND_HI: f32 = 215.0;
/// Output level (calibrated against the pre-modal LF balance).
/// Voiced against the scoreboard: 14 dragged broadband IACC_late
/// to 0.7 (a too-mono LF layer); 2.5 was inaudible against the
/// tank. 10 keeps the modal character with a live field.
/// Voiced with the modal limiter: 10 leaned on the limiter for
/// its level (an external review caught the 0.10.1 regex silently
/// failing to land this change — every patch asserts now).
const MODE_GAIN: f32 = 5.0;

struct Mode {
    a1: f32,
    a2: f32,
    /// Per-source input couplings (true stereo: L and R stand at
    /// different points of the mode shape).
    k_a: f32,
    k_b: f32,
    y1: f32,
    y2: f32,
    /// Slow detune walk phase (deterministic, hash-rated).
    drift_ph: f32,
}

pub(super) struct Modes {
    modes: [Mode; MODE_COUNT],
    sr: f32,
}

impl Modes {
    pub fn new() -> Self {
        Self {
            modes: core::array::from_fn(|i| Mode {
                a1: 0.0,
                a2: 0.0,
                k_a: 0.0,
                k_b: 0.0,
                y1: 0.0,
                y2: 0.0,
                drift_ph: unit_f32(wang_hash(i as u32)) * core::f32::consts::TAU,
            }),
            sr: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.reset();
    }

    pub fn reset(&mut self) {
        for (i, m) in self.modes.iter_mut().enumerate() {
            m.y1 = 0.0;
            m.y2 = 0.0;
            // Known place after transport stop (the dynamo lesson).
            m.drift_ph = unit_f32(wang_hash(i as u32)) * core::f32::consts::TAU;
        }
    }

    /// Block-rate: derive every mode from the geometry.
    pub fn tune(
        &mut self,
        size: f32,
        stand: f32,
        distance: f32,
        t60_s: f32,
        bass: f32,
        block_s: f32,
    ) {
        let sc = Scene::describe_full(size, stand, distance, 1.0);
        let dims = sc.dims;
        for (i, ((nx, ny, nz), m)) in MODE_IDX.iter().zip(self.modes.iter_mut()).enumerate() {
            let (nx, ny, nz) = (f32::from(*nx), f32::from(*ny), f32::from(*nz));
            let f0 = 0.5
                * SPEED_M_S
                * ((nx / dims[0]).powi(2) + (ny / dims[1]).powi(2) + (nz / dims[2]).powi(2)).sqrt();
            // The drift: neighboring modes beat, and the beating
            // itself moves (rates 0.05-0.18 Hz, per-mode).
            let rate = 0.05 + 0.13 * unit_f32(wang_hash(i as u32 ^ 0xD01F));
            m.drift_ph =
                (m.drift_ph + core::f32::consts::TAU * rate * block_s) % core::f32::consts::TAU;
            let f = f0 * (1.0 + 0.004 * m.drift_ph.sin());
            // Band-edge fade (identity persists; audibility slides).
            let fade =
                ((BAND_HI - f) / 35.0).clamp(0.0, 1.0) * ((f - BAND_LO) / 10.0).clamp(0.0, 1.0);
            if fade <= 0.0 || f >= 0.45 * self.sr {
                m.k_a = 0.0;
                m.k_b = 0.0;
                continue;
            }
            // Position coupling at source AND listener: the mode
            // shape sampled where the machine speaks and where you
            // stand. A node silences it — that is the law.
            let shape = |n: f32, frac: f32| {
                if n == 0.0 {
                    1.0
                } else {
                    (n * core::f32::consts::PI * frac).cos()
                }
            };
            // The two stage sources sample the mode shape at their
            // own positions (±0.10 of the width — kept in sync
            // with room.rs SRC_SPREAD_F): a hard-panned bass note
            // excites what IT stands on.
            // From THE SCENE (one description — no more "kept in
            // sync" comments). The physics stands: a CENTERED
            // identical pair cannot excite odd-x modes at all
            // (antisymmetric — L+R cancels exactly); panned and
            // decorrelated material can. Stereo placement is a
            // modal timbre.
            let sf = |k: usize, ax: usize| sc.src_f[k][ax];
            let c_src_a = shape(nx, sf(0, 0)) * shape(ny, sf(0, 1)) * shape(nz, sf(0, 2));
            let c_src_b = shape(nx, sf(1, 0)) * shape(ny, sf(1, 1)) * shape(nz, sf(1, 2));
            let c_lis = shape(nx, sc.lis_f[0]) * shape(ny, sc.lis_f[1]) * shape(nz, sc.lis_f[2]);
            // Per-mode T60 spread: real neighbors decay wildly
            // differently — the low tail's timbre evolves.
            let spread = 0.6 + 0.75 * unit_f32(wang_hash(i as u32 ^ 0x7E60));
            let t60 = (t60_s * bass * spread).max(0.05);
            let r = t60_gain(1.0, t60 * self.sr);
            let w = core::f32::consts::TAU * f / self.sr;
            m.a1 = 2.0 * r * w.cos();
            m.a2 = -r * r;
            // DRIVEN-GAIN-FLAT: k = 2(1-r)·sin(w) puts the
            // resonance peak at exactly MODE_GAIN regardless of
            // frequency or decay. (Two measured dead ends: a 0.05
            // sin-floor muted the bank 15x; ring-amplitude-flat
            // k = sin(w) let sustained drive integrate to Qx —
            // peak 87. A high-Q mode under program charges to Q
            // times its band; the DRIVEN view is the stable one,
            // and impulses barely charging high-Q modes is honest
            // physics — the LF life lives on program material.)
            // (1 - r) is the TRANSIENT/broadband response: at
            // decay-min x bass-min it ran ~80x the default and
            // every attack slammed the limiter into harmonic
            // splatter (the field's "terrible distortion when
            // low"). Cap it at the ~1 s-decay value: short wide
            // modes honestly peak lower, and a kick charges a
            // dead room's modes no harder than a live one's.
            // 7e-5, not 1.5e-4: the cap creates a LOUDNESS HUMP —
            // capped-maximum below modal T60 ~1 s, falling as
            // 1/T60 above — which peaked exactly at MID bass and
            // stacked kick peaks into the limiter (the field's
            // third report: distortion from 10.5 to 2 o'clock,
            // drape worst — its 0.65 bass_mult shifts the hump
            // right and its damping strips the masking). Halving
            // the cap drops the hump under the limiter; held-note
            // bloom at long decays uses the uncapped branch and is
            // untouched.
            let k_t = (1.0 - r).min(7e-5);
            let base =
                fade * c_lis * 2.0 * k_t * w.sin() * MODE_GAIN * core::f32::consts::FRAC_1_SQRT_2;
            m.k_a = base * c_src_a;
            m.k_b = base * c_src_b;
        }
    }

    /// One sample: the source charges every mode; the sum is the
    /// room's low voice (mono — the field's lows fold anyway) —
    /// and LINEAR. The 0.10.1-era soft clip here had a knee at
    /// ~0.17 and ground every kick into harmonics: measured 0.36
    /// nonlinearity share at MID bass (the field's "10.5 to 2
    /// o'clock" distortion, drape worst), collapsing to 0.0006
    /// with the modes out of band. MODE_GAIN 5, the transient cap,
    /// and the post-makeup limiter+shave made it redundant — the
    /// THIRD protection stage this project has convicted as the
    /// distortion it claimed to prevent.
    #[inline]
    pub fn tick(&mut self, xl: f32, xr: f32) -> f32 {
        let mut out = 0.0f32;
        for m in self.modes.iter_mut() {
            let y = m.a1 * m.y1 + m.a2 * m.y2 + m.k_a * xl + m.k_b * xr;
            m.y2 = m.y1;
            m.y1 = y;
            out += y;
        }
        out
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// A mode with a node at the listener is SILENT there: nx=1 at
    /// the room's exact center has cos(π/2) = 0 pickup.
    #[test]
    fn a_node_silences_its_mode() {
        let mut m = Modes::new();
        m.prepare(48_000.0);
        // Size 0.5 puts (1,0,0) at ~46 Hz — inside the band (at
        // size 1 it sits at 23 Hz, below the 26 Hz floor).
        m.tune(0.5, 0.0, 0.7, 2.0, 1.0, 0.005); // stand 0 → lis_x_f = 0.5
                                                // Mode 0 is (1,0,0): listener coupling cos(π·0.5) ≈ 0.
        let silent = m.modes[0].k_a.abs().max(m.modes[0].k_b.abs());
        m.tune(0.5, 0.8, 0.7, 2.0, 1.0, 0.005);
        let speaking = m.modes[0].k_a.abs().max(m.modes[0].k_b.abs());
        assert!(
            speaking > 50.0 * silent.max(1e-12),
            "the node must silence the mode: {silent:.3e} vs {speaking:.3e}"
        );
    }

    /// The bank is passive: struck once it only decays.
    #[test]
    fn the_bank_decays() {
        let mut m = Modes::new();
        m.prepare(48_000.0);
        m.tune(1.0, 0.3, 0.7, 1.0, 1.0, 0.005);
        for _ in 0..100 {
            m.tick(0.5, 0.5);
        }
        let early: f32 = (0..24_000)
            .map(|_| m.tick(0.0, 0.0).abs())
            .fold(0.0, f32::max);
        let late: f32 = (0..24_000)
            .map(|_| m.tick(0.0, 0.0).abs())
            .fold(0.0, f32::max);
        assert!(late < early, "modes must decay: {early} → {late}");
    }
}
