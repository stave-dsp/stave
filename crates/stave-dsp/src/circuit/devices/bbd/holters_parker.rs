//! Holters & Parker's combined model of a BBD and its filters.

use super::filter::{C64, MAX_POLES};
use super::{Chip, Filter};
use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};
use crate::primitives::random::Xorshift32;
use crate::units::{Hz, Seconds};

/// One pole of a filter as the runtime holds it: one of each conjugate
/// pair (weight 2: the pair's sum is twice the real part), real poles
/// once (weight 1).
#[derive(Copy, Clone, Debug, Default)]
struct Pole {
    /// `p·Ts`: `p̄^d = e^(p·Ts·d)`.
    p_ts: C64,
    /// `p̄ = e^(p·Ts)`.
    p_bar: C64,
    /// The input filter's `Ts·r·weight`, or the output filter's `r/p·weight`.
    gain: C64,
}

/// The poles of `filter` at the sampling interval `ts`, conjugate pairs
/// folded; `output` selects the output filter's step gain `r/p`.
fn fold(filter: &Filter, ts: f64, output: bool) -> ([Pole; MAX_POLES], usize, f64) {
    let mut poles = [Pole::default(); MAX_POLES];
    let (mut n, mut h0) = (0, 0.0);
    for m in 0..filter.len {
        let (p, r) = (filter.poles[m], filter.residues[m]);
        let r_over_p = r.div(p);
        h0 -= r_over_p.re;
        if p.im < 0.0 {
            continue; // its conjugate carries it
        }
        let weight = if p.im > 0.0 { 2.0 } else { 1.0 };
        let gain = if output { r_over_p } else { r.scale(ts) };
        poles[n] = Pole {
            p_ts: p.scale(ts),
            p_bar: p.scale(ts).exp(),
            gain: gain.scale(weight),
        };
        n += 1;
    }
    (poles, n, h0)
}

/// A bucket-brigade chip with its input and output filters, sampled at
/// the chip's clock and resampled to the host rate through those filters.
///
/// # Models
/// A BBD ([`Chip`]: MN3007, MN3008, MN3205) as Holters & Parker, "A
/// Combined Model for a Bucket Brigade Device and its Input and Output
/// Filters", DAFx-18: the input filter's output sampled on every other
/// clock edge, `stages / 2` buckets, and the held output (a staircase
/// changing on the other edges) through the output filter — so the delay
/// is `stages / (2·clock)`, the hold is the `sinc(f / clock)` droop, a
/// clock change glides the pitch as the hardware does, and whatever the
/// filters pass beyond half the clock aliases (input) and images
/// (output) as on the chip. Beyond the paper: the clock's feedthrough (a
/// square wave at the clock on the output pin) and the chip's noise, both
/// shaped by the output filter like the signal.
///
/// # Derivation
/// The filters as partial fractions `Σ rₘ/(s − pₘ)` ([`Filter`], from
/// their circuits or published coefficients), discretised by the
/// modified impulse invariance of the paper: each pole a one-pole
/// recursion at the host rate; the input read at a clock edge at
/// fraction `d` of a host interval as `Σ Ts·rₘ·p̄ₘᵈ·xₘ` (eq. 10); each
/// output step `Δ` added to the output states as `(rₘ/pₘ)·p̄ₘ^(1−d)·Δ`
/// (eq. 23), with the held value through `H₀ = −Σ rₘ/pₘ` (eq. 25).
/// Conjugate pairs run as one complex recursion and twice its real part.
/// Feedthrough enters as a step of the clock's polarity on every edge,
/// noise as a white sample of the chip's quoted S/N on each output edge.
///
/// # Verified
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::delay_follows_the_clock`
/// (a 100 Hz tone's phase less the filters' is `stages/(2·clock)` within
/// 0.01 % at 10–100 kHz; measured to 1e-6),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::matches_the_papers_sine_through_the_juno_60_filters`
/// (the paper's Fig. 7 setup — 256 stages at 50 kHz, fs 44.1 kHz, 1 kHz
/// through the Juno-60 filters — within 1e-3 of eqs. 36–38; measured
/// 6.5e-4, the aliasing the paper also shows),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::response_is_the_filters_and_the_hold`
/// (a tone's gain is `|H_in|·|H_out|·sinc(f/clock)`, the paper's eq. 37,
/// within 0.01 dB from 100 Hz to 8 kHz at 48 and 96 kHz; measured
/// 0.0012 dB),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::aliases_and_images_follow_the_clock`
/// (a tone above half the clock folds to `clock − f`, and one below
/// images there, at the level the filters and the hold predict, within
/// 0.2 dB: −69.3 and −73.7 dB at a 40 kHz clock),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::noise_floor_is_the_datasheet_s_n`
/// (idle noise within 0.2 dB of the quoted S/N held a clock period and
/// shaped by the output filter: 89 µV, 84.5 dB below 1.5 V for the
/// MN3007 at 40 kHz),
/// `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::feedthrough_is_the_clock_through_the_output_filter`
/// (the clock's square wave through the output filter, within 0.1 dB)
/// and `crates/stave-dsp/src/circuit/devices/bbd/tests.rs::a_clock_step_glides`
/// (the paper's Fig. 8: a clock step bends the pitch, no jump);
/// the filter circuits against ngspice in
/// `crates/stave-test/tests/bbd_matches_ngspice.rs`.
///
/// # Cost
/// `"HoltersParker (MN3007, Juno-60 filters, 40 kHz clock)"` and
/// `"HoltersParker (MN3007, Juno-60 filters, 100 kHz clock)"` on the
/// datasheet: one complex exponential per folded pole per clock edge, so
/// it rises with the clock.
///
/// # Aliasing
/// The chip's own, by design: what the input filter passes above half
/// the clock folds, and the output staircase images at `clock ± f`. The
/// host-rate sampling of the output filter is exact for the staircase,
/// so images above the host's Nyquist fold as the output filter lets
/// them; run at 2× when a low clock leaves images there. That fold is
/// the simulation's, not the chip's: Gabrielli, D'Angelo & Squartini
/// (DAFx-25) cancel it with BLEP corrections on the staircase's steps,
/// not applied here.
///
/// # Valid ranges
/// Clock within the chip's datasheet range (clamped); host rates where
/// the filters' poles sit well below Nyquist (44.1 kHz up); input in volts
/// (the chip is linear here: no clipping, insertion gain 1).
#[derive(Clone, Debug)]
pub struct HoltersParker {
    chip: Chip,
    input: Filter,
    output: Filter,
    sample_rate: f64,
    in_poles: [Pole; MAX_POLES],
    in_len: usize,
    out_poles: [Pole; MAX_POLES],
    out_len: usize,
    h0: f64,
    x_in: [C64; MAX_POLES],
    x_out: [C64; MAX_POLES],
    buckets: Vec<f32>,
    write: usize,
    clock: Hz,
    /// Host samples between two clock edges, `fs / (2·clock)`.
    edge_samples: f64,
    /// The next edge's time after the last host sample, in samples.
    next_edge: f64,
    /// Whether the next edge samples the input (even) or moves the output.
    next_is_input: bool,
    /// The signal on the output pin (the bucket being read, plus noise).
    held: f64,
    /// The clock's polarity on the pin.
    phase: f64,
    /// The pin's last value: `held + feedthrough·phase`.
    pin: f64,
    feedthrough: f64,
    noise: f64,
    rng: Xorshift32,
}

impl HoltersParker {
    /// `chip` between the `input` and `output` filters, clocked at the
    /// middle of its range (geometrically). Buckets are allocated in
    /// `prepare`.
    pub fn new(chip: Chip, input: Filter, output: Filter) -> Self {
        let clock = Hz::new((chip.clock_min.get() * chip.clock_max.get()).sqrt());
        let mut s = Self {
            chip,
            input,
            output,
            sample_rate: 48_000.0,
            in_poles: [Pole::default(); MAX_POLES],
            in_len: 0,
            out_poles: [Pole::default(); MAX_POLES],
            out_len: 0,
            h0: 0.0,
            x_in: [C64::default(); MAX_POLES],
            x_out: [C64::default(); MAX_POLES],
            buckets: Vec::new(),
            write: 0,
            clock,
            edge_samples: 1.0,
            next_edge: 0.5,
            next_is_input: true,
            held: 0.0,
            phase: 1.0,
            pin: 0.0,
            feedthrough: chip.feedthrough.get(),
            noise: chip.noise_volts(),
            rng: Xorshift32::new(0x5EED_BBD0),
        };
        s.update();
        s
    }

    /// The chip, for inspection.
    pub fn chip(&self) -> &Chip {
        &self.chip
    }

    /// The input filter.
    pub fn input_filter(&self) -> &Filter {
        &self.input
    }

    /// The output filter.
    pub fn output_filter(&self) -> &Filter {
        &self.output
    }

    /// The BBD clock, clamped to the chip's range. Takes effect from the
    /// next clock edge, so a modulated clock may be set every sample.
    pub fn set_clock(&mut self, clock: Hz) {
        let (lo, hi) = (self.chip.clock_min.get(), self.chip.clock_max.get());
        let c = clock.get();
        self.clock = Hz::new(if c >= lo { c.min(hi) } else { lo });
        self.edge_samples = self.sample_rate / (2.0 * f64::from(self.clock.get()));
    }

    /// The clock for a delay of `delay` (`stages / (2·delay)`), clamped
    /// to the chip's range.
    pub fn set_delay(&mut self, delay: Seconds) {
        self.set_clock(self.chip.clock_for(delay.get().max(1e-6)));
    }

    /// The clock in use.
    pub fn clock(&self) -> Hz {
        self.clock
    }

    /// The delay at the current clock, `stages / (2·clock)`, in seconds
    /// (the filters add their group delay on top).
    pub fn delay(&self) -> Seconds {
        Seconds::new(self.chip.delay_s(self.clock))
    }

    /// The clock's residue on the output pin, in volts (peak of the
    /// square wave); `0` turns it off.
    pub fn set_feedthrough(&mut self, volts: f64) {
        self.feedthrough = volts.max(0.0);
    }

    /// The chip's noise at the output pin, RMS volts; `0` turns it off.
    pub fn set_noise(&mut self, volts: f64) {
        self.noise = volts.max(0.0);
    }

    fn update(&mut self) {
        let ts = 1.0 / self.sample_rate;
        let (p, n, _) = fold(&self.input, ts, false);
        self.in_poles = p;
        self.in_len = n;
        let (p, n, h0) = fold(&self.output, ts, true);
        self.out_poles = p;
        self.out_len = n;
        self.h0 = h0;
        self.set_clock(self.clock);
    }

    /// Run the clock edges that fall in `(k−1, k]` (in host samples, `k`
    /// the sample about to be produced).
    #[inline]
    fn edges(&mut self) {
        loop {
            let d = self.next_edge;
            // An input edge exactly on the sample belongs to the next
            // interval (it must see that sample); an output edge to this.
            if d > 1.0 || (self.next_is_input && d >= 1.0) {
                break;
            }
            if self.next_is_input {
                let mut u = 0.0;
                for (pole, x) in self.in_poles[..self.in_len].iter().zip(&self.x_in) {
                    u += pole.gain.mul(pole.p_ts.scale(d).exp()).mul(*x).re;
                }
                if let Some(slot) = self.buckets.get_mut(self.write) {
                    *slot = u as f32;
                }
                self.write += 1;
                if self.write >= self.buckets.len() {
                    self.write = 0;
                }
            } else {
                // The oldest bucket reaches the output.
                let out = self.buckets.get(self.write).copied().unwrap_or(0.0);
                let noise = if self.noise > 0.0 {
                    self.noise * 3f64.sqrt() * f64::from(self.rng.next_bipolar())
                } else {
                    0.0
                };
                self.held = f64::from(out) + noise;
            }
            self.phase = -self.phase;
            let pin = self.held + self.feedthrough * self.phase;
            let step = pin - self.pin;
            self.pin = pin;
            if step != 0.0 {
                for (pole, x) in self.out_poles[..self.out_len].iter().zip(self.x_out.iter_mut()) {
                    *x = x.add(pole.gain.mul(pole.p_ts.scale(1.0 - d).exp()).scale(step));
                }
            }
            self.next_edge += self.edge_samples;
            self.next_is_input = !self.next_is_input;
        }
        self.next_edge -= 1.0;
    }

    /// One sample in `f64` (volts in, volts out).
    #[inline]
    pub fn tick_f64(&mut self, x: f64) -> f64 {
        self.edges();
        for (pole, s) in self.in_poles[..self.in_len].iter().zip(self.x_in.iter_mut()) {
            let v = pole.p_bar.mul(*s);
            *s = C64::new(denormal::flush64(v.re + x), denormal::flush64(v.im));
        }
        let mut y = self.h0 * self.pin;
        for (pole, s) in self.out_poles[..self.out_len].iter().zip(self.x_out.iter_mut()) {
            y += s.re;
            let v = pole.p_bar.mul(*s);
            *s = C64::new(denormal::flush64(v.re), denormal::flush64(v.im));
        }
        y
    }
}

impl Stage for HoltersParker {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = f64::from(sample_rate.max(1.0));
        self.buckets = vec![0.0; (self.chip.stages / 2).max(1) as usize];
        self.update();
        self.reset();
    }

    fn reset(&mut self) {
        self.buckets.iter_mut().for_each(|b| *b = 0.0);
        self.write = 0;
        self.x_in = [C64::default(); MAX_POLES];
        self.x_out = [C64::default(); MAX_POLES];
        self.next_edge = 0.5 * self.edge_samples;
        self.next_is_input = true;
        self.held = 0.0;
        self.phase = 1.0;
        self.pin = self.feedthrough;
        self.rng = Xorshift32::new(0x5EED_BBD0);
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.tick_f64(f64::from(x)) as f32
    }

    fn tail(&self) -> Tail {
        // The longest delay (the slowest clock) and both filters' decay.
        let longest = self.chip.delay_s(self.chip.clock_min) * self.sample_rate;
        let decay = |f: &Filter| Tail::from_time_constant(1.0 / f.slowest_decay(), self.sample_rate as f32);
        decay(&self.input).then(decay(&self.output)).plus(longest.ceil() as u32)
    }
}
