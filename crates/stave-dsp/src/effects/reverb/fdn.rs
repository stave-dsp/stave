//! The feedback delay network: eight modulated delay lines recirculating
//! through an orthogonal matrix.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::{Interpolator, Lagrange3};

use crate::units::Hz;

use super::attenuation::{loop_target, Design, GraphicEq, POINTS};
use super::kronecker::Kronecker;
use super::t60::T60;
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
/// `crates/stave-dsp/src/effects/reverb/tests.rs::the_fdn_matrix_is_unitary`: `AᵀA = I` within
/// 1e-6 for both.
///
/// # Cost
/// Part of the `"Fdn (8 lines, Householder)"` and
/// `"Fdn (8 lines, Hadamard)"` datasheet lines.
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
/// damped, mixed by an orthogonal matrix and fed back, with slowly
/// modulated read points.
///
/// # Models
/// Jot's FDN (Jot & Chaigne 1991): `s[n+1] = A·Γ(s[n − m])`, `A`
/// orthogonal, `Γ` a per-line absorption filter. No hardware claim; the
/// network is wet only (mix dry yourself). Two absorptions:
///
/// - [`set_t60_s`](Self::set_t60_s) and [`set_hf_ratio`](Self::set_hf_ratio):
///   a gain and a one-pole shelf per line (Jot 1992), a T60 at DC and one
///   at Nyquist, cheapest.
/// - [`set_t60`](Self::set_t60): a [`T60`] per octave band, held by a
///   graphic-EQ attenuation filter per line (Schlecht & Habets 2017;
///   Prawda, Schlecht & Välimäki 2019): a broadband gain, ten octave
///   peak-notch sections and a high shelf.
///
/// The matrix is a fixed [`Mixing`] or a parametric [`Kronecker`]
/// (Coppola 2026) whose angles can be set or modulated continuously;
/// either is orthogonal, so the decay is the absorption's alone.
/// [`set_freeze`](Self::set_freeze) makes the loop exactly lossless.
///
/// # Derivation
/// Line `i` of length `mᵢ` samples falls `mᵢ·60/(fs·T60(ω))` dB per pass
/// (Jot's condition: the whole loop then falls 60 dB in T60 at every
/// frequency, whatever the line). The shelf form: gain `gᵢ` and one-pole
/// `gᵢ(1 − bᵢ)/(1 − bᵢz⁻¹)`, `bᵢ = (gᵢ − hᵢ)/(gᵢ + hᵢ)`, `hᵢ` the gain at
/// the high-frequency T60. The graphic-EQ form fits that target per line
/// at the octave centres and midpoints (least squares through the peak
/// filters' interaction matrix, then refined against the real cascade),
/// less the mean treble loss of the line's [`Lagrange3`] read over its
/// modulation (the interpolator's magnitude response at its fractional
/// delays: a characterised loss, not a fitted one). Lengths are in
/// milliseconds, read fractionally, so the network is the same in
/// seconds at every rate; each read point wanders ±`depth` with its own
/// LFO (Dattorro 1997, §3). Modulated Kronecker angles move every 16
/// samples.
///
/// Freeze mutes the input, holds each read point at its nearest whole
/// sample (no interpolation), stops the LFOs and bypasses the absorption:
/// with an orthogonal matrix the energy stored in the lines is then
/// invariant, sample by sample, and stays so while the Kronecker angles
/// move ([`stored_energy`](Self::stored_energy)).
///
/// # Verified
/// In `crates/stave-dsp/src/effects/reverb/tests.rs`:
/// `measured_t60_matches_the_setting`, `the_fdn_matrix_is_unitary`,
/// `stable_at_maximum_decay` and `sample_rate_invariant` (T60 as
/// Schroeder T30 around 1 kHz, ISO 3382): within 3 % of the setting at 1
/// and 2.5 s; the matrix is unitary within 1e-6; a network at infinite
/// T60 fed full-scale noise never grows; from 44.1 to 192 kHz the T60
/// holds within 0.1 % and the level within 0.01 dB. The shelf form's
/// modulated reads lose a little treble each pass, so a broadband
/// impulse measures 5 % short.
///
/// `t60_per_octave_band_matches_the_profile`: the graphic-EQ form's
/// measured T30 per octave band (63 Hz – 8 kHz) within 5 % of the profile
/// (the T60 just-noticeable difference of ISO 3382-1, Prawda et al.'s
/// budget) for a real hall and a studio room, modulated and static, at
/// 48 and 96 kHz: worst 3.9 %. The filter itself meets its target within
/// 2 % at every band centre (`attenuation.rs`,
/// `the_fit_meets_the_target_at_every_control_point`: worst 1.4 %, and
/// 0.65 % at 16 kHz). `freeze_preserves_energy`: frozen, the stored
/// energy holds within 1e-4 over ten seconds (worst 1.5e-5, the `f32`
/// matrix's rounding), with the Kronecker angles swept and without.
/// `modulating_the_matrix_keeps_the_decay`: angle modulation moves the
/// per-band T60 by under 3 % (worst 2.4 %, at 63 Hz, where few modes
/// beat).
///
/// # Cost
/// `"Fdn (8 lines, Householder)"`, `"Fdn (8 lines, Hadamard)"`,
/// `"Fdn (8 lines, graphic EQ)"` and
/// `"Fdn (8 lines, Kronecker, modulated)"` on the datasheet.
///
/// # Aliasing
/// None (linear; the modulation is a sub-sample, sub-Hz resampling).
///
/// # Valid ranges
/// T60 in `(0, ∞]` seconds; the graphic-EQ form holds per-pass
/// attenuations up to about 40 dB (a T60 down to 0.1 s at size 1) and
/// sample rates from 44.1 kHz (below, bands above 0.45·fs stay flat).
/// Size in `[0.1, Fdn::MAX_SIZE]`; high-frequency ratio in `(0, 1]`;
/// modulation depth in `[0, 2]` ms; angles any finite radians. Memory:
/// eight lines of `75·MAX_SIZE + 2` ms. A design (`set_t60`, and in that
/// form `set_size` and `set_modulation_ms`) allocates nothing.
#[derive(Clone, Debug)]
pub struct Fdn {
    lines: [DelayLine<Lagrange3>; LINES],
    mixing: Mixing,
    kronecker: Option<Kronecker>,
    t60_s: f32,
    profile: Option<T60>,
    hf_ratio: f32,
    size: f32,
    depth_ms: f32,
    sample_rate: f32,
    delay: [f32; LINES],
    depth: f32,
    gain: [f32; LINES],
    pole: [f32; LINES],
    damped: [f32; LINES],
    design: Design,
    eq: [GraphicEq; LINES],
    lfo: [(f32, f32); LINES],
    lfo_step: [(f32, f32); LINES],
    /// Kronecker angle modulation: depth (radians), the base angles, the
    /// LFO phases (cycles) and their per-update steps.
    angle_depth: f32,
    angle_base: [f32; Kronecker::LEVELS],
    angle_phase: [f32; Kronecker::LEVELS],
    angle_step: [f32; Kronecker::LEVELS],
    angle_rate: Hz,
    countdown: u32,
    frozen: bool,
    /// The whole-sample read points while frozen.
    held: [usize; LINES],
}

/// Samples between Kronecker angle updates while modulating.
const ANGLE_UPDATE: u32 = 16;

/// Each level's share of the angle LFO rate: unrelated, so the levels
/// never move together.
const ANGLE_RATIOS: [f32; Kronecker::LEVELS] = [1.0, 1.37, 1.73];

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
            kronecker: None,
            t60_s: 1.5,
            profile: None,
            hf_ratio: 1.0,
            size: 1.0,
            depth_ms: 0.25,
            sample_rate: 48_000.0,
            delay: [0.0; LINES],
            depth: 0.0,
            gain: [0.0; LINES],
            pole: [0.0; LINES],
            damped: [0.0; LINES],
            design: Design::new(48_000.0),
            eq: Default::default(),
            lfo: [(0.0, 1.0); LINES],
            lfo_step: [(0.0, 1.0); LINES],
            angle_depth: 0.0,
            angle_base: [0.0; Kronecker::LEVELS],
            angle_phase: [0.0, 0.33, 0.67],
            angle_step: [0.0; Kronecker::LEVELS],
            angle_rate: Hz::new(0.0),
            countdown: 0,
            frozen: false,
            held: [0; LINES],
        };
        f.update();
        f
    }

    /// The time for the tail to fall 60 dB, in seconds, the same in every
    /// band (the gain-and-shelf absorption, with
    /// [`set_hf_ratio`](Self::set_hf_ratio)); `f32::INFINITY` never
    /// decays. Replaces a [`set_t60`](Self::set_t60) profile.
    // TODO(U4): take `units::Seconds` once it lands.
    pub fn set_t60_s(&mut self, t60_s: f32) {
        debug_assert!(t60_s > 0.0, "T60 {t60_s} s");
        self.t60_s = t60_s;
        self.profile = None;
        self.update();
    }

    /// A reverberation time per octave band, held by a graphic-EQ
    /// attenuation filter in every line ([`set_hf_ratio`](Self::set_hf_ratio)
    /// does not apply). Allocation-free.
    pub fn set_t60(&mut self, profile: T60) {
        self.profile = Some(profile);
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

    /// A fixed feedback matrix (replaces a [`Kronecker`]).
    pub fn set_mixing(&mut self, mixing: Mixing) {
        self.mixing = mixing;
        self.kronecker = None;
    }

    /// Mix through a parametric [`Kronecker`] matrix; its angles are the
    /// base the [`set_angle_modulation`](Self::set_angle_modulation) LFOs
    /// swing around. Any angle keeps the loop lossless.
    pub fn set_kronecker(&mut self, matrix: Kronecker) {
        for level in 0..Kronecker::LEVELS {
            self.angle_base[level] = matrix.angle(level);
        }
        self.kronecker = Some(matrix);
    }

    /// The Kronecker matrix the network mixes through, when it does
    /// (its angles as the modulation last left them).
    pub fn kronecker(&self) -> Option<&Kronecker> {
        self.kronecker.as_ref()
    }

    /// Swing every Kronecker angle ±`depth_rad` radians around its base,
    /// level 1 at `rate`, the others at unrelated multiples of it
    /// (0 depth = static). Moving the matrix spreads its modes (Schlecht
    /// & Habets 2015) without touching the decay.
    pub fn set_angle_modulation(&mut self, depth_rad: f32, rate: Hz) {
        self.angle_depth = depth_rad.max(0.0);
        self.angle_rate = rate;
        self.update();
    }

    /// Freeze: the input is muted and the loop made exactly lossless
    /// (whole-sample reads, no absorption, LFOs held), so the tail
    /// sustains; releasing resumes the decay from where it stands.
    pub fn set_freeze(&mut self, frozen: bool) {
        if frozen && !self.frozen {
            for i in 0..LINES {
                let read = self.delay[i] + self.depth * self.lfo[i].0;
                let hi = self.lines[i].capacity().saturating_sub(1).max(1);
                self.held[i] = (read.round().max(1.0) as usize).min(hi);
            }
        }
        self.frozen = frozen;
    }

    /// Whether the network is frozen.
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// The energy circulating in the lines: the sum of squares of every
    /// sample written and not yet read. Exact, and invariant, while
    /// frozen; otherwise taken at the unmodulated read points.
    pub fn stored_energy(&self) -> f64 {
        (0..LINES)
            .map(|i| {
                let d = if self.frozen {
                    self.held[i]
                } else {
                    self.delay[i].round() as usize
                };
                (1..=d)
                    .map(|k| f64::from(self.lines[i].read_int(k)).powi(2))
                    .sum::<f64>()
            })
            .sum()
    }

    /// Mono in, stereo out (wet only).
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        let mut v = [0.0f32; LINES];
        let (mut left, mut right) = (0.0, 0.0);
        let graphic = self.profile.is_some();
        for i in 0..LINES {
            let read = if self.frozen {
                self.lines[i].read_int(self.held[i])
            } else {
                let (s, c) = self.lfo[i];
                let (ds, dc) = self.lfo_step[i];
                // Rotate the LFO phasor; renormalise (first order) so it
                // neither grows nor shrinks.
                let (s, c) = (s * dc + c * ds, c * dc - s * ds);
                let norm = 1.5 - 0.5 * (s * s + c * c);
                self.lfo[i] = (s * norm, c * norm);
                self.lines[i].interpolated(self.delay[i] + self.depth * s)
            };
            // The absorption runs while frozen too, so its state is
            // current when the freeze lets go.
            let absorbed = if graphic {
                self.eq[i].tick(read)
            } else {
                let p = self.pole[i];
                self.damped[i] = denormal::flush((1.0 - p) * read + p * self.damped[i]);
                self.gain[i] * self.damped[i]
            };
            v[i] = if self.frozen { read } else { absorbed };
            left += LEFT[i] * read;
            right += RIGHT[i] * read;
        }
        self.mix(&mut v);
        // Inputs alternate sign so no single matrix eigenvector holds
        // the whole excitation.
        let input = if self.frozen {
            0.0
        } else {
            x * (1.0 / (LINES as f32).sqrt())
        };
        for (i, line) in self.lines.iter_mut().enumerate() {
            line.write(v[i] + if i % 2 == 0 { input } else { -input });
            line.advance();
        }
        let out = 1.0 / (LINES as f32).sqrt();
        (left * out, right * out)
    }

    #[inline]
    fn mix(&mut self, v: &mut [f32; LINES]) {
        let Some(matrix) = self.kronecker.as_mut() else {
            self.mixing.mix(v);
            return;
        };
        if self.angle_depth > 0.0 {
            if self.countdown == 0 {
                self.countdown = ANGLE_UPDATE;
                for level in 0..Kronecker::LEVELS {
                    let phase = (self.angle_phase[level] + self.angle_step[level]).fract();
                    self.angle_phase[level] = phase;
                    let swing = (core::f32::consts::TAU * phase).sin();
                    matrix.set_angle(level, self.angle_base[level] + self.angle_depth * swing);
                }
            }
            self.countdown -= 1;
        }
        matrix.mix(v);
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
        for level in 0..Kronecker::LEVELS {
            self.angle_step[level] =
                self.angle_rate.get() * ANGLE_RATIOS[level] * ANGLE_UPDATE as f32 / rate;
        }
        if let Some(profile) = self.profile {
            let bands = profile.seconds();
            for i in 0..LINES {
                let mut target = loop_target(&bands, f64::from(self.delay[i]), rate);
                let loss = read_loss_db(self.design.points(), self.delay[i], self.depth);
                for (t, l) in target.iter_mut().zip(loss) {
                    // The read already loses `l` (≤ 0): the filter
                    // attenuates that much less, never boosting.
                    *t = (*t - l).min(0.0);
                }
                self.design.fit(&target, &mut self.eq[i]);
            }
        }
    }

    fn longest_samples(&self) -> f32 {
        LENGTHS_MS[LINES - 1] * self.size * self.sample_rate / 1000.0 + self.depth
    }

    fn longest_t60(&self) -> f32 {
        match self.profile {
            Some(p) => p.longest().get() as f32,
            None => self.t60_s,
        }
    }
}

/// The mean dB gain (≤ 0) of a [`Lagrange3`] read at `delay` samples
/// swinging ±`depth`, at each control point: the exact fractional delay
/// when static; the mean over eight evenly spaced fractions when it
/// moves (its fractional part sweeps them all).
fn read_loss_db(points: &[f64; POINTS], delay: f32, depth: f32) -> [f64; POINTS] {
    const SWEPT: [f32; 8] = [
        0.0625, 0.1875, 0.3125, 0.4375, 0.5625, 0.6875, 0.8125, 0.9375,
    ];
    let fixed = [delay - delay.floor()];
    let fracs: &[f32] = if depth > 0.0 { &SWEPT } else { &fixed };
    let mut loss = [0.0f64; POINTS];
    for (l, &w) in loss.iter_mut().zip(points) {
        let mut sum = 0.0;
        for &frac in fracs {
            let h = Lagrange3::weights(frac);
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (j, &hj) in h.iter().enumerate() {
                re += f64::from(hj) * (w * j as f64).cos();
                im -= f64::from(hj) * (w * j as f64).sin();
            }
            sum += 10.0 * (re * re + im * im).max(1e-300).log10();
        }
        *l = (sum / fracs.len() as f64).min(0.0);
    }
    loss
}

impl Stage for Fdn {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        if sample_rate != self.sample_rate {
            self.design = Design::new(sample_rate);
        }
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
        for eq in &mut self.eq {
            eq.reset();
        }
        if self.frozen {
            // Re-take the held read points from the reset LFOs.
            self.frozen = false;
            self.set_freeze(true);
        }
    }

    /// The mid (`(left + right) / 2`) of [`tick_stereo`](Fdn::tick_stereo).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (l, r) = self.tick_stereo(x);
        0.5 * (l + r)
    }

    fn tail(&self) -> Tail {
        if self.frozen {
            return Tail::Infinite;
        }
        decay_tail(self.longest_t60(), self.sample_rate, self.longest_samples())
    }
}
