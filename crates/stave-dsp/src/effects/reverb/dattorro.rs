//! Dattorro's plate: four input diffusers into a figure-eight tank of
//! modulated allpasses and delays.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::time::delay::DelayLine;
use crate::primitives::time::interpolators::Lagrange3;

use super::decay_tail;

/// The rate Dattorro's lengths are given at (Hz).
const DESIGN_RATE: f32 = 29_761.0;

/// Input diffusers: (length, coefficient) at the design rate.
const DIFFUSERS: [(f32, f32); 4] = [(142.0, 0.75), (107.0, 0.75), (379.0, 0.625), (277.0, 0.625)];

/// One tank half at the design rate: modulated allpass, delay, allpass,
/// delay.
struct HalfLengths {
    modulated: f32,
    first: f32,
    allpass: f32,
    second: f32,
}

const HALVES: [HalfLengths; 2] = [
    HalfLengths {
        modulated: 672.0,
        first: 4_453.0,
        allpass: 1_800.0,
        second: 3_720.0,
    },
    HalfLengths {
        modulated: 908.0,
        first: 4_217.0,
        allpass: 2_656.0,
        second: 3_163.0,
    },
];

/// The modulated allpasses' peak excursion at the design rate (samples).
const EXCURSION: f32 = 16.0;
/// Their LFO rate (Hz).
const MOD_HZ: f32 = 1.0;
/// Decay diffusion 1 (the modulated allpasses, sign flipped) and 2.
const DECAY_DIFFUSION: [f32; 2] = [-0.70, 0.50];
/// The output taps, Dattorro's Table 2: (tank half, node, position at
/// the design rate, sign). Nodes: 0 first delay, 1 allpass, 2 second
/// delay.
const TAPS: [[(usize, usize, f32, f32); 7]; 2] = [
    [
        (1, 0, 266.0, 1.0),
        (1, 0, 2_974.0, 1.0),
        (1, 1, 1_913.0, -1.0),
        (1, 2, 1_996.0, 1.0),
        (0, 0, 1_990.0, -1.0),
        (0, 1, 187.0, -1.0),
        (0, 2, 1_066.0, -1.0),
    ],
    [
        (0, 0, 353.0, 1.0),
        (0, 0, 3_627.0, 1.0),
        (0, 1, 1_228.0, -1.0),
        (0, 2, 2_673.0, 1.0),
        (1, 0, 2_111.0, -1.0),
        (1, 1, 335.0, -1.0),
        (1, 2, 121.0, -1.0),
    ],
];
/// Dattorro's output gain.
const OUTPUT_GAIN: f32 = 0.6;

/// A Schroeder allpass on a line of whole samples:
/// `w = x + g·line[m]`, `y = line[m] − g·w`.
#[derive(Clone, Debug, Default)]
struct Allpass {
    line: DelayLine,
    length: usize,
    gain: f32,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let delayed = self.line.read_int(self.length);
        let w = denormal::flush(x + self.gain * delayed);
        self.line.write(w);
        self.line.advance();
        delayed - self.gain * w
    }
}

/// One half of the tank.
#[derive(Clone, Debug, Default)]
struct Half {
    modulated: DelayLine<Lagrange3>,
    modulated_length: f32,
    first: DelayLine,
    first_length: usize,
    damping: f32,
    allpass: Allpass,
    second: DelayLine,
    second_length: usize,
}

impl Half {
    /// Run the half one sample; returns its end (the second delay's
    /// output), before the cross-feed gain.
    #[inline]
    fn tick(&mut self, x: f32, excursion: f32, damping: f32, decay: f32) -> f32 {
        // Modulated allpass (coefficient decay diffusion 1).
        let g = DECAY_DIFFUSION[0];
        let delayed = self.modulated.interpolated(self.modulated_length + excursion);
        let w = denormal::flush(x + g * delayed);
        self.modulated.write(w);
        self.modulated.advance();
        let a = delayed - g * w;
        // First delay, damping, decay.
        let d1 = self.first.read_int(self.first_length);
        self.first.write(a);
        self.first.advance();
        self.damping = denormal::flush(d1 + (self.damping - d1) * damping);
        let b = self.allpass.tick(self.damping * decay);
        let d2 = self.second.read_int(self.second_length);
        self.second.write(b);
        self.second.advance();
        d2
    }

    /// The signal `position` samples into node `node`.
    #[inline]
    fn tap(&self, node: usize, position: usize) -> f32 {
        match node {
            0 => self.first.read_int(position),
            1 => self.allpass.line.read_int(position),
            _ => self.second.read_int(position),
        }
    }
}

/// Dattorro's plate reverberator: band-limited, predelayed input through
/// four diffusers into a two-half tank whose modulated allpasses keep
/// the tail from ringing, with seven output taps per channel.
///
/// # Models
/// The plate-class reverb of Dattorro, "Effect Design Part 1: Reverberator
/// and Other Filters", JAES 45(9), 1997, Fig. 1 and Tables 1–2 (after
/// Griesinger). An algorithmic design: no plate is modelled (see
/// `physical` for that). Wet only.
///
/// # Derivation
/// Every length is Dattorro's at 29 761 Hz scaled to the prepared rate
/// (whole samples, the modulated allpasses fractional through
/// [`Lagrange3`]). The decay gain, applied four times per trip round the
/// tank, is solved from T60 by energy renewal through the tank's
/// delays and allpasses (an allpass spreads a pass's energy over
/// `M, 2M, …`, which lengthens the asymptotic decay beyond what the
/// summed lengths predict — 13 % at 1 s). Decay diffusion 2 is held at
/// 0.5 rather than tied to the decay, so the T60 is the setting. Damping and input bandwidth are
/// one-pole corners in Hz (Dattorro's per-sample coefficients, made
/// rate-independent).
///
/// # Verified
/// `crates/stave-dsp/src/effects/reverb/tests.rs::measured_t60_matches_the_setting`,
/// `crates/stave-dsp/src/effects/reverb/tests.rs::stable_at_maximum_decay` and
/// `crates/stave-dsp/src/effects/reverb/tests.rs::sample_rate_invariant` (T60 as Schroeder T30 around 1 kHz): within
/// 3 % of the setting at 1 and 2.5 s (measured −0.9 % and −1.6 %; the
/// summed-length formula this replaced measured +13 %); stable and
/// finite at infinite T60 under full-scale noise; from 44.1 to 192 kHz
/// the T60 holds within 0.6 % and the level within 0.06 dB.
///
/// # Cost
/// `"Dattorro (plate)"` on the datasheet.
///
/// # Aliasing
/// None (linear; sub-sample modulation at 1 Hz).
///
/// # Valid ranges
/// T60 in `(0, ∞]` s, but no shorter than the tank allpasses' own ring
/// (the −0.7 allpass of 908 samples at 29 761 Hz falls 60 dB in about
/// 0.6 s; below that the decay gain is 0 and the tail is that ring);
/// predelay in `[0, max_predelay_ms]`; damping and
/// bandwidth corners in `(0, 0.49·sample_rate]` Hz.
#[derive(Clone, Debug)]
pub struct Dattorro {
    predelay: DelayLine,
    max_predelay_ms: f32,
    predelay_ms: f32,
    predelay_samples: usize,
    bandwidth_hz: f32,
    bandwidth: f32,
    bandwidth_state: f32,
    diffusers: [Allpass; 4],
    halves: [Half; 2],
    damping_hz: f32,
    damping: f32,
    t60_s: f32,
    decay: f32,
    sample_rate: f32,
    scale: f32,
    excursion: f32,
    lfo: (f32, f32),
    lfo_step: (f32, f32),
    feedback: [f32; 2],
    taps: [[(usize, usize, usize, f32); 7]; 2],
}

impl Default for Dattorro {
    fn default() -> Self {
        Self::new(100.0)
    }
}

impl Dattorro {
    /// A plate that can predelay up to `max_predelay_ms`: T60 2 s, no
    /// predelay, damping at 8 kHz, bandwidth at 16 kHz. Lines are
    /// allocated in `prepare`.
    pub fn new(max_predelay_ms: f32) -> Self {
        let mut d = Self {
            predelay: DelayLine::new(),
            max_predelay_ms: max_predelay_ms.max(0.0),
            predelay_ms: 0.0,
            predelay_samples: 0,
            bandwidth_hz: 16_000.0,
            bandwidth: 1.0,
            bandwidth_state: 0.0,
            diffusers: Default::default(),
            halves: Default::default(),
            damping_hz: 8_000.0,
            damping: 0.0,
            t60_s: 2.0,
            decay: 0.0,
            sample_rate: 48_000.0,
            scale: 1.0,
            excursion: 0.0,
            lfo: (0.0, 1.0),
            lfo_step: (0.0, 1.0),
            feedback: [0.0; 2],
            taps: [[(0, 0, 0, 0.0); 7]; 2],
        };
        d.update();
        d
    }

    /// The time for the tail to fall 60 dB, in seconds; `f32::INFINITY`
    /// freezes it.
    // TODO(U4): take `units::Seconds` once it lands.
    pub fn set_t60_s(&mut self, t60_s: f32) {
        debug_assert!(t60_s > 0.0, "T60 {t60_s} s");
        self.t60_s = t60_s;
        self.update();
    }

    /// Delay before the plate, in ms, within `[0, max_predelay_ms]`.
    pub fn set_predelay_ms(&mut self, ms: f32) {
        self.predelay_ms = ms.clamp(0.0, self.max_predelay_ms);
        self.update();
    }

    /// The tank's high-frequency loss: a one-pole lowpass corner in Hz
    /// (lower is darker).
    pub fn set_damping_hz(&mut self, hz: f32) {
        debug_assert!(hz > 0.0, "damping corner {hz} Hz");
        self.damping_hz = hz;
        self.update();
    }

    /// The input's bandwidth: a one-pole lowpass corner in Hz.
    pub fn set_bandwidth_hz(&mut self, hz: f32) {
        debug_assert!(hz > 0.0, "bandwidth corner {hz} Hz");
        self.bandwidth_hz = hz;
        self.update();
    }

    /// Mono in, stereo out (wet only).
    #[inline]
    pub fn tick_stereo(&mut self, x: f32) -> (f32, f32) {
        self.predelay.write(x);
        let delayed = self.predelay.read_int(self.predelay_samples);
        self.predelay.advance();
        self.bandwidth_state = denormal::flush(
            self.bandwidth_state + (delayed - self.bandwidth_state) * self.bandwidth,
        );
        let mut diffused = self.bandwidth_state;
        for d in &mut self.diffusers {
            diffused = d.tick(diffused);
        }
        let (s, c) = self.lfo;
        let (ds, dc) = self.lfo_step;
        let (s, c) = (s * dc + c * ds, c * dc - s * ds);
        let norm = 1.5 - 0.5 * (s * s + c * c);
        self.lfo = (s * norm, c * norm);
        let excursions = [self.excursion * s, self.excursion * c];
        let (damping, decay) = (1.0 - self.damping, self.decay);
        let ends = [
            self.halves[0].tick(diffused + self.feedback[1], excursions[0], damping, decay),
            self.halves[1].tick(diffused + self.feedback[0], excursions[1], damping, decay),
        ];
        self.feedback = [ends[0] * decay, ends[1] * decay];
        let mut out = [0.0f32; 2];
        for (channel, taps) in self.taps.iter().enumerate() {
            for &(half, node, position, sign) in taps {
                out[channel] += sign * self.halves[half].tap(node, position);
            }
        }
        (out[0] * OUTPUT_GAIN, out[1] * OUTPUT_GAIN)
    }

    /// The step `1 − e^(−2π·hz/sample_rate)` of a one-pole lowpass.
    fn one_pole(&self, hz: f32) -> f32 {
        let hz = hz.clamp(1.0, 0.49 * self.sample_rate);
        1.0 - (-core::f32::consts::TAU * hz / self.sample_rate).exp()
    }

    fn update(&mut self) {
        let rate = self.sample_rate;
        self.scale = rate / DESIGN_RATE;
        let whole = |n: f32, scale: f32| ((n * scale).round() as usize).max(1);
        self.predelay_samples = (self.predelay_ms * rate / 1000.0).round() as usize;
        self.bandwidth = self.one_pole(self.bandwidth_hz);
        self.damping = self.one_pole(self.damping_hz);
        for (d, &(length, gain)) in self.diffusers.iter_mut().zip(&DIFFUSERS) {
            d.length = whole(length, self.scale);
            d.gain = gain;
        }
        for (half, lengths) in self.halves.iter_mut().zip(&HALVES) {
            half.modulated_length = lengths.modulated * self.scale;
            half.first_length = whole(lengths.first, self.scale);
            half.allpass.length = whole(lengths.allpass, self.scale);
            half.allpass.gain = DECAY_DIFFUSION[1];
            half.second_length = whole(lengths.second, self.scale);
        }
        self.decay = self.tank_decay();
        self.excursion = EXCURSION * self.scale;
        let w = core::f32::consts::TAU * MOD_HZ / rate;
        self.lfo_step = (w.sin(), w.cos());
        for (channel, taps) in TAPS.iter().enumerate() {
            for (k, &(half, node, position, sign)) in taps.iter().enumerate() {
                self.taps[channel][k] = (half, node, whole(position, self.scale), sign);
            }
        }
    }

    /// The per-pass decay gain that makes the tank's energy fall 60 dB in
    /// T60. A trip round the tank passes four decay gains, four fixed
    /// delays and four allpasses; with energy adding incoherently (a
    /// dense tail), the asymptotic rate `α` (per sample, of energy)
    /// solves `d⁸ · e^(αD) · Π Aᵢ(α) = 1`, where an allpass of `M`
    /// samples and gain `g` holds its energy for `M, 2M, …` with weights
    /// `g², (1 − g²)², (1 − g²)²g², …`, so
    /// `A(α) = g² + (1 − g²)² e^(αM) / (1 − g² e^(αM))`. When
    /// `g² e^(αM) ≥ 1` the allpass alone rings longer than T60: the
    /// decay is 0 and the tail is that ring (see Valid ranges).
    fn tank_decay(&self) -> f32 {
        if self.t60_s.is_infinite() {
            return 1.0;
        }
        if self.t60_s.is_nan() || self.t60_s <= 0.0 {
            return 0.0;
        }
        let alpha = 1e6f64.ln() / (f64::from(self.t60_s) * f64::from(self.sample_rate));
        let mut log_f = 0.0f64;
        for half in &self.halves {
            log_f += alpha * (half.first_length + half.second_length) as f64;
            for (m, g) in [
                (f64::from(half.modulated_length), f64::from(DECAY_DIFFUSION[0])),
                (half.allpass.length as f64, f64::from(half.allpass.gain)),
            ] {
                let (g2, grow) = (g * g, (alpha * m).exp());
                if g2 * grow >= 1.0 {
                    return 0.0;
                }
                log_f += (g2 + (1.0 - g2).powi(2) * grow / (1.0 - g2 * grow)).ln();
            }
        }
        (-log_f / 8.0).exp() as f32
    }

    fn longest_samples(&self) -> f32 {
        let tank: usize = self
            .halves
            .iter()
            .map(|h| h.first_length + h.allpass.length + h.second_length)
            .sum();
        let diffusers: usize = self.diffusers.iter().map(|d| d.length).sum();
        (self.predelay_samples + tank + diffusers) as f32
    }
}

impl Stage for Dattorro {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.update();
        let lines = |n: usize| n + 2;
        self.predelay
            .allocate(lines((self.max_predelay_ms * sample_rate / 1000.0).ceil() as usize));
        for d in &mut self.diffusers {
            d.line.allocate(lines(d.length));
        }
        let excursion = self.excursion.ceil() as usize;
        for half in &mut self.halves {
            half.modulated
                .allocate(lines(half.modulated_length.ceil() as usize + excursion + 4));
            half.first.allocate(lines(half.first_length));
            half.allpass.line.allocate(lines(half.allpass.length));
            half.second.allocate(lines(half.second_length));
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.predelay.reset();
        self.bandwidth_state = 0.0;
        for d in &mut self.diffusers {
            d.line.reset();
        }
        for half in &mut self.halves {
            half.modulated.reset();
            half.first.reset();
            half.allpass.line.reset();
            half.second.reset();
            half.damping = 0.0;
        }
        self.feedback = [0.0; 2];
        self.lfo = (0.0, 1.0);
    }

    /// The mid (`(left + right) / 2`) of
    /// [`tick_stereo`](Dattorro::tick_stereo).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let (l, r) = self.tick_stereo(x);
        0.5 * (l + r)
    }

    fn tail(&self) -> Tail {
        decay_tail(self.t60_s, self.sample_rate, self.longest_samples())
    }
}
