//! A rectangular room around a source and a microphone: image-source
//! reflections and a late field.

use super::{Fdn, Mixing, T60};
use crate::physical::rotary::SPEED_OF_SOUND;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Sinc};
use crate::units::Seconds;

type Read = Sinc<8>;

/// A rectangular room: its size (m, along x, y, z), the cabinet's and
/// the microphone's positions in it (m, from the corner at the origin),
/// and each wall's pressure reflection coefficient, in the order x = 0,
/// x = size, y = 0, y = size, z = 0 (the floor), z = size (the ceiling).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Shoebox {
    pub size: [f64; 3],
    pub source: [f64; 3],
    pub mic: [f64; 3],
    pub reflection: [f64; 6],
}

impl Shoebox {
    /// A 5 × 4 × 2.7 m live room, cabinet on the floor (cone 35 cm up)
    /// 1.2 m from a wall, the microphone 1 m in front and a little to
    /// the side at the cone's height; a reflective floor, plastered walls
    /// and an absorbent ceiling (reflection 0.9, 0.85, 0.6). A voicing.
    pub const LIVE_ROOM: Self = Self {
        size: [5.0, 4.0, 2.7],
        source: [1.2, 1.6, 0.35],
        mic: [2.2, 1.9, 0.35],
        reflection: [0.85, 0.85, 0.85, 0.85, 0.9, 0.6],
    };

    /// The direct path, m.
    pub fn direct(&self) -> f64 {
        distance(self.source, self.mic)
    }

    /// The six first-order image sources (Allen & Berkley 1979): each
    /// wall's mirror image of the source, as (path length m, reflection).
    pub fn images(&self) -> [(f64, f64); 6] {
        core::array::from_fn(|w| {
            let axis = w / 2;
            let mut image = self.source;
            image[axis] = if w % 2 == 0 { -self.source[axis] } else { 2.0 * self.size[axis] - self.source[axis] };
            (distance(image, self.mic), self.reflection[w])
        })
    }

    /// The room's volume, m³.
    pub fn volume(&self) -> f64 {
        self.size[0] * self.size[1] * self.size[2]
    }

    /// Sabine's reverberation time, `0.161·V / Σ Sᵢ·αᵢ` with each wall's
    /// absorption `α = 1 − β²`, s.
    pub fn t60(&self) -> f64 {
        let [x, y, z] = self.size;
        let areas = [y * z, y * z, x * z, x * z, x * y, x * y];
        let absorption: f64 = areas.iter().zip(&self.reflection).map(|(s, b)| s * (1.0 - b * b)).sum();
        0.161 * self.volume() / absorption.max(1e-9)
    }

    /// The mean free path `4V/S` between reflections, m.
    pub fn mean_free_path(&self) -> f64 {
        let [x, y, z] = self.size;
        4.0 * self.volume() / (2.0 * (x * y + x * z + y * z))
    }

    /// Kuttruff's critical distance `0.057·√(V/T60)` (an omnidirectional
    /// source): where the reverberant field is as loud as the direct
    /// sound, m.
    pub fn critical_distance(&self) -> f64 {
        0.057 * (self.volume() / self.t60()).sqrt()
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The room around a cabinet: the input (what the microphone hears of
/// the cabinet directly) plus the room's first reflections and its
/// late reverberation.
///
/// # Models
/// A rectangular room's six first-order reflections as image sources
/// (Allen & Berkley, "Image method for efficiently simulating
/// small-room acoustics", JASA 65(4), 1979), each delayed by its extra
/// path and scaled by its wall's reflection and the spherical spreading
/// relative to the direct path, frequency-independent; and a diffuse
/// late field: the [`Fdn`], its lines scaled to the room's mean free
/// path, decaying by Sabine's T60 at the level
/// Kuttruff's room acoustics gives a diffuse field relative to the
/// direct sound at the microphone's distance (`(r/r_c)²` in energy,
/// *Room Acoustics*, 5th ed., §5.1). The cabinet is an omnidirectional
/// source to the room; higher-order reflections and the walls'
/// frequency dependence are the late field's.
///
/// # Derivation
/// Geometry in [`Shoebox`]; the reflections read from one delay line
/// through an 8-tap windowed sinc. The late field's level is set at
/// `prepare` by the network's own impulse energy (measured there), so
/// its energy relative to the direct sound is Kuttruff's ratio.
///
/// # Verified
/// `crates/stave-dsp/src/effects/reverb/room/tests.rs`: each reflection
/// arrives at its image's delay and level (`room_reflections_follow_the_images`);
/// the late field decays at Sabine's T60 within 5 % and carries
/// Kuttruff's energy within 10 % (`room_tail_follows_sabine_and_kuttruff`).
///
/// # Cost
/// `"Room (live room, early + late)"` on the datasheet.
///
/// # Aliasing
/// None (linear; the network's modulation is S3's).
///
/// # Valid ranges
/// Positions inside the room, reflections in `[0, 1)`, rooms up to the
/// size whose longest reflection fits 200 ms.
#[derive(Clone, Debug)]
pub struct Room {
    shoebox: Shoebox,
    line: DelayLine<Read>,
    taps: [(f32, f32); 6],
    late: Fdn,
    late_gain: f32,
    early: bool,
    rate: f32,
}

impl Room {
    /// The room `shoebox`, early and late fields on.
    pub fn new(shoebox: Shoebox) -> Self {
        Self {
            shoebox,
            line: DelayLine::default(),
            taps: [(0.0, 0.0); 6],
            late: Fdn::new(Mixing::default()),
            late_gain: 0.0,
            early: true,
            rate: 48_000.0,
        }
    }

    /// The geometry.
    pub fn shoebox(&self) -> &Shoebox {
        &self.shoebox
    }

    /// The reflections as designed: (delay after the direct sound,
    /// samples; gain relative to it).
    pub fn taps(&self) -> &[(f32, f32); 6] {
        &self.taps
    }

    /// The late field's gain on the network's output.
    pub fn late_gain(&self) -> f32 {
        self.late_gain
    }

    /// Hear the first reflections (`false`: the late field only).
    pub fn set_early(&mut self, on: bool) {
        self.early = on;
    }

    /// Set the late field's level directly (`prepare` sets Kuttruff's).
    pub fn set_late_gain(&mut self, gain: f32) {
        self.late_gain = gain;
    }

    fn design(&mut self) {
        let direct = self.shoebox.direct();
        for (tap, (path, reflection)) in self.taps.iter_mut().zip(self.shoebox.images()) {
            let delay = (path - direct) / SPEED_OF_SOUND * f64::from(self.rate);
            *tap = (delay as f32, (reflection * direct / path) as f32);
        }
    }
}

impl Stage for Room {
    fn prepare(&mut self, sample_rate: f32) {
        self.rate = sample_rate;
        self.line.allocate((0.2 * sample_rate) as usize + Read::NEWER + Read::OLDER + 2);
        self.design();
        let t60 = self.shoebox.t60() as f32;
        // The network's lines scaled so their mean is the room's mean
        // free path; the decay held per octave (S3b's compensated form).
        let mean_ms = 50.725;
        self.late.set_size((self.shoebox.mean_free_path() / SPEED_OF_SOUND * 1000.0 / mean_ms) as f32);
        self.late.set_t60(T60::uniform(Seconds::new(f64::from(t60))));
        self.late.prepare(sample_rate);
        // The network's impulse energy, to set Kuttruff's level.
        let n = (f64::from(t60) * 1.5 * f64::from(sample_rate)) as usize;
        let mut energy = 0.0f64;
        for k in 0..n {
            let (l, r) = self.late.tick_stereo(if k == 0 { 1.0 } else { 0.0 });
            energy += (0.5 * (f64::from(l) + f64::from(r))).powi(2);
        }
        let ratio = self.shoebox.direct() / self.shoebox.critical_distance();
        self.late_gain = (ratio / energy.max(1e-30).sqrt()) as f32;
        self.reset();
    }

    fn reset(&mut self) {
        self.line.reset();
        self.late.reset();
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.line.write(x);
        let mut y = x;
        if self.early {
            for &(d, g) in &self.taps {
                y += g * self.line.interpolated(d);
            }
        }
        self.line.advance();
        let (l, r) = self.late.tick_stereo(x);
        y + self.late_gain * 0.5 * (l + r)
    }

    fn tail(&self) -> Tail {
        Tail::Samples((self.shoebox.t60() * f64::from(self.rate)).ceil() as u32)
    }
}

#[cfg(test)]
mod tests;
