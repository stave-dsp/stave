//! The feedback delay network: eight modulated delay lines recirculating
//! through an orthogonal matrix.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};

use super::{decay_tail, loop_gain};

const LINES: usize = 8;
const MAX_SIZE: f32 = 2.0;

/// Line lengths at size 1, in milliseconds: spread over 30–75 ms, no
/// two in a simple ratio, so the loop's modes interleave.
const LENGTHS_MS: [f32; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.1, 73.9];

/// LFO rates per line (Hz): slow, and unrelated so the lines never move
/// together.
const MOD_HZ: [f32; LINES] = [0.31, 0.43, 0.53, 0.67, 0.73, 0.89, 0.97, 1.13];

/// Output sign patterns: two rows of the 8×8 Hadamard matrix, so the
/// channels are orthogonal mixes of the same lines.
const LEFT: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
const RIGHT: [f32; LINES] = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

/// The orthogonal matrix an [`Fdn`] recirculates through.
///
/// # Models
/// A lossless (unitary) feedback matrix: every eigenvalue on the unit
/// circle, so the decay is set by the line gains alone.
///
/// # Derivation
/// Householder: `A = I − (2/N)·11ᵀ` (Jot & Chaigne 1991), `O(N)`.
/// Hadamard: the Sylvester `H₈/√8`, by the fast Walsh–Hadamard
/// butterfly, `O(N log N)`, every line feeding every other equally.
///
/// # Verified
/// `effects::reverb::tests::the_fdn_matrix_is_unitary`: `AᵀA = I` within
/// 1e-6 for both.
///
/// # Cost
/// Part of the `Fdn` datasheet lines.
///
/// # Aliasing
/// None (a linear map).
///
/// # Valid ranges
/// Any finite input.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Mixing {
    /// `I − (2/N)·11ᵀ`.
    #[default]
    Householder,
    /// `H₈/√8`.
    Hadamard,
}

impl Mixing {
    /// Multiply `v` by the matrix in place.
    #[inline]
    pub fn mix(self, v: &mut [f32; LINES]) {
        match self {
            Mixing::Householder => {
                let k = v.iter().sum::<f32>() * (2.0 / LINES as f32);
                for x in v.iter_mut() {
                    *x -= k;
                }
            }
            Mixing::Hadamard => {
                let mut h = 1;
                while h < LINES {
                    for i in (0..LINES).step_by(2 * h) {
                        for j in i..i + h {
                            let (a, b) = (v[j], v[j + h]);
                            v[j] = a + b;
                            v[j + h] = a - b;
                        }
                    }
                    h *= 2;
                }
                let norm = 1.0 / (LINES as f32).sqrt();
                for x in v.iter_mut() {
                    *x *= norm;
                }
            }
        }
    }
}

/// A feedback delay network reverb: eight delay lines whose outputs are
/// damped, mixed by an orthogonal [`Mixing`] matrix and fed back, with
/// slowly modulated read points.
///
/// # Models
/// Jot's FDN (Jot & Chaigne 1991): `s[n+1] = A·Γ(s[n − m])`, `A`
/// unitary, `Γ` a per-line absorption filter whose gain at DC sets the
/// T60 and whose gain at Nyquist sets the high-frequency T60. No
/// hardware claim; the network is wet only (mix dry yourself).
///
/// # Derivation
/// Line `i` of length `mᵢ` samples has absorption
/// `gᵢ = 10^(−3·mᵢ/(T60·sample_rate))` (the loop falls 60 dB in T60
/// whatever the line), shelved by a one-pole `gᵢ(1 − bᵢ)/(1 − bᵢz⁻¹)`
/// with `bᵢ = (gᵢ − hᵢ)/(gᵢ + hᵢ)`, `hᵢ` the same formula at the
/// high-frequency T60 (Jot 1992). Lengths are set in milliseconds and
/// read fractionally through [`Lagrange3`], so the network is the same
/// in seconds at every rate; each read point wanders ±`depth` with its
/// own LFO, which smears the modes (Dattorro 1997, §3).
///
/// # Verified
/// `effects::reverb::tests` (T60 as Schroeder T30 around 1 kHz, ISO
/// 3382): within 3 % of the setting at 1 and 2.5 s (measured −0.3 % and
/// +1.5 % Householder, 0.0 % and +2.0 % Hadamard); the matrix is unitary
/// within 1e-6; a frozen (infinite T60) network fed full-scale noise
/// never grows and stays finite; from 44.1 to 192 kHz the T60 holds
/// within 0.1 % and the level within 0.01 dB. The modulated reads lose a
/// little treble each pass (Lagrange3), so the top octave decays
/// somewhat faster than the setting: a broadband impulse measures 5 %
/// short, and a frozen loop sheds 1.6 dB over three seconds.
///
/// # Cost
/// `Fdn (8 lines, Householder)` and `Fdn (8 lines, Hadamard)` on the
/// datasheet.
///
/// # Aliasing
/// None (linear; the modulation is a sub-sample, sub-Hz resampling).
///
/// # Valid ranges
/// T60 in `(0, ∞]` seconds (∞ freezes the loop); size in
/// `[0.1, Fdn::MAX_SIZE]`; high-frequency ratio in `(0, 1]`; modulation
/// depth in `[0, 2]` ms. Memory: eight lines of `75·MAX_SIZE + 2` ms.
#[derive(Clone, Debug)]
pub struct Fdn {
    lines: [DelayLine<Lagrange3>; LINES],
    mixing: Mixing,
    t60_s: f32,
    hf_ratio: f32,
    size: f32,
    depth_ms: f32,
    sample_rate: f32,
    delay: [f32; LINES],
    depth: f32,
    gain: [f32; LINES],
    pole: [f32; LINES],
    damped: [f32; LINES],
    lfo: [(f32, f32); LINES],
    lfo_step: [(f32, f32); LINES],
}

impl Default for Fdn {
    fn default() -> Self {
        Self::new(Mixing::default())
    }
}

impl Fdn {
    /// Delay lines in the network.
    pub const LINES: usize = LINES;

    /// The largest [`set_size`](Self::set_size): the lines are allocated
    /// for it.
    pub const MAX_SIZE: f32 = MAX_SIZE;

    /// A network mixing through `mixing`: T60 1.5 s, size 1, no
    /// high-frequency damping, 0.25 ms of modulation. The lines are
    /// allocated in `prepare`.
    pub fn new(mixing: Mixing) -> Self {
        let mut f = Self {
            lines: Default::default(),
            mixing,
            t60_s: 1.5,
            hf_ratio: 1.0,
            size: 1.0,
            depth_ms: 0.25,
            sample_rate: 48_000.0,
            delay: [0.0; LINES],
            depth: 0.0,
            gain: [0.0; LINES],
            pole: [0.0; LINES],
            damped: [0.0; LINES],
            lfo: [(0.0, 1.0); LINES],
            lfo_step: [(0.0, 1.0); LINES],
        };
        f.update();
        f
    }

    /// The time for the tail to fall 60 dB, in seconds; `f32::INFINITY`
    /// freezes it.
    // TODO(U4): take `units::Seconds` once it lands.
    pub fn set_t60_s(&mut self, t60_s: f32) {
        debug_assert!(t60_s > 0.0, "T60 {t60_s} s");
        self.t60_s = t60_s;
        self.update();
    }

    /// The T60 at Nyquist as a fraction of [`set_t60_s`](Self::set_t60_s)'s
    /// (1 = no damping; 0.25 = highs die four times faster).
    pub fn set_hf_ratio(&mut self, ratio: f32) {
        debug_assert!(ratio > 0.0 && ratio <= 1.0, "hf ratio {ratio}");
        self.hf_ratio = ratio.clamp(1e-3, 1.0);
        self.update();
    }

    /// Scale every line length (the room's size), in `[0.1, Fdn::MAX_SIZE]`.
    /// A change jumps the read points: not click-free.
    pub fn set_size(&mut self, size: f32) {
        self.size = size.clamp(0.1, MAX_SIZE);
        self.update();
    }

    /// Peak excursion of each line's read point, in ms (0 = static).
    pub fn set_modulation_ms(&mut self, depth_ms: f32) {
        self.depth_ms = depth_ms.clamp(0.0, 2.0);
        self.update();
    }

    /// The feedback matrix.
    pub fn set_mixing(&mut self, mixing: Mixing) {
        self.mixing = mixing;
    }

    /// Mono in, stereo out (wet only).
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        let mut v = [0.0f32; LINES];
        let (mut left, mut right) = (0.0, 0.0);
        for i in 0..LINES {
            let (s, c) = self.lfo[i];
            let (ds, dc) = self.lfo_step[i];
            // Rotate the LFO phasor; renormalise (first order) so it
            // neither grows nor shrinks.
            let (s, c) = (s * dc + c * ds, c * dc - s * ds);
            let norm = 1.5 - 0.5 * (s * s + c * c);
            self.lfo[i] = (s * norm, c * norm);
            let read = self.lines[i].interpolated(self.delay[i] + self.depth * s);
            let p = self.pole[i];
            self.damped[i] = denormal::flush((1.0 - p) * read + p * self.damped[i]);
            v[i] = self.gain[i] * self.damped[i];
            left += LEFT[i] * read;
            right += RIGHT[i] * read;
        }
        self.mixing.mix(&mut v);
        // Inputs alternate sign so no single matrix eigenvector holds
        // the whole excitation.
        let input = x * (1.0 / (LINES as f32).sqrt());
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.write(v[i] + if i % 2 == 0 { input } else { -input });
            line.advance();
        }
        let out = 1.0 / (LINES as f32).sqrt();
        (left * out, right * out)
    }

    fn update(&mut self) {
        let rate = self.sample_rate;
        self.depth = self.depth_ms * rate / 1000.0;
        for i in 0..LINES {
            self.delay[i] = LENGTHS_MS[i] * self.size * rate / 1000.0;
            let g = loop_gain(self.delay[i], self.t60_s, rate);
            let h = loop_gain(self.delay[i], self.t60_s * self.hf_ratio, rate);
            self.gain[i] = g;
            self.pole[i] = if g + h > 0.0 { (g - h) / (g + h) } else { 0.0 };
            let w = core::f32::consts::TAU * MOD_HZ[i] / rate;
            self.lfo_step[i] = (w.sin(), w.cos());
        }
    }

    fn longest_samples(&self) -> f32 {
        LENGTHS_MS[LINES - 1] * self.size * self.sample_rate / 1000.0 + self.depth
    }
}

impl Stage for Fdn {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        let max_delay = (LENGTHS_MS[LINES - 1] * MAX_SIZE + 2.0) * sample_rate / 1000.0;
        let capacity = max_delay.ceil() as usize + Lagrange3::NEWER + Lagrange3::OLDER + 2;
        for line in &mut self.lines {
            line.allocate(capacity);
        }
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.reset();
            // Spread the LFO phases around the circle.
            let phase = core::f32::consts::TAU * i as f32 / LINES as f32;
            self.lfo[i] = (phase.sin(), phase.cos());
        }
        self.damped = [0.0; LINES];
    }

    /// The mid (`(left + right) / 2`) of [`tick_stereo`](Fdn::tick_stereo).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (l, r) = self.tick_stereo(x);
        0.5 * (l + r)
    }

    fn tail(&self) -> Tail {
        decay_tail(self.t60_s, self.sample_rate, self.longest_samples())
    }
}
