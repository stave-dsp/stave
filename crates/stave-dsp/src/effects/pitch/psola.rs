//! Time-domain pitch-synchronous overlap-add: monophonic pitch shifting
//! that keeps the spectral envelope (formants) in place.

use super::yin::Yin;
use crate::kernel::node::{Stage, Tail};
use crate::units::{Hz, Semitones};

/// A TD-PSOLA pitch shifter for monophonic sources (voice, bass, a
/// solo instrument): grains two periods long, cut pitch-synchronously
/// from the input and laid down at the shifted period.
///
/// # Models
///
/// Time-domain pitch-synchronous overlap-add (Moulines & Charpentier,
/// *Pitch-synchronous waveform processing techniques for text-to-speech
/// synthesis using diphones*, Speech Communication 1990; Moulines &
/// Laroche 1995): the input's period `P` is tracked; analysis marks
/// follow it a period apart; every `P/α` samples of output a Hann grain
/// of `2P` is taken around the analysis mark nearest the corresponding
/// input time and added there. Each grain carries one period's
/// waveform, so the spectral envelope stays (formants do not move) and
/// the fundamental becomes `α·f₀`. Unvoiced input passes through as
/// half-overlapped grains at the input's own positions. No hardware
/// claim.
///
/// # Derivation
///
/// The period by YIN (de Cheveigné & Kawahara 2002: the
/// cumulative-mean-normalised difference, threshold 0.15, parabolic
/// refinement of the raw difference), its difference function by FFT
/// correlation every 5 ms over a window of the longest period; analysis
/// marks on a lattice one tracked period apart, started at the
/// strongest sample of the first voiced period; synthesis marks in
/// `f64`, so the output period is exact on average while grains sit on
/// whole samples.
///
/// # Verified
///
/// `crates/stave-dsp/src/effects/pitch/tests.rs::psola_shifts_pitch_to_the_cent`:
/// band-limited sawtooths at 98, 196 and 392 Hz shifted ±12, +7, −3 and
/// 0 semitones at 44.1, 48 and 96 kHz: the output's fundamental within
/// 0.29 cents of the target (budget 3);
/// `crates/stave-dsp/src/effects/pitch/tests.rs::psola_keeps_the_formant`:
/// a 150 Hz pulse train through a 1 kHz resonance keeps its spectral
/// centroid (400 Hz–2.5 kHz) within 0.5 % shifted ±5 semitones (budget
/// 5 %), where resampling would move it +33 / −25 %.
///
/// # Cost
///
/// Datasheet `"Psola +7 st"` at 48 kHz: three real transforms of 2048
/// every 5 ms for the detector, and about two multiply-adds per output
/// sample for the grains.
///
/// # Aliasing
///
/// None: grains are copied, never resampled.
///
/// # Valid ranges
///
/// Monophonic input with a fundamental between `lowest` and `highest`
/// (default 60 Hz to 1 kHz); polyphonic input is treated as its
/// strongest period and garbles. Shifts within ±24 semitones; large
/// downward shifts leave gaps between grains (one pulse per period, as
/// TD-PSOLA does). Latency `⌈2.5·fs/lowest⌉ + 4` samples (2 004 at
/// 48 kHz), reported.
///
/// ```
/// use stave_dsp::effects::pitch::Psola;
/// use stave_dsp::units::{hz, semitones};
/// use stave_dsp::Stage;
///
/// let mut octave_down = Psola::new(hz(60.0), hz(1_000.0));
/// octave_down.set_shift(semitones(-12.0));
/// octave_down.prepare(48_000.0);
/// assert_eq!(octave_down.latency(), 2_004);
/// ```
#[derive(Clone, Debug)]
pub struct Psola {
    lowest: Hz,
    highest: Hz,
    ratio: f64,
    sample_rate: f32,
    yin: Yin,
    /// Input history, circular.
    input: Vec<f32>,
    /// The output being built, circular.
    output: Vec<f32>,
    ticks: u64,
    latency: usize,
    /// The longest period, samples.
    longest: usize,
    /// Samples between detections, and since the last.
    detect_hop: usize,
    since: usize,
    /// The tracked period, `None` while unvoiced.
    period: Option<f64>,
    /// The next synthesis mark, output samples.
    next_mark: f64,
    /// The last analysis mark, input samples, while voiced.
    lattice: Option<f64>,
    /// Half a grain for unvoiced input.
    unvoiced_half: usize,
    window: Vec<f32>,
}

impl Psola {
    /// A shifter for fundamentals from `lowest` to `highest`, unshifted
    /// until [`set_shift`](Self::set_shift).
    ///
    /// # Panics
    ///
    /// Unless `0 < lowest < highest`.
    pub fn new(lowest: Hz, highest: Hz) -> Self {
        assert!(
            lowest.get() > 0.0 && lowest.get() < highest.get(),
            "Psola: the lowest fundamental must be positive and below the highest"
        );
        let mut s = Self {
            lowest,
            highest,
            ratio: 1.0,
            sample_rate: 0.0,
            yin: Yin::new(2, 4),
            input: Vec::new(),
            output: Vec::new(),
            ticks: 0,
            latency: 0,
            longest: 0,
            detect_hop: 1,
            since: 0,
            period: None,
            next_mark: 0.0,
            lattice: None,
            unvoiced_half: 1,
            window: Vec::new(),
        };
        s.plan(48_000.0);
        s
    }

    /// Moves the fundamental by `shift` (clamped to ±24 semitones).
    /// Realtime: never allocates.
    pub fn set_shift(&mut self, shift: Semitones) {
        self.ratio = (f64::from(shift.get()).clamp(-24.0, 24.0) / 12.0).exp2();
    }

    /// The tracked period of the input, in samples, while voiced.
    pub fn period(&self) -> Option<f64> {
        self.period
    }

    fn plan(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let fs = f64::from(sample_rate);
        self.longest = (fs / f64::from(self.lowest.get())).ceil() as usize;
        let shortest = (fs / f64::from(self.highest.get())).floor().max(2.0) as usize;
        self.yin = Yin::new(shortest, self.longest);
        self.latency = (2.5 * fs / f64::from(self.lowest.get())).ceil() as usize + 4;
        let span = (self.latency + 4 * self.longest + self.yin.segment()).next_power_of_two();
        self.input = vec![0.0; span];
        self.output = vec![0.0; span];
        self.detect_hop = ((0.005 * fs).round() as usize).max(1);
        self.unvoiced_half = ((0.005 * fs).round() as usize).max(2);
        // A Hann table to read grains of any length from.
        self.window = (0..=WINDOW_POINTS)
            .map(|i| {
                let x = i as f64 / WINDOW_POINTS as f64;
                (0.5 - 0.5 * (core::f64::consts::TAU * x).cos()) as f32
            })
            .collect();
        self.clear();
    }

    fn clear(&mut self) {
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.ticks = 0;
        self.since = 0;
        self.period = None;
        self.next_mark = self.latency as f64;
        self.lattice = None;
    }

    /// The Hann window at `x ∈ [0, 1]`.
    #[inline]
    fn hann(&self, x: f64) -> f32 {
        let p = x.clamp(0.0, 1.0) * WINDOW_POINTS as f64;
        let i = (p as usize).min(WINDOW_POINTS - 1);
        let f = (p - i as f64) as f32;
        self.window[i] + (self.window[i + 1] - self.window[i]) * f
    }

    /// Runs the detector over the newest segment.
    fn detect(&mut self) {
        let seg = self.yin.segment();
        let mask = self.input.len() - 1;
        let end = self.ticks as usize + 1;
        let input = &self.input;
        self.period = self.yin.period(|buf| {
            for (k, b) in buf.iter_mut().enumerate() {
                *b = input[(end - seg + k) & mask];
            }
        });
        if self.period.is_none() {
            self.lattice = None;
        }
    }

    /// The strongest sample within half a period of `u`: where the
    /// lattice starts.
    fn epoch_near(&self, u: f64, period: f64) -> f64 {
        let mask = self.input.len() - 1;
        let half = (period / 2.0) as i64;
        let centre = u.round() as i64;
        let best = (centre - half..=centre + half)
            .max_by(|&a, &b| {
                let x = self.input[a as usize & mask].abs();
                let y = self.input[b as usize & mask].abs();
                x.total_cmp(&y)
            })
            .unwrap_or(centre);
        best as f64
    }

    /// Lays down the grain for the synthesis mark at `s`.
    fn place(&mut self, s: f64) {
        let u = s - self.latency as f64;
        let (analysis, half, step) = match self.period {
            Some(p) => {
                let a = match self.lattice {
                    Some(l) => l + ((u - l) / p).round() * p,
                    None => self.epoch_near(u, p),
                };
                self.lattice = Some(a);
                (a, p, p / self.ratio)
            }
            None => {
                let h = self.unvoiced_half as f64;
                (u, h, h)
            }
        };
        let mask = self.input.len() - 1;
        let width = half.round().max(1.0) as i64;
        let (si, ai) = (s.round() as i64, analysis.round() as i64);
        for k in -width..=width {
            let w = self.hann((k + width) as f64 / (2 * width) as f64);
            let x = self.input[(ai + k) as usize & mask];
            self.output[(si + k) as usize & mask] += w * x;
        }
        self.next_mark += step;
    }
}

/// Points in the Hann table.
const WINDOW_POINTS: usize = 1024;

impl Stage for Psola {
    /// Re-plans for the rate (this allocates) and clears the state.
    fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.plan(sample_rate);
        }
        self.clear();
    }

    fn reset(&mut self) {
        self.clear();
    }

    /// Realtime: never allocates; every 5 ms the detector runs three
    /// transforms.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mask = self.input.len() - 1;
        let t = self.ticks;
        self.input[t as usize & mask] = x;
        self.since += 1;
        if self.since >= self.detect_hop && t as usize + 1 >= self.yin.segment() {
            self.since = 0;
            self.detect();
        }
        let lead = (self.longest + 1) as f64;
        while self.next_mark <= t as f64 + lead {
            let s = self.next_mark;
            self.place(s);
        }
        let i = t as usize & mask;
        let y = self.output[i];
        self.output[i] = 0.0;
        self.ticks += 1;
        y
    }

    fn latency(&self) -> u32 {
        self.latency as u32
    }

    /// The latency plus a grain.
    fn tail(&self) -> Tail {
        Tail::samples((self.latency + 2 * self.longest) as u32)
    }
}
