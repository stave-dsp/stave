//! THE SCENE — the ONE description of the place. Every acoustic
//! region derives from here: the ROOM's image sources (early), the
//! tank's feed (late, via the room), and the MODES (LF standing
//! waves). Before this file, room.rs and modes.rs each carried
//! their own copy of the dims, stage and listener math, "kept in
//! sync" by comments — the exact silent-drift class that produced
//! two shipped bugs (an external review's catch). Now the compiler
//! enforces the single description: neither consumer owns a
//! geometry constant.

/// Speed of sound, m/s.
pub(super) const SPEED_M_S: f32 = 343.0;
/// The room at size 1.0 (W, D, H in meters). Height scales gentler
/// than plan (^0.85) — ceilings grow slower than floors.
const BASE_DIMS: [f32; 3] = [7.4, 9.6, 4.4];
/// The STAGE: two sources (L, R) straddling the room's center
/// axis, forward of the listener.
const STAGE_Z_F: f32 = 0.44;
const STAGE_SPREAD_F: f32 = 0.18;
/// The listener (STAND walks the x fraction ±0.32 around center).
const LIS_Y_F: f32 = 0.27;
const LIS_Z_F: f32 = 0.47;
/// Half the interaural distance, meters — the head does NOT scale
/// with the room.
pub(super) const EAR_DX: f32 = 0.09;

/// The described place, block-rate cheap.
pub(super) struct Scene {
    /// Meters.
    pub dims: [f32; 3],
    /// Half the interaural distance, meters — scaled by the
    /// IMPOSSIBLE HEAD above natural width.
    pub ear_dx: f32,
    /// Source positions as FRACTIONS of the dims (source 0 = the
    /// LEFT input's stage spot, 1 = the right's).
    pub src_f: [[f32; 3]; 2],
    /// Listener position as fractions.
    pub lis_f: [f32; 3],
}

impl Scene {
    pub fn describe(size: f32, stand: f32) -> Self {
        Self::describe_full(size, stand, 0.7, 1.0)
    }

    /// The full description: `distance` walks the STAGE toward or
    /// away from the listener (0 = intimate, 1 = the far wall) —
    /// ITDG, wet-internal D/R and air brightness all move together
    /// because they are one geometry. `ear_scale` is THE IMPOSSIBLE
    /// HEAD: above natural width the ears themselves move apart
    /// (geometry-derived hyper-width, not a post M/S trick).
    pub fn describe_full(size: f32, stand: f32, distance: f32, ear_scale: f32) -> Self {
        let dims = [
            BASE_DIMS[0] * size,
            BASE_DIMS[1] * size,
            BASE_DIMS[2] * size.powf(0.85),
        ];
        let lis_x_f = 0.5 + 0.32 * stand.clamp(-1.0, 1.0);
        // Stage depth: 0.34 (right in front of the listener at
        // y 0.27) to 0.84 (the far wall). 0.7 maps to ~the old
        // fixed 0.70 — the pre-distance voicing.
        let stage_y = 0.34 + 0.5 * distance.clamp(0.0, 1.0);
        Self {
            dims,
            src_f: [
                [0.5 - STAGE_SPREAD_F, stage_y, STAGE_Z_F],
                [0.5 + STAGE_SPREAD_F, stage_y, STAGE_Z_F],
            ],
            lis_f: [lis_x_f, LIS_Y_F, LIS_Z_F],
            // 0.04..4: coincident ears (narrow-to-mono) through
            // the impossible head (the stereo-honesty slice).
            ear_dx: EAR_DX * ear_scale.clamp(0.04, 4.0),
        }
    }

    /// A source's position in meters.
    pub fn source(&self, i: usize) -> [f32; 3] {
        [
            self.src_f[i][0] * self.dims[0],
            self.src_f[i][1] * self.dims[1],
            self.src_f[i][2] * self.dims[2],
        ]
    }

    /// The two ears' positions in meters.
    pub fn ears(&self) -> [[f32; 3]; 2] {
        let lis = [
            self.lis_f[0] * self.dims[0],
            self.lis_f[1] * self.dims[1],
            self.lis_f[2] * self.dims[2],
        ];
        [
            [lis[0] - self.ear_dx, lis[1], lis[2]],
            [lis[0] + self.ear_dx, lis[1], lis[2]],
        ]
    }

    /// The field's lateral lean: where the listener stands relative
    /// to the STAGE CENTER (positive = stage is to the left).
    pub fn balance(&self) -> f32 {
        (2.0 * (self.lis_f[0] - 0.5)).clamp(-1.0, 1.0)
    }

    /// Mean free path (4V/S), in SECONDS of travel — the room's
    /// own statistical bounce interval; the mid-field's clock.
    pub fn mean_free_path_s(&self) -> f32 {
        let [w, d, h] = self.dims;
        let v = w * d * h;
        let s = 2.0 * (w * d + w * h + d * h);
        4.0 * v / s / SPEED_M_S
    }
}

/// THE MATERIALS — what the walls are MADE of (field: "they all
/// sound like a room with the same materials"). Each entry:
/// (wall gain per bounce, wall cutoff multiplier, bass decay
/// multiplier, high decay multiplier, damp offset). PLASTER is the
/// pre-materials voicing exactly (the default — every pin holds).
pub(super) struct Material {
    pub wall: (f32, f32),
    pub bass_mult: f32,
    pub hi_mult: f32,
    pub damp_add: f32,
}

pub(super) fn material(idx: usize) -> Material {
    // Order matches params::MATERIALS.
    // Per-band T60 declarations (worklist 3): bass_mult and
    // hi_mult are PROMISES — the tank's shelves realize them and
    // materials_keep_their_promises referees the band report.
    const TABLE: [((f32, f32), f32, f32, f32); 6] = [
        ((0.87, 0.80), 0.85, 0.80, 0.05),  // WOOD: warm, eats highs a bit
        ((0.85, 0.90), 1.00, 1.00, 0.00),  // PLASTER: the baseline
        ((0.95, 1.08), 1.50, 1.05, -0.08), // STONE: hard, bass rings on
        ((0.93, 1.25), 0.75, 1.20, -0.12), // GLASS: bright, thin lows
        ((0.60, 0.45), 0.65, 0.50, 0.30),  // DRAPE: soft, dark, short
        ((0.96, 1.35), 1.15, 1.35, -0.15), // METAL: hard and glassy-bright
    ];
    let (wall, bass_mult, hi_mult, damp_add) = TABLE[idx.min(5)];
    Material {
        wall,
        bass_mult,
        hi_mult,
        damp_add,
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    /// One description: the stage straddles center, the listener
    /// walks with STAND, and the balance is their difference.
    #[test]
    fn the_scene_is_one_description() {
        let sc = Scene::describe(1.0, 0.5);
        assert!(
            (sc.src_f[0][0] + sc.src_f[1][0] - 1.0).abs() < 1e-6,
            "stage centered"
        );
        assert!(
            (sc.lis_f[0] - 0.66).abs() < 1e-6,
            "stand walks the listener"
        );
        assert!((sc.balance() - 0.32).abs() < 1e-6, "balance is the offset");
        let big = Scene::describe(3.0, 0.0);
        assert!(big.dims[0] > 2.9 * sc.dims[0], "size scales the plan");
        assert!(big.dims[2] < 3.0 * sc.dims[2], "ceilings grow slower");
    }
}
