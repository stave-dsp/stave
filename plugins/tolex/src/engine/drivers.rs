//! Drivers — the multi-speaker chorus. A 2×12 or 4×12 is
//! mostly GEOMETRY at the mic: neighbor cones radiate the same
//! signal over longer paths, √(r² + d²) for 12" centers d, and
//! comb against the near cone. Close-mic'd, the neighbors nearly
//! vanish (the extra path dwarfs r and 1/distance buries them) —
//! which is why a close 4×12 sounds like its nearest cone and a
//! pulled-back one blooms into the classic weave. Level is
//! normalized by the total in-phase sum so driver count changes
//! the WEAVE, not the volume.

use stave_dsp::DelayLine;

const C_AIR: f32 = 343.0;
/// 12" driver center spacing in a standard lattice.
const SPACING_M: f32 = 0.33;
/// Neighbor path lengths per layout: 2×12 has one neighbor; 4×12
/// two adjacent and one diagonal.
const LAYOUTS: [&[f32]; 3] = [
    &[],
    &[SPACING_M],
    &[SPACING_M, SPACING_M, SPACING_M * core::f32::consts::SQRT_2],
];

pub struct Drivers {
    delay: DelayLine,
    layout: usize,
    rate: f32,
}

impl Drivers {
    pub fn new() -> Self {
        Self {
            delay: DelayLine::new(),
            layout: 0,
            rate: 0.0,
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        // Longest extra path: the diagonal as r → 0, ~1.4 ms.
        self.delay.prepare((self.rate * 0.002) as usize + 8);
        self.delay.reset();
    }

    pub fn set(&mut self, layout: usize) {
        self.layout = layout.min(LAYOUTS.len() - 1);
    }

    /// `mic_r` in meters (rides the declared mic-distance ramp).
    #[inline]
    pub fn tick(&mut self, x: f32, mic_r: f32) -> f32 {
        self.delay.write(x);
        let mut y = x;
        let mut norm = 1.0f32;
        for d in LAYOUTS[self.layout] {
            let path = (mic_r * mic_r + d * d).sqrt();
            let g = neighbor_gain(mic_r, path);
            let tau = (path - mic_r) / C_AIR;
            y += g * self.delay.read(tau * self.rate);
            norm += g;
        }
        self.delay.advance();
        y / norm
    }
}

/// A neighbor is not a point source: at close mic the listening
/// point sits nearly 90° off its axis, where cone directivity and
/// baffle shadow bury it (the reason a close-mic'd 4×12 sounds like
/// one cone — the chorus law's close case pinned this). First-order
/// cardioid-ish weighting on top of 1/distance: cosθ = r/path.
#[inline]
fn neighbor_gain(mic_r: f32, path: f32) -> f32 {
    let cos_theta = mic_r / path;
    (mic_r / path) * (0.2 + 0.8 * cos_theta)
}

/// The display twin: |Σ paths| / Σ|paths| in dB from the same
/// geometry as tick().
pub(crate) fn response_db(layout: usize, mic_r: f32, hz: f32) -> f32 {
    let (mut re, mut im, mut norm) = (1.0f32, 0.0f32, 1.0f32);
    for d in LAYOUTS[layout.min(LAYOUTS.len() - 1)] {
        let path = (mic_r * mic_r + d * d).sqrt();
        let g = neighbor_gain(mic_r, path);
        let th = core::f32::consts::TAU * hz * (path - mic_r) / C_AIR;
        re += g * th.cos();
        im -= g * th.sin();
        norm += g;
    }
    10.0 * ((re * re + im * im) / (norm * norm)).max(1e-24).log10()
}
