//! A rotary-speaker cabinet: a crossover, a horn and a drum.

use super::{Directivity, Mic, Radiator, Rotor, Speed, Wall};
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::crossover::{Crossover, Order};
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};
use crate::units::Hz;

/// A rotary speaker: the signal split at a crossover, the treble to a
/// horn [`Radiator`] and the bass to a drum one, both heard by the same
/// two microphones.
///
/// # Models
/// A Leslie-type cabinet (Smith et al., DAFx-02; Pekonen et al.,
/// DAFx-11, Fig. 2): crossover, two independently driven rotors with
/// their own speeds and inertia, Doppler, directivity and reflections
/// per rotor ([`Radiator`]), summed at each microphone. The power amp,
/// the drivers and the cabinet's own resonances are not modelled (the
/// 122's 6550 push-pull amp is a circuit; H7 covers speakers).
///
/// # Derivation
/// [`Cabinet::leslie_122h`]: the Hammond 122H/142H guide's 800 Hz
/// crossover as a Linkwitz–Riley 4th order (the slope is not published;
/// Pekonen et al. use 4th order), its factory rotor speeds and ramps
/// ([`Rotor::HORN_122H`], [`Rotor::DRUM_122H`]), the horn and drum
/// directivities and the closed back ([`Wall::BACK_122H`]) as voicing.
///
/// # Verified
/// `crates/stave-dsp/src/physical/rotary/tests.rs::cabinet_bands_reach_their_rotors`
/// (with both rotors still and omnidirectional the cabinet is the
/// crossover's allpass sum, delayed by the path, within 1e-4), and the
/// [`Radiator`] tests it is built on.
///
/// # Cost
/// `"Cabinet (Leslie 122H, stereo)"` on the datasheet.
///
/// # Aliasing
/// As [`Radiator`]: the interpolator's images of a modulated read.
///
/// # Valid ranges
/// As [`Radiator`]; the crossover corner inside `(0, 0.49·fs)`.
#[derive(Clone, Debug)]
pub struct Cabinet<I: Interpolator = Lagrange3> {
    crossover: Crossover<2>,
    horn: Radiator<I>,
    drum: Radiator<I>,
}

impl<I: Interpolator> Cabinet<I> {
    /// A horn and a drum split at `corner`. Allocates.
    pub fn new(horn: Radiator<I>, drum: Radiator<I>, corner: Hz) -> Self {
        Self { crossover: Crossover::linkwitz_riley(Order::Lr4, &[corner]), horn, drum }
    }

    /// A Hammond 122H (and 147-type) cabinet at its factory settings,
    /// microphones 1 m away at ±60°.
    pub fn leslie_122h() -> Self {
        let mut horn = Radiator::new(Rotor::HORN_122H, Directivity::HORN_122H);
        let mut drum = Radiator::new(Rotor::DRUM_122H, Directivity::DRUM_122H);
        let (h, d) = (horn.set_walls(&[Wall::BACK_122H]), drum.set_walls(&[Wall::BACK_122H]));
        debug_assert!(h && d, "the 122H's back wall fits the default room");
        Self::new(horn, drum, Hz::new(800.0))
    }

    /// The horn rotor.
    pub fn horn(&mut self) -> &mut Radiator<I> {
        &mut self.horn
    }

    /// The drum rotor.
    pub fn drum(&mut self) -> &mut Radiator<I> {
        &mut self.drum
    }

    /// Switch both rotors (each ramps by its own inertia).
    pub fn set_speed(&mut self, speed: Speed) {
        self.horn.set_speed(speed);
        self.drum.set_speed(speed);
    }

    /// Place microphone `i` for both rotors; `false` if either refuses
    /// (see [`Radiator::set_mic`]), and then neither moves.
    pub fn set_mic(&mut self, i: usize, mic: Mic) -> bool {
        let (h, d) = (self.horn.mics()[i.min(1)], self.drum.mics()[i.min(1)]);
        if self.horn.set_mic(i, mic) && self.drum.set_mic(i, mic) {
            return true;
        }
        self.horn.set_mic(i, h);
        self.drum.set_mic(i, d);
        false
    }

    /// One sample in, both microphones out.
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        let [low, high] = self.crossover.split(x);
        let (hl, hr) = self.horn.tick_stereo(high);
        let (dl, dr) = self.drum.tick_stereo(low);
        (hl + dl, hr + dr)
    }
}

impl<I: Interpolator> Stage for Cabinet<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.crossover.prepare(sample_rate);
        self.horn.prepare(sample_rate);
        self.drum.prepare(sample_rate);
    }

    fn reset(&mut self) {
        self.crossover.reset();
        self.horn.reset();
        self.drum.reset();
    }

    /// The left microphone.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_stereo(x).0
    }

    fn tail(&self) -> Tail {
        self.crossover.tail().then(self.horn.tail().max(self.drum.tail()))
    }
}
