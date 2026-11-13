//! [`Wavetable`]: one cycle of any waveform, played band-limited by
//! mip-mapping: a table per octave, each holding only the harmonics that
//! stay below Nyquist for every pitch it serves.

use std::sync::Arc;

use super::Waveform;
use crate::kernel::node::{Generator, Stage, Tail};
use crate::primitives::spectral::{Complex, Fft};

/// Points per table. Every level is the same length, so the highest
/// harmonic any level holds sits at a quarter of its points (four
/// points per cycle) and the cubic interpolation's images stay as far
/// down at every pitch.
const SIZE: usize = 4096;
/// Harmonics in the first level; level `l` holds `TOP >> l`.
const TOP: usize = SIZE / 4;
/// Levels: 1024, 512, … 1 harmonics.
const LEVELS: usize = 11;
/// One level as stored: a point before and two after the cycle, so the
/// four-point interpolation never wraps.
const STRIDE: usize = SIZE + 3;

/// A band-limited table oscillator: a generator [`Stage`] (its input is
/// ignored) over one cycle, with the mip-mapped tables shared between
/// clones (a voice pool clones one for the price of its phase).
///
/// # Models
///
/// A periodic waveform given by one cycle (or a [`Waveform`]), played
/// at any pitch from tables band-limited per octave: the level a pitch
/// reads holds only the harmonics below Nyquist at that pitch.
///
/// # Derivation
///
/// The cycle's Fourier series (a direct DFT of the given cycle, or the
/// waveform's analytic series), truncated per octave and synthesised by
/// the inverse FFT into 4096-point tables; played with four-point cubic
/// (Catmull-Rom) interpolation (the mip-mapped wavetable of Massie,
/// "Wavetable sampling synthesis", in Kahrs & Brandenburg, *Applications
/// of DSP to Audio and Acoustics*, 1998).
///
/// # Verified
///
/// `crates/stave-dsp/tests/aliasing_budgets.rs::oscillators_hold_their_alias_budgets`
/// (a saw table's worst inharmonic energy over a stepped tone, held to
/// its budget beside its measurement),
/// `crates/stave-dsp/src/primitives/osc/wavetable.rs::a_waveform_table_is_its_fourier_series`
/// and `crates/stave-dsp/src/primitives/osc/wavetable.rs::levels_stay_below_nyquist`.
///
/// # Cost
///
/// Datasheet `"Wavetable (saw)"`. Construction runs eleven 4096-point
/// inverse FFTs and allocates 180 kB, off the audio thread.
///
/// # Aliasing
///
/// None from the waveform: each level stops below Nyquist. What remains
/// is the interpolation's images, far down (see Verified). The price is
/// the octave: within one, the top harmonic falls from Nyquist to half
/// of it as the pitch rises toward the next level.
///
/// # Valid ranges
///
/// `0 ≤ hz < sample_rate / 2`. Below `sample_rate / 2048` (23 Hz at
/// 48 kHz) the first level's 1024 harmonics stop short of Nyquist. A
/// cycle of any length ≥ 2; its harmonics above 1024 are dropped.
#[derive(Clone)]
pub struct Wavetable {
    tables: Arc<[f32]>,
    hz: f32,
    sample_rate: f32,
    phase: f64,
    inc: f64,
    level: usize,
}

impl Wavetable {
    /// The table of a waveform, from its exact Fourier series.
    pub fn new(waveform: Waveform) -> Self {
        let pi = core::f64::consts::PI;
        // (cos, sin) amplitudes of harmonic h.
        let series = |h: usize| -> (f64, f64) {
            let k = h as f64;
            match waveform {
                Waveform::Sine => (0.0, if h == 1 { 1.0 } else { 0.0 }),
                Waveform::Saw => (0.0, -2.0 / (pi * k)),
                Waveform::Square => (0.0, if h % 2 == 1 { 4.0 / (pi * k) } else { 0.0 }),
                Waveform::Triangle => (
                    if h % 2 == 1 {
                        -8.0 / (pi * pi * k * k)
                    } else {
                        0.0
                    },
                    0.0,
                ),
            }
        };
        Self::from_series((1..=TOP).map(series).collect())
    }

    /// The table of one cycle of samples, any length ≥ 2 (its DC is
    /// dropped; harmonics above 1024 too).
    pub fn from_cycle(cycle: &[f32]) -> Self {
        assert!(
            cycle.len() >= 2,
            "Wavetable::from_cycle: a cycle needs two samples"
        );
        let n = cycle.len();
        let top = ((n - 1) / 2).min(TOP);
        let series = (1..=top)
            .map(|h| {
                let (mut a, mut b) = (0.0f64, 0.0f64);
                for (i, &x) in cycle.iter().enumerate() {
                    let ph = core::f64::consts::TAU * (h * i % n) as f64 / n as f64;
                    a += f64::from(x) * ph.cos();
                    b += f64::from(x) * ph.sin();
                }
                (2.0 * a / n as f64, 2.0 * b / n as f64)
            })
            .collect();
        Self::from_series(series)
    }

    /// Tables from `(cos, sin)` amplitudes of harmonics 1, 2, ….
    fn from_series(series: Vec<(f64, f64)>) -> Self {
        let fft = Fft::<f64>::new(SIZE);
        let mut tables = vec![0.0f32; LEVELS * STRIDE];
        let mut bins = vec![Complex::new(0.0, 0.0); SIZE / 2 + 1];
        let mut cycle = vec![0.0f64; SIZE];
        for (level, table) in tables.chunks_exact_mut(STRIDE).enumerate() {
            bins.fill(Complex::new(0.0, 0.0));
            for (h, &(a, b)) in series.iter().enumerate().take(TOP >> level) {
                // x = a·cos + b·sin  ⇔  X[h] = (a − i·b)/2 for an
                // inverse that returns n·x from n/2·(a − i·b).
                bins[h + 1] = Complex::new(0.5 * a, -0.5 * b);
            }
            fft.inverse_real(&mut bins, &mut cycle);
            table[0] = cycle[SIZE - 1] as f32;
            for (t, &c) in table[1..=SIZE].iter_mut().zip(&cycle) {
                *t = c as f32;
            }
            table[SIZE + 1] = cycle[0] as f32;
            table[SIZE + 2] = cycle[1] as f32;
        }
        Self {
            tables: tables.into(),
            hz: 0.0,
            sample_rate: 48_000.0,
            phase: 0.0,
            inc: 0.0,
            level: 0,
        }
    }

    #[inline]
    pub fn set_hz(&mut self, hz: f32) {
        self.hz = hz;
        self.inc = f64::from(hz) / f64::from(self.sample_rate);
        // The first level whose top harmonic is at or below Nyquist.
        let mut level = 0;
        while level + 1 < LEVELS && (TOP >> level) as f64 * self.inc > 0.5 {
            level += 1;
        }
        self.level = level;
    }

    /// The level the current pitch reads (0 holds the most harmonics).
    pub fn level(&self) -> usize {
        self.level
    }

    /// Phase in cycles, `0 ≤ t < 1`.
    pub fn phase(&self) -> f64 {
        self.phase
    }

    pub fn set_phase(&mut self, cycles: f64) {
        self.phase = cycles.rem_euclid(1.0);
    }
}

impl Stage for Wavetable {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.phase = 0.0;
    }
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        let x = self.phase * SIZE as f64;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let i = x as usize;
        let f = (x - i as f64) as f32;
        let t = &self.tables[self.level * STRIDE + i..self.level * STRIDE + i + 4];
        // Catmull-Rom through t[1]..t[2] (point i is t[1]).
        let (y0, y1, y2, y3) = (t[0], t[1], t[2], t[3]);
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * f + c2) * f + c1) * f + y1
    }
    /// A generator: it never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for Wavetable {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saw table's first level is the saw's Fourier series to 1024
    /// harmonics: within its Gibbs ripple of the naive saw away from the
    /// edge.
    #[test]
    fn a_waveform_table_is_its_fourier_series() {
        let w = Wavetable::new(Waveform::Saw);
        let t = &w.tables[1..=SIZE];
        for i in (SIZE / 8..SIZE * 7 / 8).step_by(97) {
            let naive = 2.0 * i as f32 / SIZE as f32 - 1.0;
            assert!(
                (t[i] - naive).abs() < 2e-3,
                "point {i}: {} vs {naive}",
                t[i]
            );
        }
        // A sine cycle of any length is one harmonic.
        let cycle: Vec<f32> = (0..100)
            .map(|i| (core::f32::consts::TAU * i as f32 / 100.0).sin())
            .collect();
        let s = Wavetable::from_cycle(&cycle);
        for i in (0..SIZE).step_by(61) {
            let want = (core::f64::consts::TAU * i as f64 / SIZE as f64).sin() as f32;
            assert!((s.tables[1 + i] - want).abs() < 1e-5);
        }
    }

    /// Every pitch reads a level whose top harmonic is below Nyquist.
    #[test]
    fn levels_stay_below_nyquist() {
        let mut w = Wavetable::new(Waveform::Square);
        w.prepare(48_000.0);
        for hz in [
            20.0f32, 23.0, 50.0, 440.0, 1_000.0, 5_000.0, 11_000.0, 23_000.0,
        ] {
            w.set_hz(hz);
            let top = (TOP >> w.level()) as f32 * hz;
            assert!(
                top <= 24_000.0,
                "{hz} Hz: level {} reaches {top} Hz",
                w.level()
            );
            if w.level() > 0 {
                // and the level above would not have been.
                assert!((TOP >> (w.level() - 1)) as f32 * hz > 24_000.0);
            }
        }
    }
}
