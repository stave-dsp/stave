//! First-order sections: the one-pole lowpass/highpass, its smoothing
//! coefficient, and the DC blocker.

use crate::kernel::denormal;
use crate::kernel::node::{Stage, Tail};

/// The per-sample step of a one-pole toward its target for a time
/// constant of `seconds` at `sample_rate`: `1 − e^(−1/(seconds·fs))`.
/// After one time constant a step input has covered `1 − 1/e` (63 %) of
/// the way. Zero or negative times follow instantly (coefficient 1).
///
/// The one formula every smoother, envelope and lag in the crate uses:
/// `y += tau_coeff(τ, fs) · (x − y)`.
#[inline]
pub fn tau_coeff(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        return 1.0;
    }
    debug_assert!(sample_rate > 0.0, "sample rate must be positive");
    1.0 - (-1.0 / (seconds * sample_rate)).exp()
}

/// Which side of the corner a [`OnePole`] passes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OnePoleMode {
    Lowpass,
    Highpass,
}

/// A 6 dB/octave lowpass or highpass with a matched pole: the lowpass is
/// `y += a·(x − y)`, `a = 1 − e^(−2π·hz/fs)` (−3 dB at the corner); the
/// highpass is the input minus that lowpass, so the two modes sum to the
/// input exactly (it reads −3.6 dB at the corner).
#[derive(Copy, Clone, Debug)]
pub struct OnePole {
    mode: OnePoleMode,
    hz: f32,
    sample_rate: f32,
    a: f32,
    y: f32,
}

impl OnePole {
    pub fn new(mode: OnePoleMode, hz: f32) -> Self {
        let mut f = Self {
            mode,
            hz,
            sample_rate: 48_000.0,
            a: 1.0,
            y: 0.0,
        };
        f.set_hz(hz);
        f
    }

    pub fn lowpass(hz: f32) -> Self {
        Self::new(OnePoleMode::Lowpass, hz)
    }

    pub fn highpass(hz: f32) -> Self {
        Self::new(OnePoleMode::Highpass, hz)
    }

    /// Move the corner (state kept). Held below 0.49·fs.
    pub fn set_hz(&mut self, hz: f32) {
        debug_assert!(hz >= 0.0, "corner {hz} Hz");
        self.hz = hz;
        let hz = hz.clamp(0.0, 0.49 * self.sample_rate);
        self.a = 1.0 - (-core::f32::consts::TAU * hz / self.sample_rate).exp();
    }

    pub const fn hz(&self) -> f32 {
        self.hz
    }
}

impl Stage for OnePole {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.y = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.y = denormal::flush(self.y + self.a * (x - self.y));
        match self.mode {
            OnePoleMode::Lowpass => self.y,
            OnePoleMode::Highpass => x - self.y,
        }
    }
    fn tail(&self) -> Tail {
        Tail::from_pole(f64::from(1.0 - self.a))
    }
}

/// Removes DC: `y = x − x₁ + R·y₁`, a zero at DC and a pole at
/// `R = e^(−2π·hz/fs)` just inside it. State and coefficient are `f64`:
/// a 5 Hz pole at 4× rates sits ~1.6e-4 from unity, where `f32`
/// quantizes the corner and the recursion leaves a low-frequency error
/// floor.
#[derive(Copy, Clone, Debug)]
pub struct DcBlocker {
    hz: f32,
    sample_rate: f32,
    r: f64,
    x1: f64,
    y1: f64,
}

impl Default for DcBlocker {
    /// A 5 Hz corner: below the audio band, fast enough to clear an
    /// operating-point shift in a fraction of a second.
    fn default() -> Self {
        Self::new(5.0)
    }
}

impl DcBlocker {
    pub fn new(hz: f32) -> Self {
        let mut d = Self {
            hz,
            sample_rate: 48_000.0,
            r: 0.0,
            x1: 0.0,
            y1: 0.0,
        };
        d.set_hz(hz);
        d
    }

    /// Move the corner (state kept).
    pub fn set_hz(&mut self, hz: f32) {
        self.hz = hz;
        self.r = (-2.0 * core::f64::consts::PI * f64::from(hz) / f64::from(self.sample_rate)).exp();
    }
}

impl Stage for DcBlocker {
    fn prepare(&mut self, sample_rate: f32) {
        debug_assert!(sample_rate > 0.0, "sample rate must be positive");
        self.sample_rate = sample_rate;
        self.set_hz(self.hz);
        self.reset();
    }
    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let out = f64::from(x) - self.x1 + self.r * self.y1;
        self.x1 = f64::from(x);
        self.y1 = denormal::flush64(out);
        out as f32
    }
    fn tail(&self) -> Tail {
        Tail::from_pole(self.r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tau_coeff_covers_63_percent_in_one_time_constant() {
        let fs = 48_000.0;
        let a = tau_coeff(0.010, fs);
        let mut y = 0.0f32;
        for _ in 0..480 {
            y += a * (1.0 - y);
        }
        assert!((y - (1.0 - (-1.0f32).exp())).abs() < 1e-3, "{y}");
        assert_eq!(tau_coeff(0.0, fs), 1.0);
    }

    #[test]
    fn one_pole_corner_is_minus_3_db_and_modes_sum_to_the_input() {
        let fs = 48_000.0f32;
        let hz = 1_000.0f32;
        let gain = |mode: OnePoleMode| {
            let mut f = OnePole::new(mode, hz);
            f.prepare(fs);
            let (mut acc_in, mut acc_out) = (0.0f64, 0.0f64);
            for i in 0..48_000 {
                let x = (core::f32::consts::TAU * hz * i as f32 / fs).sin();
                let y = f.tick(x);
                if i > 4_800 {
                    acc_in += f64::from(x * x);
                    acc_out += f64::from(y * y);
                }
            }
            10.0 * (acc_out / acc_in).log10()
        };
        // The matched-pole lowpass is −3.0 dB at the corner well below
        // Nyquist; its complement (input − lowpass) measures −3.6 dB.
        let (lp, hp) = (gain(OnePoleMode::Lowpass), gain(OnePoleMode::Highpass));
        assert!((lp + 3.0).abs() < 0.1, "lowpass {lp:.2} dB");
        assert!((hp + 3.6).abs() < 0.1, "highpass {hp:.2} dB");
        let (mut lp, mut hp) = (OnePole::lowpass(300.0), OnePole::highpass(300.0));
        lp.prepare(fs);
        hp.prepare(fs);
        for i in 0..1_000 {
            let x = ((i * 7919) % 101) as f32 / 50.0 - 1.0;
            assert!((lp.tick(x) + hp.tick(x) - x).abs() < 1e-6);
        }
    }

    #[test]
    fn dc_blocker_removes_dc_and_passes_audio() {
        let mut d = DcBlocker::default();
        d.prepare(48_000.0);
        let mut y = 0.0;
        for _ in 0..48_000 {
            y = d.tick(0.5);
        }
        assert!(y.abs() < 1e-6, "DC left: {y}");
        let Tail::Samples(n) = d.tail() else { panic!() };
        // 5 Hz pole: τ = 1/(2π·5) s → 13.8 τ ≈ 0.44 s.
        assert!((20_000..22_000).contains(&n), "{n}");
        d.reset();
        let mut peak = 0.0f32;
        for i in 0..4_800 {
            let x = (core::f32::consts::TAU * 1_000.0 * i as f32 / 48_000.0).sin();
            peak = peak.max(d.tick(x).abs());
        }
        assert!((peak - 1.0).abs() < 0.01, "{peak}");
    }

    #[test]
    fn one_pole_tail_matches_its_pole() {
        let mut f = OnePole::lowpass(100.0);
        f.prepare(48_000.0);
        let Tail::Samples(n) = f.tail() else { panic!() };
        let mut y = 1.0f32;
        f.reset();
        // Charge, then count samples until 120 dB down.
        for _ in 0..100_000 {
            f.tick(1.0);
        }
        let mut k = 0u32;
        while y > 1e-6 {
            y = f.tick(0.0);
            k += 1;
        }
        assert!(k.abs_diff(n) <= 2, "measured {k} vs reported {n}");
    }
}
