//! `Space` — the GENERATIVE ROOM. A small VAE learned the library's ~700
//! real reverbs (halls, rooms, plates, springs, strange places); every
//! point of its 8-D latent decodes to a plausible room's ANATOMY:
//!
//! * early reflections — energy 0–120 ms in 5 ms bins, low/mid/high;
//! * decay — T60 per octave, 125 Hz … 8 kHz;
//! * tone — the reverb's energy per octave;
//! * density — the mixing time (echoes → a smooth tail);
//! * stereo — early and late L/R correlation.
//!
//! (The measurement's "predelay" — silence before the direct sound —
//! is the recording's padding, not the room: its gap before the first
//! reflection is in the early pattern. It is decoded but not used.)
//!
//! The MAP lays the real spaces out in 2-D; a cursor on it is a kernel
//! blend of the nearby spaces' latents, so between two dots is a room
//! in between them. Memories also carry the latent they were heard in
//! (bank.rs), and recall pulls the room toward the remembered spaces.
//!
//! Decoding is ≈7 k multiply-adds: once a block, never per frame.

use super::space_model::*;

pub(super) const ER_BANDS: usize = 3;
pub(super) const ER_BINS: usize = 24;
pub(super) const ER_BIN_S: f32 = 0.005;
pub(super) const OCTAVES: usize = 7;
pub(super) type Latent = [f32; LATENT];

/// Feature layout (train/ir_measure.py).
const EARLY: usize = 0;
const DECAY: usize = EARLY + ER_BANDS * ER_BINS;
const TONE: usize = DECAY + OCTAVES;
const DENSITY: usize = TONE + OCTAVES;
const STEREO: usize = DENSITY + 2 + 12;
const PREDELAY: usize = STEREO + 3;
const _: () = assert!(PREDELAY + 1 == FEATURES);

/// A room, in the engine's terms.
#[derive(Copy, Clone, Debug)]
pub(super) struct Anatomy {
    /// Early energy per bin per band, as linear shares of the band's
    /// reverb energy (the direct sound not counted; bin 0 is empty).
    /// A band's bins sum to its reflections' share; the late tail is
    /// the rest.
    pub(super) early: [[f32; ER_BINS]; ER_BANDS],
    /// T60 per octave (125 Hz … 8 kHz), seconds.
    pub(super) t60: [f32; OCTAVES],
    /// Energy per octave, dB, mean zero.
    pub(super) tone_db: [f32; OCTAVES],
    /// Seconds until the echoes blur into a tail.
    #[cfg_attr(not(test), allow(dead_code))] // read by tests; the engine doesn't model mixing time yet
    pub(super) mixing_s: f32,
    /// L/R correlation, early and late (1 = mono, 0 = wide).
    pub(super) early_corr: f32,
    pub(super) late_corr: f32,
}

/// The raw decoder: latent → the standardised-then-restored features.
pub(super) fn features(z: &Latent) -> [f32; FEATURES] {
    let mut g = [0.0f32; HIDDEN];
    for (j, gj) in g.iter_mut().enumerate() {
        let mut s = B4[j];
        for (k, zk) in z.iter().enumerate() {
            s += zk * W4[k * HIDDEN + j];
        }
        *gj = s.tanh();
    }
    let mut y = [0.0f32; FEATURES];
    for (i, yi) in y.iter_mut().enumerate() {
        let mut s = B5[i];
        for (j, gj) in g.iter().enumerate() {
            s += gj * W5[j * FEATURES + i];
        }
        *yi = s * STD[i] + MEAN[i];
    }
    y
}

/// The decoder's error at nearby real rooms, added back: weights by a
/// Gaussian kernel in the latent (MEMORY_WIDTH of the rooms' spacing) —
/// at a real room its residual counts fully, between rooms they blend,
/// and far from every room the pure decoder speaks. ≈700 × 8 madds +
/// exps, then ≈104 per room that matters: once a block.
const MEMORY_WIDTH: f32 = 0.35;

fn remembered(z: &Latent, y: &mut [f32; FEATURES]) {
    // 0.35 × the rooms' spacing: a room's nearest neighbour weighs ≈ 2%
    // of it at its own latent.
    let s = MEMORY_WIDTH * LATENT_SIGMA;
    let k = -0.5 / (s * s);
    let mut d2 = [0.0f32; SPACES];
    let mut near = f32::MAX;
    for (i, d) in d2.iter_mut().enumerate() {
        *d = (0..LATENT).map(|j| (MAP_LATENT[i * LATENT + j] - z[j]).powi(2)).sum();
        near = near.min(*d);
    }
    // Weights RELATIVE to the nearest room (it always dominates; twins —
    // the same hall in two collections — share), summing to one; then a
    // fade by the distance to that nearest room, so far from all rooms
    // (a DRIFT into new territory) the pure decoder speaks.
    let mut w = [0.0f32; SPACES];
    let mut wsum = 0.0f32;
    for (wi, &d) in w.iter_mut().zip(&d2) {
        *wi = (k * (d - near)).exp();
        wsum += *wi;
    }
    let fade = (k * near).exp() / wsum;
    for (i, &wi) in w.iter().enumerate() {
        let g = wi * fade;
        if g < 1e-4 {
            continue;
        }
        let r = &RESIDUAL[i * FEATURES..(i + 1) * FEATURES];
        for (yv, rv) in y.iter_mut().zip(r) {
            *yv += g * rv;
        }
    }
}

pub(super) fn decode(z: &Latent) -> Anatomy {
    let mut y = features(z);
    remembered(z, &mut y);
    let mut early = [[0.0; ER_BINS]; ER_BANDS];
    for (b, band) in early.iter_mut().enumerate() {
        let o = EARLY + b * ER_BINS;
        // Slot 0: the reflections' share (logit); 1..: their pattern
        // over bins 0..22 (0–115 ms), log10(share + 1e-4), renormalised.
        let frac = 1.0 / (1.0 + (-y[o]).exp());
        for k in 0..ER_BINS - 1 {
            band[k] = (10f32.powf(y[o + 1 + k]) - 1e-4).max(0.0);
        }
        let sum: f32 = band.iter().sum();
        let k = if sum > 0.0 { frac / sum } else { 0.0 };
        band.iter_mut().for_each(|e| *e *= k);
    }
    let mean_tone = y[TONE..TONE + OCTAVES].iter().sum::<f32>() / OCTAVES as f32;
    Anatomy {
        early,
        // A room that "never decays" in the data reads 60 s.
        t60: core::array::from_fn(|k| 10f32.powf(y[DECAY + k]).clamp(0.05, 60.0)),
        tone_db: core::array::from_fn(|k| 10.0 * (y[TONE + k] - mean_tone)),
        mixing_s: 10f32.powf(y[DENSITY]).clamp(0.001, 0.12),
        early_corr: y[STEREO].clamp(-1.0, 1.0),
        late_corr: y[STEREO + 1].clamp(-1.0, 1.0),
    }
}

/// The map's kernel width (map units): a third of the dots' spacing, so
/// on a dot you get that room (measured: ≈ its own latent's fidelity),
/// and between dots a blend of the two or three nearest.
const KERNEL: f32 = 0.010;

/// The latent at a point on the map: the real spaces near it, blended
/// by a Gaussian kernel (≈700 exps — once a block, and only on change).
pub(super) fn at_map(x: f32, y: f32) -> Latent {
    let mut z = [0.0f32; LATENT];
    let mut wsum = 0.0f32;
    let k = -0.5 / (KERNEL * KERNEL);
    let d2 = |i: usize| (MAP_XY[2 * i] - x).powi(2) + (MAP_XY[2 * i + 1] - y).powi(2);
    // Relative to the nearest dot: far from every dot, still its room.
    let near = (0..SPACES).map(d2).fold(f32::MAX, f32::min);
    for i in 0..SPACES {
        let w = (k * (d2(i) - near)).exp();
        wsum += w;
        for (j, zj) in z.iter_mut().enumerate() {
            *zj += w * MAP_LATENT[i * LATENT + j];
        }
    }
    if wsum > 1e-20 {
        z.iter_mut().for_each(|v| *v /= wsum);
    }
    z
}

/// Where a latent sits ON THE MAP — the inverse of `at_map`, for the
/// face: the real spaces near it in the latent, their map positions
/// blended by a Gaussian kernel (LATENT_SIGMA ≈ the spacing of real
/// spaces). Also the nearest real space (for the display's name).
/// ≈700 × 8 multiply-adds and exps: once a block.
pub(super) fn to_map(z: &Latent) -> ([f32; 2], usize) {
    let d2 = |i: usize| (0..LATENT).map(|k| (MAP_LATENT[i * LATENT + k] - z[k]).powi(2)).sum::<f32>();
    let (mut near, mut near_d) = (0, f32::MAX);
    for i in 0..SPACES {
        let d = d2(i);
        if d < near_d {
            (near, near_d) = (i, d);
        }
    }
    // Half the spacing: a room sits on its own dot, and between rooms
    // it moves smoothly (the map holds a third of the latent's spread,
    // so latent neighbours can be far apart on it).
    let s = 0.5 * LATENT_SIGMA;
    let k = -0.5 / (s * s);
    let (mut xy, mut wsum) = ([0.0f32; 2], 0.0f32);
    for i in 0..SPACES {
        let w = (k * (d2(i) - near_d)).exp();
        xy[0] += w * MAP_XY[2 * i];
        xy[1] += w * MAP_XY[2 * i + 1];
        wsum += w;
    }
    ([xy[0] / wsum, xy[1] / wsum], near)
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    /// With the memory, a real room's own latent decodes to (nearly) its
    /// own measured anatomy — e.g. its T60s within 5%, its reflections'
    /// share within 0.03.
    #[test]
    fn a_real_room_decodes_to_itself() {
        let (mut errs, mut fracs) = (alloc::vec::Vec::new(), alloc::vec::Vec::new());
        let frac = |v: f32| 1.0 / (1.0 + (-v).exp());
        for i in 0..SPACES {
            let own: Latent = core::array::from_fn(|k| MAP_LATENT[i * LATENT + k]);
            let mut y = features(&own);
            remembered(&own, &mut y);
            let real: [f32; FEATURES] = core::array::from_fn(|f| features(&own)[f] + RESIDUAL[i * FEATURES + f]);
            for k in DECAY..DECAY + OCTAVES {
                errs.push((y[k] - real[k]).abs() * core::f32::consts::LN_10);
            }
            fracs.push((frac(y[EARLY + ER_BINS]) - frac(real[EARLY + ER_BINS])).abs());
        }
        errs.sort_by(f32::total_cmp);
        fracs.sort_by(f32::total_cmp);
        // Twins (one hall in two collections) share a latent and blend:
        // judged by the median room, and the 90th percentile.
        let (m, p90) = (errs[errs.len() / 2], errs[errs.len() * 9 / 10]);
        assert!(m < 0.02 && p90 < 0.10, "|ln T60 ratio| median {m:.3}, p90 {p90:.3}");
        assert!(fracs[fracs.len() / 2] < 0.01, "early share median error {:.3}", fracs[fracs.len() / 2]);
    }

    /// A real space's own latent lands near its own dot.
    #[test]
    fn a_room_finds_itself_on_the_map() {
        let mut err = 0.0;
        let mut found = 0;
        for i in (0..SPACES).step_by(17) {
            let own: Latent = core::array::from_fn(|k| MAP_LATENT[i * LATENT + k]);
            let ([x, y], near) = to_map(&own);
            err += (x - MAP_XY[2 * i]).hypot(y - MAP_XY[2 * i + 1]);
            found += usize::from(near == i);
        }
        let n = SPACES.div_ceil(17);
        assert_eq!(found, n, "each latent's nearest space is itself");
        assert!(err / (n as f32) < 0.03, "mean map error {:.3}", err / n as f32);
    }

    #[test]
    fn the_decoder_matches_the_trained_model() {
        for g in 0..3 {
            let z: Latent = core::array::from_fn(|k| GOLDEN_Z[g * LATENT + k]);
            let y = features(&z);
            for i in 0..FEATURES {
                let want = GOLDEN_FEATURES[g * FEATURES + i];
                assert!((y[i] - want).abs() <= 1e-3 * (1.0 + want.abs()), "golden {g} feature {i}: {} vs {want}", y[i]);
            }
        }
    }

    #[test]
    fn every_point_of_the_map_is_a_sane_room() {
        for i in 0..=20 {
            for j in 0..=20 {
                let a = decode(&at_map(i as f32 / 20.0, j as f32 / 20.0));
                assert!(a.t60.iter().all(|t| t.is_finite() && *t >= 0.05));
                assert!(a.early.iter().flatten().all(|e| e.is_finite() && *e >= 0.0 && *e <= 1.0));
                assert!(a.mixing_s.is_finite());
            }
        }
    }

    /// On a real space's dot, the map gives back (about) that room.
    #[test]
    fn a_dot_on_the_map_is_its_room() {
        let mut err = 0.0;
        for i in (0..SPACES).step_by(37) {
            let z = at_map(MAP_XY[2 * i], MAP_XY[2 * i + 1]);
            let own: Latent = core::array::from_fn(|k| MAP_LATENT[i * LATENT + k]);
            let (a, b) = (decode(&z), decode(&own));
            err += (a.t60[3] / b.t60[3]).ln().abs();
        }
        let n = SPACES.div_ceil(37) as f32;
        assert!(err / n < 0.1, "mean |ln T60 ratio| at the dots {:.2}", err / n);
    }
}
