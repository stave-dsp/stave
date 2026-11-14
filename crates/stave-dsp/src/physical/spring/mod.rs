//! Spring reverberation: a helical spring between two magnetic
//! transducers, as dispersive waveguides.
//!
//! Waves cross a spring at speeds that depend on frequency, so an
//! impulse arrives as a chirp, and every reflection between the beads
//! disperses it further: the "boing" of a struck tank. Below the
//! spring's transition frequency `f_C` the group delay rises toward
//! `f_C`; a second, much quieter family of chirps covers the whole band
//! with the delay falling from DC (Välimäki, Parker & Abel 2010; Parker
//! 2011). [`Spring`] models one spring this way; [`Tank`] drives several
//! from one transducer and sums them at the pickup; [`Chirp`] is the
//! dispersive element both are built from.
//!
//! **Patent note.** Spring emulation with dispersive allpass delays in
//! waveguide structures is described and claimed in US 8,391,504 (Abel,
//! Berners, Costello & Smith; filed 2007, so probably in force until
//! about 2027–28). Check its claims before shipping a product on this
//! module. The patent-safe path is a modal spring — the helical spring's
//! modes computed from its dimensions (van Walstijn, DAFx 2020; McQuillan
//! & van Walstijn, DAFx20in21) as a resonator bank — which is also the
//! physical reference this module's dispersion could be held to; it is
//! not built yet.

mod chirp;
mod tank;

pub use chirp::Chirp;
pub use tank::{Tank, MAX_SPRINGS};

use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::biquad::{Biquad, FilterSpec};
use crate::primitives::time::delay::DelayLine;
use crate::units::{Db, Hz, Seconds};

/// The low chirps' allpass coefficient: positive, so the delay peaks at
/// `f_C`.
pub const LOW_COEFFICIENT: f32 = 0.6;
/// The wide-band chirps' coefficient: negative, so the delay peaks at DC.
pub const HIGH_COEFFICIENT: f32 = -0.6;
/// Sections in the low chirp cascade (both directions together), as in
/// Välimäki et al.
pub const LOW_SECTIONS: usize = 100;
/// Sections in the wide-band cascade (both directions together).
pub const HIGH_SECTIONS: usize = 200;
/// The rate the wide-band cascade's unit stretch is designed at.
const HIGH_DESIGN_RATE: f64 = 44_100.0;

/// One dispersive waveguide: a forward and a backward travelling wave,
/// each a delay and half the chirp cascade, reflected at the beads with
/// the round trip's loss.
#[derive(Clone, Debug, Default)]
struct Waveguide {
    forward: DelayLine,
    forward_chirp: Chirp,
    back: DelayLine,
    back_chirp: Chirp,
    length: usize,
    reflection: f32,
    returning: f32,
}

impl Waveguide {
    fn new(sections: usize, stretch: f64, a: f32, length: usize) -> Self {
        let length = length.max(1);
        let mut w = Self {
            forward_chirp: Chirp::stretched(sections / 2, stretch, a),
            back_chirp: Chirp::stretched(sections - sections / 2, stretch, a),
            length,
            ..Self::default()
        };
        w.forward.allocate(length + 1);
        w.back.allocate(length + 1);
        w
    }

    /// The mean round trip, in samples: both delays and both cascades'
    /// mean group delay.
    fn round_trip(&self) -> f64 {
        2.0 * self.length as f64 + self.forward_chirp.mean_delay() + self.back_chirp.mean_delay()
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.forward.write(x + self.reflection * self.returning);
        let arriving = self.forward.read_int(self.length);
        self.forward.advance();
        let y = self.forward_chirp.tick(arriving);
        // The back path is a sample short: `returning` waits one.
        self.back.write(y);
        let back = self.back.read_int(self.length - 1);
        self.back.advance();
        self.returning = self.back_chirp.tick(back);
        y
    }

    fn reset(&mut self) {
        self.forward.reset();
        self.back.reset();
        self.forward_chirp.reset();
        self.back_chirp.reset();
        self.returning = 0.0;
    }
}

/// One spring between ideal transducers: the low chirps below the
/// transition frequency and the quieter wide-band chirps, each a
/// dispersive waveguide.
///
/// # Models
/// The parametric spring of Välimäki, Parker & Abel, "Parametric Spring
/// Reverberation Effect", JAES 58(7/8), 2010 (structures `C_lf` and
/// `C_hf`, after Parker's spring measurements), written as waveguides:
/// a wave crosses the spring (a delay and half a [`Chirp`] cascade),
/// reflects at the far bead, crosses back, and reflects again with the
/// round trip's loss — so arrivals come at 1, 3, 5 … transits and each
/// is dispersed further, the chirp train that is a spring's "boing" on
/// a transient. Low chirps: [`LOW_SECTIONS`] stretched allpasses of
/// [`LOW_COEFFICIENT`], stretch `k = fs/(2·f_C)`, then a fourth-order
/// lowpass at `f_C`; wide-band chirps: [`HIGH_SECTIONS`] of
/// [`HIGH_COEFFICIENT`], 30 dB down (Parker 2011: "generally at least
/// 30 dB below").
///
/// # Derivation
/// Group delay per transit `L + D(ω)/2`, `D` Parker's eq. 4 for the whole
/// cascade; `L` set so the band-mean transit (`L + kM/2`, the cascade's
/// mean delay being `kM`) is the spring's transit time. The decay is a
/// reflection gain per round trip from T60 over the mean round trip, so
/// the setting holds where `D(ω) = kM` (`cos(ωk) = −a`). The stretches
/// are fractional ([`Chirp::stretched`]): the low `k = fs/(2·f_C)`
/// exactly, so `f_C` is the one asked for at every rate; the wide-band
/// `fs/44.1 kHz`, which keeps its chirp's length in time. (Whole-sample
/// stretches moved `f_C` with the rate — 3.68 kHz for 4 kHz at
/// 44.1 kHz — and snapped a tank's spread of transitions to one.)
///
/// # Verified
/// `crates/stave-dsp/src/physical/spring/tests.rs::transient_boings_as_rising_chirps`
/// (the group-delay ridge of the impulse response — the time each band's
/// energy peaks — at the first and second arrivals against
/// `L + D/2` and `3L + 3D/2` (plus the lowpass's group delay), 300 Hz
/// to 3 kHz, within 0.1 ms; measured 0.05 ms, and rising with frequency),
/// `crates/stave-dsp/src/physical/spring/tests.rs::decay_is_the_t60`
/// (the band about `cos(ωk) = −a` decays at the set T60 within 2 % at 1.5
/// and 3 s; measured +0.5 and +0.2 %),
/// `crates/stave-dsp/src/physical/spring/tests.rs::lossless_spring_stays_bounded`
/// (lossless: a second of full-scale noise, then silence, stays bounded
/// and finite for 2–4 s, at 44.1–192 kHz) and the chirp's own tests.
///
/// # Cost
/// `"Spring (low and wide-band chirps)"` and `"Spring (low chirps only)"`
/// on the datasheet: a multiply–add per allpass section on a sample's
/// path, 300 sections with the wide-band chirps, 100 without.
///
/// # Aliasing
/// None (linear).
///
/// # Valid ranges
/// Transition frequency 2–6 kHz, below `fs/2` (a stretch of at least 1);
/// transit time longer than the low cascade's half mean delay
/// (`kM/2/fs`, 6.25 ms at the defaults); T60 in `(0, ∞]`.
#[derive(Clone, Debug)]
pub struct Spring {
    transit: Seconds,
    transition: Hz,
    t60: Seconds,
    high: Db,
    sample_rate: f32,
    low: Waveguide,
    high_chirps: Waveguide,
    high_gain: f32,
    lowpass: [Biquad; 2],
}

impl Spring {
    /// A spring whose waves cross in `transit` (band mean) with chirps
    /// up to `transition`; T60 2.5 s, wide-band chirps 30 dB down.
    /// Allocates in `prepare`.
    pub fn new(transit: Seconds, transition: Hz) -> Self {
        let mut s = Self {
            transit,
            transition,
            t60: Seconds::new(2.5),
            high: Db::new(-30.0),
            sample_rate: 48_000.0,
            low: Waveguide::default(),
            high_chirps: Waveguide::default(),
            high_gain: 0.0,
            lowpass: [Biquad::default(), Biquad::default()],
        };
        s.build();
        s
    }

    /// The decay to −60 dB (infinite: lossless). Block rate.
    pub fn set_t60(&mut self, t60: Seconds) {
        self.t60 = t60;
        self.update_decay();
    }

    /// The wide-band chirps' level relative to the low ones; at −120 dB
    /// or below they are not computed (Parker 2011's cheaper spring).
    pub fn set_high_chirps(&mut self, level: Db) {
        self.high = level;
        self.high_gain = if level.get() <= -120.0 { 0.0 } else { 10f32.powf(level.get() / 20.0) };
    }

    /// The transition frequency in use, `fs/(2k)`: the one set, unless
    /// that is above a quarter of the rate (a stretch under 2).
    pub fn transition(&self) -> Hz {
        Hz::new((f64::from(self.sample_rate) / (2.0 * self.low.forward_chirp.stretch())) as f32)
    }

    /// The low chirps' transit delay `L` (each direction), in samples.
    pub fn transit_samples(&self) -> usize {
        self.low.length
    }

    /// The low chirp cascade of one transit (half the sections), for its
    /// group delay.
    pub fn low_chirp(&self) -> &Chirp {
        &self.low.forward_chirp
    }

    /// The low chirps' reflection gain per round trip.
    pub fn reflection(&self) -> f32 {
        self.low.reflection
    }

    fn build(&mut self) {
        let fs = f64::from(self.sample_rate);
        let k = (fs / (2.0 * f64::from(self.transition.get().max(1.0)))).max(1.0);
        let transit = self.transit.get() * fs;
        let low_len = (transit - k * LOW_SECTIONS as f64 / 2.0).round().max(2.0) as usize;
        self.low = Waveguide::new(LOW_SECTIONS, k, LOW_COEFFICIENT, low_len);
        let kh = (fs / HIGH_DESIGN_RATE).max(1.0);
        let high_len = (transit - kh * HIGH_SECTIONS as f64 / 2.0).round().max(2.0) as usize;
        self.high_chirps = Waveguide::new(HIGH_SECTIONS, kh, HIGH_COEFFICIENT, high_len);
        let fc = (fs / (2.0 * k)) as f32;
        // A fourth-order Butterworth at f_C (Q of its two sections).
        for (b, q) in self.lowpass.iter_mut().zip([0.541_196_1, 1.306_563]) {
            *b = Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(fc.min(0.45 * self.sample_rate)), q });
            b.prepare(self.sample_rate);
        }
        self.set_high_chirps(self.high);
        self.update_decay();
    }

    fn update_decay(&mut self) {
        let t60 = self.t60.get();
        let fs = f64::from(self.sample_rate);
        let gain = |w: &Waveguide| -> f32 {
            if t60.is_infinite() {
                1.0
            } else if t60.is_nan() || t60 <= 0.0 {
                0.0
            } else {
                10f64.powf(-3.0 * w.round_trip() / (t60 * fs)) as f32
            }
        };
        self.low.reflection = gain(&self.low);
        self.high_chirps.reflection = gain(&self.high_chirps);
    }
}

impl Stage for Spring {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.build();
    }

    fn reset(&mut self) {
        self.low.reset();
        self.high_chirps.reset();
        self.lowpass.iter_mut().for_each(Stage::reset);
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let low = self.low.tick(x);
        let low = self.lowpass[0].tick(low);
        let low = self.lowpass[1].tick(low);
        if self.high_gain > 0.0 {
            low + self.high_gain * self.high_chirps.tick(x)
        } else {
            low
        }
    }

    fn tail(&self) -> Tail {
        let t60 = self.t60.get();
        if !t60.is_finite() {
            return Tail::Infinite;
        }
        // Two T60s reach −120 dB, after the slowest first arrival.
        let first = self.low.round_trip() + self.low.forward_chirp.mean_delay() * 4.0;
        let n = 2.0 * t60 * f64::from(self.sample_rate) + first;
        if n >= f64::from(u32::MAX) {
            Tail::Infinite
        } else {
            Tail::samples(n.ceil() as u32)
        }
    }
}

#[cfg(test)]
mod tests;
