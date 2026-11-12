//! `AnalogPath` — a feedback path that leaves the DSP and comes back:
//! the pedal's two regeneration loops run through analog circuitry,
//! so each lap is coupled (a DC-blocking high-pass), band-limited (a
//! one-pole low-pass), and soft-clipped. The clipper is what lets the
//! knobs reach unity without running away — a maxed loop sustains
//! and saturates instead of exploding.

pub(super) struct AnalogPath {
    hp: [f32; 2],
    hp_x: [f32; 2],
    lp: [f32; 2],
    hp_a: f32,
    lp_a: f32,
    drive: f32,
}

impl AnalogPath {
    pub(super) fn new() -> Self {
        Self { hp: [0.0; 2], hp_x: [0.0; 2], lp: [0.0; 2], hp_a: 0.0, lp_a: 1.0, drive: 1.0 }
    }

    /// Corner frequencies in Hz at the core tick rate `fs`; `drive`
    /// scales into the clipper (and back out — unity in the small
    /// signal region, harmonics only when pushed).
    pub(super) fn voice(&mut self, fs: f32, hp_hz: f32, lp_hz: f32, drive: f32) {
        self.hp_a = (-2.0 * core::f32::consts::PI * hp_hz / fs).exp();
        self.lp_a = 1.0 - (-2.0 * core::f32::consts::PI * lp_hz / fs).exp();
        self.drive = drive;
    }

    pub(super) fn reset(&mut self) {
        self.hp = [0.0; 2];
        self.hp_x = [0.0; 2];
        self.lp = [0.0; 2];
    }

    #[inline]
    pub(super) fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let mut y = [0.0; 2];
        for c in 0..2 {
            let h = self.hp_a * (self.hp[c] + x[c] - self.hp_x[c]);
            self.hp_x[c] = x[c];
            self.hp[c] = flush(h);
            self.lp[c] = flush(self.lp[c] + self.lp_a * (h - self.lp[c]));
            y[c] = soft(self.lp[c] * self.drive) / self.drive;
        }
        y
    }
}

/// Rational tanh — exact slope 1 at zero, ±1 asymptote, no libm.
#[inline]
pub(super) fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[inline]
pub(super) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}
