//! RearPath — the open back's second radiator. The cone's rear wave
//! is the front's INVERSE; it wraps around the baffle (a delay set
//! by the cab's scale), loses its top to edge diffraction (a gentle
//! lowpass), and meets the front wave at the mic. Level follows
//! geometry: the front arrives from `r`, the rear from `r + wrap`,
//! so g = r/(r + wrap) — a close mic barely hears the back door, a
//! pulled-back mic hears the comb (a law pins exactly this).
//!
//! Closed back = the path fades out (the air-spring side of "closed"
//! lives in the motor's k_box, not here). The fade is a 20 ms lag so
//! a back switch mid-note is a move, not a click.

use stave_dsp::FilterSpec;
use stave_dsp::{Biquad, DelayLine};

/// Wrap distance for the 60 L reference cab (front of baffle to the
/// open back and out to the axis), scaled by ∛(V/60L) with the box.
const WRAP_60L_M: f32 = 0.55;
/// Edge diffraction: the wrap keeps its lows, loses its top.
const DIFFRACTION_HZ: f32 = 1_500.0;

/// The display twin: the rear path's exact |H| in dB at `hz` —
/// |1 − g·H_lp(e^{jω})·e^{-jωτ}| from the same geometry as tick().
pub(crate) fn response_db(open: bool, vb_liters: f32, mic_r_m: f32, hz: f32, rate: f32) -> f32 {
    if !open {
        return 0.0;
    }
    let wrap = WRAP_60L_M * (vb_liters.max(1.0) / 60.0).cbrt();
    let tau = wrap / 343.0;
    let g = mic_r_m / (mic_r_m + wrap);
    let mut lp = Biquad::new();
    lp.set(
        FilterSpec::Lowpass {
            hz: DIFFRACTION_HZ,
            q: 0.6,
        },
        rate,
    );
    let (lr, li) = lp.response_at(hz, rate);
    let th = core::f32::consts::TAU * hz * tau;
    let (c, s) = (th.cos(), th.sin());
    // e^{-jθ}·(lr + j·li)
    let (rr, ri) = (lr * c + li * s, li * c - lr * s);
    let (re, im) = (1.0 - g * rr, -g * ri);
    10.0 * (re * re + im * im).max(1e-24).log10()
}

pub struct RearPath {
    delay: DelayLine,
    diffraction: Biquad,
    wrap_m: f32,
    delay_samples: f32,
    open: bool,
    /// Open/closed engagement, lagged (click-free back switch).
    mix: f32,
    mix_k: f32,
}

impl RearPath {
    pub fn new() -> Self {
        Self {
            delay: DelayLine::new(),
            diffraction: Biquad::new(),
            wrap_m: WRAP_60L_M,
            delay_samples: 0.0,
            open: false,
            mix: 0.0,
            mix_k: 0.0,
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        // Longest wrap (200 L) at this rate, with headroom.
        self.delay.prepare((rate * 0.005) as usize + 8);
        self.delay.reset();
        self.diffraction.reset();
        self.mix_k = 1.0 - (-1.0 / (0.020 * rate)).exp();
        self.mix = 0.0;
        self.rate_apply(rate);
    }

    /// Re-voice at block rate; path state is kept.
    pub fn set(&mut self, open: bool, vb_liters: f32, rate: f32) {
        self.open = open;
        self.wrap_m = WRAP_60L_M * (vb_liters.max(1.0) / 60.0).cbrt();
        self.rate_apply(rate);
    }

    /// Jump the open/closed engagement to its target — the FIRST
    /// bind after activate only (no history to click against; the
    /// 20 ms glide is for mid-play switches). Without this, a fresh
    /// open-back cab faded its rear path in from silence — the
    /// picture-truth law caught the render missing its comb.
    pub fn snap(&mut self) {
        self.mix = if self.open { 1.0 } else { 0.0 };
    }

    fn rate_apply(&mut self, rate: f32) {
        self.delay_samples = self.wrap_m / 343.0 * rate;
        self.diffraction.set(
            FilterSpec::Lowpass {
                hz: DIFFRACTION_HZ,
                q: 0.6,
            },
            rate,
        );
    }

    /// `mic_r` in meters — the per-sample geometry read (rides the
    /// declared mic-distance ramp).
    #[inline]
    pub fn tick(&mut self, x: f32, mic_r: f32) -> f32 {
        self.delay.write(x);
        let rear = self
            .diffraction
            .process(self.delay.read(self.delay_samples));
        self.delay.advance();

        let target = if self.open { 1.0 } else { 0.0 };
        self.mix += self.mix_k * (target - self.mix);
        let g = mic_r / (mic_r + self.wrap_m) * self.mix;
        x - g * rear
    }
}
