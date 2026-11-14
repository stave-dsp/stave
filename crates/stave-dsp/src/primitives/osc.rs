//! `Osc` — the audio-rate oscillator (`Lfo` is the control-rate one)
//! — [`Wavetable`], the mip-mapped table oscillator, and [`Waveform`],
//! the shape vocabulary they share. A phase accumulator with f64 phase
//! (pitch is exactness, and f32 increments drift audibly over a held
//! note), producing:
//! * `Sine` — pure, THD at the float floor;
//! * `Saw` / `Square` — band-limited steps at each discontinuity;
//! * `Triangle` — band-limited ramps (BLAMP) at each corner.
//!
//! Two band-limiting kinds, chosen at construction:
//! [`Osc::new`] corrects each edge over the two samples around it with
//! the polynomial residuals (polyBLEP and polyBLAMP), and
//! [`Osc::min_blep`] adds a tabulated minimum-phase residual over the
//! samples after it (minBLEP and minBLAMP), much cleaner near Nyquist
//! for a few samples of delay. Either oscillator hard-syncs to a master
//! frequency ([`Osc::set_sync`]): each reset is one more edge, a jump
//! and a slope change, corrected the same way. The residual kernels live
//! in `osc/blep.rs`.

use crate::kernel::node::{Generator, Stage, Tail};
use crate::units::Hz;

pub(crate) mod blep;
mod wavetable;

pub use wavetable::Wavetable;

use blep::MinBlep;

/// The periodic waveforms every oscillator and LFO draws from.
///
/// Phase is in **cycles**, `0 ≤ t < 1`, everywhere in stave-dsp. At
/// `t = 0`: the sine is 0 and rising, the triangle is at −1 and rising
/// to +1 at `t = ½`, the saw ramps −1 → +1 over the cycle, and the
/// square is +1 for the first half (in phase with the sine).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Waveform {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Waveform {
    /// The naive (not band-limited) waveform at phase `t` cycles —
    /// exact for control-rate use; [`Osc`] band-limits the edges.
    #[inline]
    pub fn eval(self, t: f32) -> f32 {
        let t = t.rem_euclid(1.0);
        match self {
            Waveform::Sine => (core::f32::consts::TAU * t).sin(),
            Waveform::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Waveform::Saw => 2.0 * t - 1.0,
            Waveform::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    /// The naive value at `t` (in `[0, 1)`), in f64.
    #[inline]
    fn value(self, t: f64) -> f64 {
        match self {
            Waveform::Sine => (core::f64::consts::TAU * t).sin(),
            Waveform::Triangle => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Waveform::Saw => 2.0 * t - 1.0,
            Waveform::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    /// The naive slope at `t`, per sample, at `inc` cycles per sample.
    #[inline]
    fn slope(self, t: f64, inc: f64) -> f64 {
        match self {
            Waveform::Sine => core::f64::consts::TAU * inc * (core::f64::consts::TAU * t).cos(),
            Waveform::Triangle => {
                if t < 0.5 {
                    4.0 * inc
                } else {
                    -4.0 * inc
                }
            }
            Waveform::Saw => 2.0 * inc,
            Waveform::Square => 0.0,
        }
    }

    /// The naive edges within a cycle: `(phase, jump, slope change)`,
    /// ascending; the last is the wrap at 1 (an edge of zero size for a
    /// sine).
    #[inline]
    fn edges(self, inc: f64) -> ([(f64, f64, f64); 2], usize) {
        match self {
            Waveform::Sine => ([(1.0, 0.0, 0.0), (0.0, 0.0, 0.0)], 1),
            Waveform::Saw => ([(1.0, -2.0, 0.0), (0.0, 0.0, 0.0)], 1),
            Waveform::Square => ([(0.5, -2.0, 0.0), (1.0, 2.0, 0.0)], 2),
            Waveform::Triangle => ([(0.5, 0.0, -8.0 * inc), (1.0, 0.0, 8.0 * inc)], 2),
        }
    }
}

/// How an [`Osc`] band-limits its edges.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Edges {
    /// Two-sample polynomial residuals, centred on the edge.
    Poly,
    /// Tabulated minimum-phase residuals, after the edge.
    MinBlep,
}

/// One edge inside a sample interval: `(fraction, jump, slope change)`.
type Edge = (f64, f64, f64);

/// The most edges one interval can hold below Nyquist: a natural edge,
/// a sync reset, and a natural edge after it.
const MAX_EDGES: usize = 4;

/// The minBLEP correction ring, in samples (a power of two above the
/// residual's length; checked by `the_minblep_ring_holds_the_residual`).
const RING: usize = 128;

/// The naive history the minBLEP output is delayed through.
const HISTORY: usize = 8;

/// The audio-rate oscillator: a generator [`Stage`] (its input is
/// ignored). Phase is `f64` cycles.
///
/// # Models
///
/// The four [`Waveform`]s from a phase accumulator, sine exact; saw and
/// square band-limited at every jump, the triangle at every corner, and
/// a hard-synced oscillator at every reset. [`Osc::new`] uses the
/// two-sample polynomial residuals, [`Osc::min_blep`] the tabulated
/// minimum-phase ones.
///
/// # Derivation
///
/// The polynomial band-limited step of Välimäki and Huovilainen (2007)
/// and its integral, the band-limited ramp (Esqueda, Välimäki and
/// Bilbao, DAFx 2016); the minimum-phase band-limited step of Brandt
/// ("Hard sync without aliasing", ICMC 2001) from a 100 dB Kaiser
/// windowed sinc with 32 zero crossings, made minimum phase by the real
/// cepstrum, and its integral. A hard-sync reset is an edge at the
/// master's wrap: a jump to the start of the cycle and a change of
/// slope, each corrected by its residual.
///
/// # Verified
///
/// `crates/stave-dsp/tests/aliasing_budgets.rs::oscillators_hold_their_alias_budgets`
/// (each kind's worst inharmonic energy over a stepped tone, 110 Hz to
/// 5 kHz at 44.1 and 48 kHz, held to a budget recorded beside its
/// measurement), `crates/stave-dsp/src/primitives/osc/tests.rs::polyblep_buys_at_least_20_db_on_the_worst_alias`,
/// `crates/stave-dsp/src/primitives/osc/tests.rs::blamp_buys_at_least_20_db_on_the_triangle`,
/// `crates/stave-dsp/src/primitives/osc/tests.rs::pitch_is_exact` and
/// `crates/stave-dsp/src/primitives/osc/tests.rs::sine_harmonics_are_at_the_floor`.
///
/// # Cost
///
/// Datasheet `"Osc (sine)"`, `"Osc (saw, polyBLEP)"`,
/// `"Osc (triangle, polyBLAMP)"`, `"Osc (saw, minBLEP)"` and
/// `"Osc (saw, hard sync)"`.
///
/// # Aliasing
///
/// Band-limited edges: polyBLEP leaves residual aliasing near Nyquist
/// (first-order rejection grows as an alias departs it); minBLEP rejects
/// it until its prototype's transition, 0.46 of the rate. The budgets
/// are in Verified. [`Wavetable`] is alias-free to its interpolation.
///
/// # Valid ranges
///
/// `0 ≤ hz < sample_rate / 2`, and the same for a sync master. The
/// residuals assume edges at least a sample apart; above a quarter of
/// the rate they overlap and add, still finite. The minBLEP output lags
/// the phase by a few samples (the minimum-phase step's delay).
#[derive(Copy, Clone, Debug)]
pub struct Osc {
    waveform: Waveform,
    edges: Edges,
    hz: f32,
    sample_rate: f32,
    phase: f64,
    inc: f64,
    /// The sync master, when set: its frequency, increment and phase.
    master: Option<f32>,
    master_inc: f64,
    master_phase: f64,
    /// polyBLEP with edges found by the accumulator: the next sample's
    /// share of this interval's edges.
    pending: f64,
    /// minBLEP: corrections waiting for their samples, and the naive
    /// waveform's recent past (the output is delayed to match the
    /// residuals').
    ring: [f32; RING],
    at: usize,
    history: [f32; HISTORY],
}

#[inline]
fn polyblep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

impl Osc {
    /// polyBLEP and polyBLAMP: two samples per edge, no delay.
    pub fn new(waveform: Waveform) -> Self {
        Self::with_edges(waveform, Edges::Poly)
    }

    /// minBLEP and minBLAMP: a tabulated minimum-phase residual after
    /// each edge, cleaner near Nyquist; the output lags the phase by the
    /// residual's delay (3 samples). The tables are compile-time
    /// constants: nothing is built or allocated.
    pub fn min_blep(waveform: Waveform) -> Self {
        Self::with_edges(waveform, Edges::MinBlep)
    }

    fn with_edges(waveform: Waveform, edges: Edges) -> Self {
        Self {
            waveform,
            edges,
            hz: 0.0,
            sample_rate: 48_000.0,
            phase: 0.0,
            inc: 0.0,
            master: None,
            master_inc: 0.0,
            master_phase: 0.0,
            pending: 0.0,
            ring: [0.0; RING],
            at: 0,
            history: [0.0; HISTORY],
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    #[inline]
    pub fn set_frequency(&mut self, frequency: Hz) {
        let hz = frequency.get();
        self.hz = hz;
        self.inc = f64::from(hz) / f64::from(self.sample_rate);
    }

    /// Hard sync to a master oscillator at `master` (`None`: free
    /// running): each time the master's phase wraps, this oscillator's
    /// restarts at 0, mid-sample, with the jump and the slope change
    /// band-limited.
    pub fn set_sync(&mut self, master: Option<Hz>) {
        self.master = master.map(Hz::get);
        self.master_inc = self
            .master
            .map_or(0.0, |m| f64::from(m) / f64::from(self.sample_rate));
    }

    /// Phase in cycles, `0 ≤ t < 1`.
    pub fn phase(&self) -> f64 {
        self.phase
    }

    pub fn set_phase(&mut self, cycles: f64) {
        self.phase = cycles.rem_euclid(1.0);
    }

    /// The polyBLEP oscillator without sync: edges found from the phase
    /// alone (the saw and square as they always were; the triangle's
    /// corners get polyBLAMP).
    #[inline]
    fn tick_free(&mut self) -> f64 {
        let t = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let dt = self.inc;
        match self.waveform {
            Waveform::Sine => (core::f64::consts::TAU * t).sin(),
            Waveform::Triangle => {
                let naive = if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                };
                // Corners at 0 (slope +8·dt) and ½ (−8·dt), at τ samples.
                let corner = |c: f64| {
                    let d = t - c;
                    let d = d - d.round();
                    blep::poly_ramp(d / dt)
                };
                if dt > 0.0 {
                    naive + 8.0 * dt * (corner(0.0) - corner(0.5))
                } else {
                    naive
                }
            }
            Waveform::Saw => (2.0 * t - 1.0) - polyblep(t, dt),
            Waveform::Square => {
                let raw = if t < 0.5 { 1.0 } else { -1.0 };
                raw + polyblep(t, dt) - polyblep((t + 0.5).rem_euclid(1.0), dt)
            }
        }
    }

    /// Advance one sample, collecting every edge in the interval: the
    /// waveform's own and a sync reset, with each one's fraction.
    fn advance(&mut self) -> ([Edge; MAX_EDGES], usize) {
        let inc = self.inc;
        let w = self.waveform;
        let mut out = [(0.0, 0.0, 0.0); MAX_EDGES];
        let mut n = 0;
        let reset = if self.master.is_some() {
            let m = self.master_phase + self.master_inc;
            if m >= 1.0 && self.master_inc > 0.0 {
                let f = (1.0 - self.master_phase) / self.master_inc;
                self.master_phase = (m - 1.0).rem_euclid(1.0);
                Some(f.clamp(0.0, 1.0))
            } else {
                self.master_phase = m;
                None
            }
        } else {
            None
        };
        let (natural, count) = w.edges(inc);
        let mut t = self.phase;
        let mut done = 0.0;
        // The waveform's edges up to `limit`, from phase `t` at `done`.
        let run = |t: &mut f64,
                       done: &mut f64,
                       limit: f64,
                       out: &mut [Edge; MAX_EDGES],
                       n: &mut usize| {
            for _ in 0..MAX_EDGES {
                let Some(&(e, dv, ds)) = natural[..count].iter().find(|&&(e, _, _)| e > *t) else {
                    break;
                };
                let f = *done + (e - *t) / inc;
                if f.is_nan() || f > limit {
                    break;
                }
                if (dv != 0.0 || ds != 0.0) && *n < MAX_EDGES {
                    out[*n] = (f, dv, ds);
                    *n += 1;
                }
                *t = if e >= 1.0 { 0.0 } else { e };
                *done = f;
            }
        };
        run(&mut t, &mut done, reset.unwrap_or(1.0), &mut out, &mut n);
        if let Some(f) = reset {
            let before = (t + (f - done) * inc).min(1.0);
            if n < MAX_EDGES {
                out[n] = (
                    f,
                    w.value(0.0) - w.value(before),
                    w.slope(0.0, inc) - w.slope(before, inc),
                );
                n += 1;
            }
            t = 0.0;
            done = f;
            run(&mut t, &mut done, 1.0, &mut out, &mut n);
        }
        t += (1.0 - done) * inc;
        self.phase = if t >= 1.0 { t - 1.0 } else { t };
        (out, n)
    }

    /// The polyBLEP oscillator with edges from [`advance`](Self::advance):
    /// each edge corrects this sample and the next.
    fn tick_poly(&mut self) -> f64 {
        let mut y = self.waveform.value(self.phase) + self.pending;
        let (edges, n) = self.advance();
        let mut next = 0.0;
        for &(f, dv, ds) in &edges[..n] {
            let (before, after) = blep::poly(f, dv, ds);
            y += before;
            next += after;
        }
        self.pending = next;
        y
    }

    /// The minBLEP oscillator: the naive waveform delayed by the
    /// residual's delay, plus the corrections due now.
    fn tick_min(&mut self) -> f64 {
        let table = MinBlep::get();
        let d = table.delay;
        self.history.copy_within(0..HISTORY - 1, 1);
        self.history[0] = self.waveform.value(self.phase) as f32;
        let mask = RING - 1;
        let y = f64::from(self.history[d]) + f64::from(self.ring[self.at]);
        self.ring[self.at] = 0.0;
        let (edges, n) = self.advance();
        for &(f, dv, ds) in &edges[..n] {
            for k in 1..=table.len {
                let (step, ramp) = table.residual(k as f64 - f);
                let slot = &mut self.ring[(self.at + k) & mask];
                *slot += (dv * step + ds * ramp) as f32;
            }
        }
        self.at = (self.at + 1) & mask;
        y
    }
}

impl Stage for Osc {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_frequency(Hz::new(self.hz));
        self.set_sync(self.master.map(Hz::new));
        self.reset();
    }
    fn reset(&mut self) {
        self.phase = 0.0;
        self.master_phase = 0.0;
        self.pending = 0.0;
        self.ring = [0.0; RING];
        self.at = 0;
        self.history = [0.0; HISTORY];
    }
    #[inline]
    fn tick(&mut self, _: f32) -> f32 {
        let y = match (self.edges, self.master) {
            (Edges::Poly, None) => self.tick_free(),
            (Edges::Poly, Some(_)) => self.tick_poly(),
            (Edges::MinBlep, _) => self.tick_min(),
        };
        y as f32
    }
    /// A generator: it never stops.
    fn tail(&self) -> Tail {
        Tail::Infinite
    }
}

impl Generator for Osc {}

#[cfg(test)]
mod tests;

/// The minBLEP table design, for `tests/minblep_tables.rs`, which writes
/// the committed constants. Not API.
#[doc(hidden)]
pub fn __minblep_design() -> (Vec<f64>, Vec<f64>, usize, usize) {
    blep::MinBlep::design()
}
