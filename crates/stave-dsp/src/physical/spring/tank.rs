//! A spring tank: one drive transducer, several springs, one pickup.

use super::Spring;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::OnePole;
use crate::units::{Db, Hz, Seconds};

/// The longest list of springs a tank takes.
pub const MAX_SPRINGS: usize = 4;

/// A reverb tank: a drive coil, springs in parallel, a pickup coil.
///
/// # Models
/// A multi-spring tank (the two- and three-spring long tanks of guitar
/// amplifiers): the drive coil's current lags its voltage through the
/// coil's inductance (a first-order lowpass at `R/(2πL)`); the springs,
/// each a [`Spring`] with its own transit time, are twisted by the same
/// input bead and summed at the output bead; the pickup's EMF is the
/// output bead's velocity (a first-order highpass: no output at DC).
/// Wet only.
///
/// # Derivation
/// The transducers as first-order sections at their corners
/// (impulse-invariant one-poles); springs summed with `1/√n` so the
/// tank's level does not grow with their number; the springs' transition
/// frequencies spread by a few percent, as wire tolerances spread them.
///
/// # Verified
/// `crates/stave-dsp/src/physical/spring/tests.rs::every_spring_arrives_in_a_tank`
/// (a three-spring tank's impulse response peaks, in the 1 kHz band, at
/// each spring's first arrival within 0.2 ms),
/// `crates/stave-dsp/src/physical/spring/tests.rs::transducers_band_limit_the_tank`
/// (the pickup removes DC and the drive and the springs' lowpass the
/// top: the tank's response at 20 Hz sits more than 12 dB, and at
/// 12 kHz more than 30 dB, below its 1–3 kHz band), and
/// the [`Spring`] tests it is built on.
///
/// # Cost
/// `"Tank (3 springs)"` on the datasheet.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// One to [`MAX_SPRINGS`] springs; each as [`Spring`]; drive and pickup
/// corners in `(0, 0.49·fs)`.
#[derive(Clone, Debug)]
pub struct Tank {
    springs: Vec<Spring>,
    drive: OnePole,
    pickup: OnePole,
    scale: f32,
}

impl Default for Tank {
    /// The three-spring long tank: transits of 27, 33 and 41 ms.
    fn default() -> Self {
        Self::new(&[Seconds::new(0.027), Seconds::new(0.033), Seconds::new(0.041)])
    }
}

impl Tank {
    /// A tank of springs with these transit times (one to
    /// [`MAX_SPRINGS`]; extras are ignored, none gives one of 33 ms),
    /// transition frequencies spread from 4.0 kHz, drive coil corner at
    /// 5 kHz, pickup at 150 Hz. Allocates here and in `prepare`.
    pub fn new(transits: &[Seconds]) -> Self {
        let default = [Seconds::new(0.033)];
        let transits = if transits.is_empty() { &default[..] } else { transits };
        let springs: Vec<Spring> = transits
            .iter()
            .take(MAX_SPRINGS)
            .enumerate()
            .map(|(i, &t)| Spring::new(t, Hz::new(4_000.0 * (1.0 + 0.04 * i as f32))))
            .collect();
        let scale = 1.0 / (springs.len() as f32).sqrt();
        Self {
            springs,
            drive: OnePole::lowpass(5_000.0),
            pickup: OnePole::highpass(150.0),
            scale,
        }
    }

    /// The springs, for inspection.
    pub fn springs(&self) -> &[Spring] {
        &self.springs
    }

    /// Every spring's decay to −60 dB.
    pub fn set_t60(&mut self, t60: Seconds) {
        self.springs.iter_mut().for_each(|s| s.set_t60(t60));
    }

    /// Every spring's wide-band chirp level.
    pub fn set_high_chirps(&mut self, level: Db) {
        self.springs.iter_mut().for_each(|s| s.set_high_chirps(level));
    }

    /// The drive coil's corner, `R/(2πL)`.
    pub fn set_drive_corner(&mut self, corner: Hz) {
        self.drive.set_hz(corner.get());
    }

    /// The pickup's corner.
    pub fn set_pickup_corner(&mut self, corner: Hz) {
        self.pickup.set_hz(corner.get());
    }
}

impl Stage for Tank {
    fn prepare(&mut self, sample_rate: f32) {
        self.drive.prepare(sample_rate);
        self.pickup.prepare(sample_rate);
        self.springs.iter_mut().for_each(|s| s.prepare(sample_rate));
    }

    fn reset(&mut self) {
        self.drive.reset();
        self.pickup.reset();
        self.springs.iter_mut().for_each(Stage::reset);
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let driven = self.drive.tick(x);
        let mut sum = 0.0;
        for s in &mut self.springs {
            sum += s.tick(driven);
        }
        self.pickup.tick(sum * self.scale)
    }

    fn tail(&self) -> Tail {
        self.springs.iter().fold(Tail::None, |t, s| t.max(s.tail())).then(self.pickup.tail())
    }
}
