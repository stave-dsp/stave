//! THE ROOM — the fidelity half: a live image-source model of a
//! real (or impossible) geometry. Nothing here is a tuned tap
//! table: a shoebox is DESCRIBED (dimensions from SIZE, source and
//! listener as fractions of it, two ears at their true positions
//! 18 cm apart), and every early reflection is DERIVED — its
//! fractional-sample delay from the path length, its level from
//! distance and per-bounce wall absorption, its darkness from
//! bounce count and head shadow. The interaural cues are not
//! synthesized: each ear simply lives where it lives, so ITD and
//! ILD fall out of the geometry and can never disagree with SIZE.
//!
//! Fidelity choices with reasons:
//! * delays are the ITDG (initial-time-delay-gap) convention —
//!   path length MINUS the direct path per ear, because the dry
//!   signal is the direct sound and arrives at t=0;
//! * a reflection path is never shorter than the direct path
//!   (mirror geometry guarantees it), so gaps are non-negative by
//!   construction;
//! * per-tap one-poles carry wall darkening (bounce count) and a
//!   crude head shadow (contralateral arrivals dark) — 48 filter
//!   states, prepare-free;
//! * tap delays GLIDE under the shared slew doctrine (SIZE turns
//!   bend the room like tape); a fresh room SNAPS (the first tune
//!   after prepare — an onset must not crawl outward).

use stave_dsp::primitives::random::{unit_f32, wang_hash};
use stave_dsp::DelayLine;

use super::scene::{Scene, SPEED_M_S};
/// Order ≤ 2 shoebox images (6 first-order + 18 second-order).
pub(super) const IMAGES: usize = 24;
/// TRUE STEREO: the L and R inputs are TWO SOURCES standing apart
/// on the same stage (review catch: folding to mono at the door
/// discarded the input's spatial field — antiphase input produced
/// NO wet at all). 2 sources x 2 ears x 24 images. ALL geometry
/// comes from the SCENE — this file owns no positions.
const SOURCES: usize = 2;
/// Each source carries its channel at -3 dB — dual-driven mono
/// sums to unity ENERGY across the incoherent tap set.
const SRC_LEVEL: f32 = core::f32::consts::FRAC_1_SQRT_2;
const TAPS: usize = IMAGES * 2 * SOURCES;
const MAX_SLEW: f32 = 0.02;

/// PER-SURFACE materials (the fidelity is in the differences):
/// the FLOOR is nearly swallowed — audience and seating absorb the
/// grazing bounce (this is also why the floor tap must not carry
/// the size cue: your head height never scales with the hall);
/// the CEILING is plaster; the WALLS carry the lateral energy that
/// IS the sense of size. (gain kept per bounce, cutoff multiplier).
const FLOOR: (f32, f32) = (0.35, 0.45);
const CEILING: (f32, f32) = (0.75, 0.80);
const ER_GAIN: f32 = 0.30;
/// SPLAY: no two real walls are parallel (halls splay them for
/// exactly this reason) — each image is perturbed by a stable
/// per-image fraction of the dims, breaking the lattice's
/// periodicity (the flutter-echo signature).
const SPLAY: f32 = 0.045;
/// SCATTERING: a wall is rough and finite, so a reflection is a
/// SMEAR, not a Dirac. The first implementation smeared PER TAP —
/// four interpolated reads each, 192 reads/sample, and the field
/// paid 5x CPU for it (1263 ns/frame measured). Scattering is a
/// TEXTURE, not a position: two short allpasses per ear smear
/// every tap for ~2% of that cost. The ears share ONE tuning —
/// the detuned-per-ear first draft decorrelated everything and
/// the scoreboard read IACC_early 0.2 where real rooms sit at
/// 0.7+ (scatter off the same wall reaches both ears nearly
/// identically; the GEOMETRY's ITD/ILD is the honest interaural
/// difference, and real dimension is a coherent early field
/// opening into a decorrelated late one).
const SMEAR_MS: [f32; 2] = [1.31, 3.43];
const SMEAR_G: f32 = 0.55;

/// ELEVATION PAINTING: the pinna hears "above" as energy toward
/// 7-10 kHz (Blauert's directional bands). Ceiling-touched images
/// pass a shared +3 dB height shelf; the floor stays swallowed —
/// the room paints its vertical axis into the spectrum.
const ELEV_XOVER_HZ: f32 = 6_500.0;
const ELEV_GAIN: f32 = 0.41;

/// The stochastic order-3+ set: 16 arrivals per ear. Level scale
/// keeps the set a THICKENING of the skeleton, not a wash over it.
const HTAPS: usize = 32;
const HTAP_LEVEL: f32 = 0.7;

struct Tap {
    delay: f32,
    step: f32,
    gain: f32,
    /// Per-sample gain glide (worklist 7): delays always glided,
    /// gains stepped per block — the measured zipper.
    gain_step: f32,
    lp_k: f32,
    lp_z: f32,
    /// Share of this tap in the LATE-FIELD FEED (second-order
    /// bounces only — the diffuse field grows from higher-order
    /// energy, so the late onset trails the early field and
    /// scales with the room).
    feed_w: f32,
    /// ELEVATION PAINTING (worklist 4): images that touched the
    /// CEILING route through the shared height shelf (+3 dB toward
    /// the Blauert "above" band); floor arrivals stay dark. The
    /// vertical axis becomes a spectral painting.
    elev: bool,
    /// Floor-touched (the swallowed bounce — the law's contrast).
    floor: bool,
}

/// One smear stage (a tiny fixed allpass).
struct Smear {
    delay: DelayLine,
    len: f32,
}

pub(super) struct Room {
    /// Per-source input history (source 0 = L, source 1 = R).
    input: [DelayLine; 2],
    /// The field's lateral balance, from the geometry: positive =
    /// the source sits to the listener's LEFT, so the wet field
    /// leans left. Per-tap ILD alone measured ~1.06 net balance —
    /// near-wall energy and source-direction energy structurally
    /// cancel in RMS; the source direction is the cue that
    /// survives, and the DIRECT sound (which carries most of it in
    /// reality) belongs to the untouchable dry.
    balance: f32,
    /// Left ear taps 0..IMAGES, right ear taps IMAGES..
    taps: [Tap; TAPS],
    /// The scattering texture: [L1, L2, R1, R2].
    smear: [Smear; 4],
    /// CATHEDRAL HIGHER-ORDER TAPS (worklist 6): a stochastic
    /// order-3+ set per ear. The exact image lattice stops at
    /// order 2; beyond it the arrivals are statistically dense
    /// but individually meaningless — so their delays are HASHED
    /// into [max order-2 gap, +3 mean free paths], wearing
    /// CUMULATIVE absorption (wall^order) and true 1/r. Size-
    /// gated: small rooms' order-2 field is already dense.
    htaps: [Tap; HTAPS],
    /// Zero when the size gate is closed — the tick skips.
    h_on: bool,
    /// The height shelf's LP state per ear (shared across all
    /// ceiling taps — texture is per-band, not per-tap).
    elev_lp: [f32; 2],
    k_elev: f32,
    sr: f32,
    /// Fresh room: the next tune SNAPS instead of gliding.
    cold: bool,
}

impl Room {
    pub fn new() -> Self {
        Self {
            input: [DelayLine::new(), DelayLine::new()],
            taps: core::array::from_fn(|_| Tap {
                delay: 1.5,
                step: 0.0,
                gain: 0.0,
                gain_step: 0.0,
                lp_k: 1.0,
                lp_z: 0.0,
                feed_w: 0.0,
                elev: false,
                floor: false,
            }),
            smear: core::array::from_fn(|_| Smear {
                delay: DelayLine::new(),
                len: 48.0,
            }),
            htaps: core::array::from_fn(|_| Tap {
                delay: 1.5,
                step: 0.0,
                gain: 0.0,
                gain_step: 0.0,
                lp_k: 1.0,
                lp_z: 0.0,
                feed_w: 0.0,
                elev: false,
                floor: false,
            }),
            h_on: false,
            elev_lp: [0.0; 2],
            k_elev: 0.3,
            balance: 0.0,
            sr: 48_000.0,
            cold: true,
        }
    }

    /// Prepare-time only: capacity for the farthest order-2 image
    /// at size 3.0 (~0.33 s of path gap) plus the stochastic
    /// order-3+ window (+3 mean free paths).
    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.input[0].prepare((0.52 * sr) as usize + 64);
        self.input[1].prepare((0.52 * sr) as usize + 64);
        for (i, sm) in self.smear.iter_mut().enumerate() {
            // Integer lengths: fixed allpasses skip interp math.
            sm.len = (SMEAR_MS[i % 2] * 0.001 * sr).round();
            sm.delay.prepare(sm.len as usize + 8);
        }
        self.k_elev = 1.0 - (-core::f32::consts::TAU * ELEV_XOVER_HZ / sr).exp();
        self.reset();
        self.cold = true;
    }

    /// The geometry's lateral lean (see `balance` field).
    pub fn balance(&self) -> f32 {
        self.balance
    }

    pub fn reset(&mut self) {
        self.input[0].reset();
        self.input[1].reset();
        for t in self.taps.iter_mut() {
            t.lp_z = 0.0;
            t.step = 0.0;
        }
        for t in self.htaps.iter_mut() {
            t.lp_z = 0.0;
            t.step = 0.0;
        }
        for sm in self.smear.iter_mut() {
            sm.delay.reset();
        }
        self.elev_lp = [0.0; 2];
    }

    /// Block-rate: rebuild the geometry and aim every tap.
    /// `stand` is the listener's lateral position (±1 walks toward
    /// the side walls); `tone` scales the MATERIAL cutoffs (the
    /// walls brighten or drape — never a post-EQ).
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        size: f32,
        stand: f32,
        distance: f32,
        ear_scale: f32,
        tone: f32,
        wall: (f32, f32),
        launch: f32,
        frames: usize,
    ) {
        let sc = Scene::describe_full(size, stand, distance, ear_scale);
        let dims = sc.dims;
        // Larger x is the RIGHT ear's side: listener right of the
        // stage center means the field leans LEFT toward it
        // (positive balance). The SCENE owns the sign.
        self.balance = sc.balance();
        let tone_mult = 2f32.powf(tone.clamp(-1.0, 1.0) * 1.6);
        let ears = sc.ears();
        // Per-axis 1-D mirror images of the source: (position,
        // bounce count, gain factor, cutoff factor) for up to two
        // bounces, each bounce wearing ITS surface's material.
        let axis = |s: f32, len: f32, near: (f32, f32), far: (f32, f32)| {
            [
                (s, 0u8, 1.0f32, 1.0f32),
                (-s, 1, near.0, near.1),
                (2.0 * len - s, 1, far.0, far.1),
                (2.0 * len + s, 2, near.0 * far.0, near.1 * far.1),
                (-2.0 * len + s, 2, near.0 * far.0, near.1 * far.1),
            ]
        };
        let n = frames.max(1) as f32;
        let slew = MAX_SLEW * n;
        let mut idx = 0usize;
        for si in 0..SOURCES {
            let src = sc.source(si);
            let xs = axis(src[0], dims[0], wall, wall);
            let ys = axis(src[1], dims[1], wall, wall);
            let zs = axis(src[2], dims[2], FLOOR, CEILING);
            for (ex, ear) in ears.iter().enumerate() {
                let r0 = dist(&src, ear);
                // Outward direction of this ear (head shadow's axis).
                let out_x = if ex == 0 { -1.0f32 } else { 1.0 };
                for (xi, (px, bx, gx, cx)) in xs.iter().copied().enumerate() {
                    for (yi, (py, by, gy, cy)) in ys.iter().copied().enumerate() {
                        for (zi, (pz, bz, gz, cz)) in zs.iter().copied().enumerate() {
                            let b = bx + by + bz;
                            if b == 0 || b > 2 {
                                continue;
                            }
                            // Splay is keyed by the IMAGE (same walls
                            // for both ears), never the ear.
                            let key = (xi * 25 + yi * 5 + zi) as u32;
                            let img = [
                                px + (unit_f32(wang_hash(key)) - 0.5) * 2.0 * SPLAY * dims[0],
                                py + (unit_f32(wang_hash(key.wrapping_add(97))) - 0.5)
                                    * 2.0
                                    * SPLAY
                                    * dims[1],
                                pz + (unit_f32(wang_hash(key.wrapping_add(193))) - 0.5)
                                    * 2.0
                                    * SPLAY
                                    * dims[2],
                            ];
                            let r = dist(&img, ear);
                            let gap_s = ((r - r0) / SPEED_M_S).max(0.0);
                            let target = gap_s * self.sr + 1.5;
                            let gain = (r0 / r.max(0.1)) * gx * gy * gz;
                            // Head shadow: arrivals from this ear's side
                            // stay bright; contralateral ones darken —
                            // in CUTOFF and in GAIN (the ILD): ears sit
                            // 18 cm apart while STAND moves meters, so
                            // without explicit level shadow the balance
                            // barely tilted (1.05 measured at the wall).
                            let side = (img[0] - ear[0]) * out_x / r.max(0.1);
                            let shade = 0.55 + 0.45 * 0.5 * (1.0 + side);
                            // LAUNCH: ILD sharpened past nature as a
                            // ZERO-NET tilt on GAIN ONLY — ipsilateral
                            // up as much as contralateral goes down,
                            // and the CUTOFF keeps the natural shade
                            // (the first cut scaled both down: the
                            // field heard "lowpass + quieter", which
                            // is exactly what it was).
                            // Energy-TRUE deep tilt: (1+x)/sqrt(1+x^2)
                            // keeps every symmetric tap pair at exactly
                            // unit energy at ANY depth (the raw 1+x
                            // form grows as 2+2x^2 — measured +2.5 dB),
                            // and the cap stops short of the polarity
                            // flip (antiphase contralateral taps were
                            // the coherence kill: IACC 0.14).
                            let x = (1.5 * launch * side).clamp(-0.93, 0.93);
                            let ild = (1.0 + x) / (1.0 + x * x).sqrt();
                            let gain = gain * shade * ild;
                            // AIR: HF dies with the PATH — a triple-
                            // bounce 40 m arrival is dull, the near
                            // wall bright. Front-to-back geography
                            // inside the early field (was bounce-count
                            // only: no distance gradient).
                            let air = 1.0 / (1.0 + r / 45.0);
                            let cutoff = (15_500.0
                                * cx
                                * cy
                                * cz
                                * shade
                                * tone_mult
                                * (0.35 + 0.65 * air * 2.0).min(1.0))
                            .min(19_000.0);
                            let tap = &mut self.taps[idx];
                            tap.elev = zi == 2;
                            tap.floor = zi == 1;
                            tap.feed_w = if b == 2 { 1.0 } else { 0.0 };
                            tap.lp_k = 1.0 - (-core::f32::consts::TAU * cutoff / self.sr).exp();
                            if self.cold {
                                tap.gain = gain;
                                tap.gain_step = 0.0;
                                tap.delay = target;
                                tap.step = 0.0;
                            } else {
                                tap.gain_step = (gain - tap.gain) / n;
                                tap.step = (target - tap.delay).clamp(-slew, slew) / n;
                            }
                            idx += 1;
                        }
                    }
                }
            }
        }
        debug_assert_eq!(idx, TAPS);
        // THE ORDER-3+ FIELD: delays hashed into [max order-2
        // gap, +3 mfp] — the statistical continuation of the
        // lattice. Gains wear wall^order and true 1/r; the gate
        // opens with SIZE (order-2 truncation only starves big
        // rooms — the mid-field lesson, specular edition).
        let h_lvl = ((size - 1.5) / 0.7).clamp(0.0, 1.0) * HTAP_LEVEL;
        self.h_on = h_lvl > 0.0;
        let max_gap_s = self.taps.iter().map(|t| t.delay).fold(0.0f32, f32::max) / self.sr;
        let mfp_s = sc.mean_free_path_s();
        let cap_s = 0.5;
        for (hi, tap) in self.htaps.iter_mut().enumerate() {
            let ear = hi / (HTAPS / 2);
            let key = 7_000 + hi as u32;
            // The sketch said [max order-2, +3 mfp]; measured,
            // that region's 1/r gains are ~0.06 and the tank
            // already owns it (NED/flutter unmoved). The STARVED
            // region is the MIDDLE of the early field — where 24
            // images are countable and the tank is still building.
            let gap_s = (0.25 * max_gap_s
                + unit_f32(wang_hash(key)) * (0.75 * max_gap_s + 1.5 * mfp_s))
                .min(cap_s);
            let order = 3 + (unit_f32(wang_hash(key.wrapping_add(31))) * 1.99) as i32;
            let r0 = dist(&sc.source(hi % 2), &ears[ear]);
            let r = r0 + gap_s * SPEED_M_S;
            let gain = (r0 / r.max(0.1)) * wall.0.powi(order) * h_lvl;
            let air = 1.0 / (1.0 + r / 45.0);
            let cutoff =
                (15_500.0 * wall.1.powi(order) * tone_mult * (0.35 + 0.65 * air * 2.0).min(1.0))
                    .min(19_000.0);
            let target = gap_s * self.sr + 1.5;
            tap.lp_k = 1.0 - (-core::f32::consts::TAU * cutoff / self.sr).exp();
            if self.cold {
                tap.gain = gain;
                tap.gain_step = 0.0;
                tap.delay = target;
                tap.step = 0.0;
            } else {
                tap.gain_step = (gain - tap.gain) / n;
                tap.step = (target - tap.delay).clamp(-slew, slew) / n;
            }
        }
        self.cold = false;
    }

    /// One sample: the source speaks, both ears listen to every
    /// image (read before write). Returns (er_l, er_r, feed_l,
    /// feed_r) — the wet early field, and the second-order-only
    /// late-field feed. PER-SAMPLE ON PURPOSE — two block-major
    /// rewrites measured SLOWER (1113 and 1021 vs 866 ns/frame
    /// total): the inlined scalar reads were already optimal, and the
    /// block machinery's buffer traffic cost more than the loop order
    /// saved.
    #[inline]
    pub fn tick(&mut self, xl: f32, xr: f32) -> (f32, f32, f32, f32) {
        let mut l = 0.0f32;
        let mut r = 0.0f32;
        let mut el = 0.0f32;
        let mut er = 0.0f32;
        let mut fl = 0.0f32;
        let mut fr = 0.0f32;
        // RUN-STRUCTURED (the CPU slice): the tap array is four
        // contiguous runs of IMAGES — [src0 L][src0 R][src1 L]
        // [src1 R] — so the input line, the ear and the
        // accumulators hoist OUT of the per-tap work; the inner
        // loop is pure stream arithmetic (the per-tap `i/IMAGES`
        // divides and branchy routing measured ~270 ns/frame).
        for (run, taps) in self.taps.chunks_exact_mut(IMAGES).enumerate() {
            let input = &self.input[run / 2];
            let mut main = 0.0f32;
            let mut ele = 0.0f32;
            let mut feed = 0.0f32;
            for tap in taps {
                // Linear read: an ER tap is heard ONCE (no
                // recirculation to compound interpolation loss)
                // and feeds a one-pole.
                let s = input.read(tap.delay) * tap.gain;
                tap.delay += tap.step;
                tap.gain += tap.gain_step;
                tap.lp_z += tap.lp_k * (s - tap.lp_z);
                if tap.elev {
                    ele += tap.lp_z;
                } else {
                    main += tap.lp_z;
                }
                feed += tap.lp_z * tap.feed_w;
            }
            if run % 2 == 0 {
                l += main;
                el += ele;
                fl += feed;
            } else {
                r += main;
                er += ele;
                fr += feed;
            }
        }
        if self.h_on {
            for (hi, tap) in self.htaps.iter_mut().enumerate() {
                let x = self.input[hi % 2].read(tap.delay) * tap.gain;
                tap.delay += tap.step;
                tap.gain += tap.gain_step;
                tap.lp_z += tap.lp_k * (x - tap.lp_z);
                if hi < HTAPS / 2 {
                    l += tap.lp_z;
                } else {
                    r += tap.lp_z;
                }
            }
        }
        // The height shelf: ceiling arrivals wear the Blauert
        // "above" boost (+3 dB past ~6.5 kHz, complement-of-LP —
        // same minimum-phase family as every other shelf here).
        self.elev_lp[0] += self.k_elev * (el - self.elev_lp[0]);
        let l = l + el + ELEV_GAIN * (el - self.elev_lp[0]);
        self.elev_lp[1] += self.k_elev * (er - self.elev_lp[1]);
        let r = r + er + ELEV_GAIN * (er - self.elev_lp[1]);
        self.input[0].write(xl * SRC_LEVEL);
        self.input[0].advance();
        self.input[1].write(xr * SRC_LEVEL);
        self.input[1].advance();
        // The scattering texture: two tiny allpasses per ear turn
        // the specular taps into smears (integer lengths — no
        // interp math on fixed structures).
        let mut l = l * ER_GAIN;
        let mut r = r * ER_GAIN;
        for (k, sm) in self.smear.iter_mut().enumerate() {
            let x = if k < 2 { l } else { r };
            let dd = sm.delay.read_int(sm.len as usize);
            let y = dd - SMEAR_G * x;
            sm.delay.write(x + SMEAR_G * y);
            sm.delay.advance();
            if k < 2 {
                l = y;
            } else {
                r = y;
            }
        }
        (l, r, fl * ER_GAIN, fr * ER_GAIN)
    }
}

#[inline]
fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod properties {
    use super::*;

    /// Geometry, not a tap table: every reflection path is at least
    /// the direct path (the ITDG gaps are non-negative), and the
    /// enumeration fills exactly the declared image count.
    #[test]
    fn the_room_is_consistent_geometry() {
        let mut room = Room::new();
        room.prepare(48_000.0);
        room.tune(1.0, 0.0, 0.7, 1.0, 0.0, (0.85, 0.90), 0.0, 256);
        for (i, t) in room.taps.iter().enumerate() {
            assert!(t.delay >= 1.5, "tap {i} arrived before the direct sound");
            assert!(t.gain > 0.0 && t.gain <= 1.0, "tap {i} gain {}", t.gain);
        }
        assert_eq!(room.taps.len(), 96, "2 sources x 2 ears x 24 images");
    }

    /// CATHEDRAL HIGHER-ORDER TAPS (worklist 6): at size 2.7 the
    /// stochastic order-3+ set EXTENDS THE SPECULAR SKELETON —
    /// +22% ER energy measured in the starved 60-180 ms window.
    /// (The sketch's acceptance — full-render NED@25 up, flutter
    /// down — measured UNMOVABLE by taps: the midfield already
    /// owns statistical density there, and cathedral flutter is
    /// the TANK's line periodicity (tank-only 0.656 vs full
    /// 0.630); its cure stays mode density, on the roadmap.)
    #[test]
    fn the_higher_orders_extend_the_skeleton() {
        let window_energy = |kill: bool| -> f32 {
            let mut room = Room::new();
            room.prepare(48_000.0);
            room.tune(2.7, 0.0, 0.7, 1.0, 0.0, (0.85, 0.90), 0.0, 256);
            if kill {
                for t in room.htaps.iter_mut() {
                    t.gain = 0.0;
                }
            }
            let mut e = 0.0f32;
            for i in 0..9_600 {
                let x = if i == 0 { 1.0 } else { 0.0 };
                let (l, r, _, _) = room.tick(x, x);
                if i >= 2_880 {
                    e += l * l + r * r;
                }
            }
            e
        };
        let with = window_energy(false);
        let without = window_energy(true);
        assert!(
            with > 1.15 * without,
            "the skeleton must extend: {with:.5} vs {without:.5}"
        );
        // The gate: a small room's order-2 field is already dense —
        // the set stays silent (and the tick skips it entirely).
        let mut small = Room::new();
        small.prepare(48_000.0);
        small.tune(1.0, 0.0, 0.7, 1.0, 0.0, (0.85, 0.90), 0.0, 256);
        assert!(!small.h_on, "the gate must hold at size 1");
        assert!(
            small.htaps.iter().all(|t| t.gain == 0.0),
            "small rooms get no phantom orders"
        );
    }

    /// ELEVATION PAINTING's acceptance: the ceiling's contribution
    /// is BRIGHTER than the floor's (render-difference isolated —
    /// zero one set of taps, subtract from the full render, compare
    /// spectral centroids of what each set alone contributed).
    #[test]
    fn the_ceiling_shines_down() {
        let render = |kill: Option<bool>| -> Vec<f32> {
            let mut room = Room::new();
            room.prepare(48_000.0);
            room.tune(1.0, 0.0, 0.7, 1.0, 0.0, (0.85, 0.90), 0.0, 256);
            if let Some(ceiling) = kill {
                for t in room.taps.iter_mut() {
                    if (ceiling && t.elev) || (!ceiling && t.floor) {
                        t.gain = 0.0;
                    }
                }
            }
            let mut out = Vec::with_capacity(4_800);
            for i in 0..4_800 {
                let x = if i == 0 { 1.0 } else { 0.0 };
                let (l, _, _, _) = room.tick(x, x);
                out.push(l);
            }
            out
        };
        let full = render(None);
        let no_ceil = render(Some(true));
        let no_floor = render(Some(false));
        let centroid = |w: &[f32]| -> f32 {
            let (mut num, mut den) = (0.0f64, 0.0f64);
            for k in 0..24 {
                let hz = 400.0 * (12_000.0f64 / 400.0).powf(f64::from(k) / 23.0);
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (i, &x) in w.iter().enumerate() {
                    let hann =
                        0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / w.len() as f64).cos();
                    let ph = core::f64::consts::TAU * hz * i as f64 / 48_000.0;
                    re += f64::from(x) * hann * ph.cos();
                    im += f64::from(x) * hann * ph.sin();
                }
                let m = (re * re + im * im).sqrt();
                num += hz * m;
                den += m;
            }
            (num / den.max(1e-30)) as f32
        };
        let ceil: Vec<f32> = full.iter().zip(&no_ceil).map(|(a, b)| a - b).collect();
        let floor: Vec<f32> = full.iter().zip(&no_floor).map(|(a, b)| a - b).collect();
        let (c_ceil, c_floor) = (centroid(&ceil), centroid(&floor));
        assert!(
            c_ceil > c_floor * 1.25,
            "the ceiling must shine: ceiling {c_ceil:.0} Hz vs floor {c_floor:.0} Hz"
        );
    }

    /// The ears live at different places: the two ears' tap delays
    /// for the same image differ (true ITD, not a synthesized one).
    #[test]
    fn the_ears_are_apart() {
        let mut room = Room::new();
        room.prepare(48_000.0);
        room.tune(1.0, 0.0, 0.7, 1.0, 0.0, (0.85, 0.90), 0.0, 256);
        let differing = (0..IMAGES)
            .filter(|&i| (room.taps[i].delay - room.taps[i + IMAGES].delay).abs() > 0.5)
            .count();
        assert!(
            differing > IMAGES / 2,
            "only {differing}/{IMAGES} images show interaural delay"
        );
    }
}
