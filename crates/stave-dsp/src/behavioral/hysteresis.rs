//! Hysteresis — path-dependent magnetization (tape/transformer).
use crate::kernel::node::Stage;

/// The Jiles-Atherton split from the Memory model, as a Stage: a
/// REVERSIBLE part follows the anhysteretic curve instantly; the
/// IRREVERSIBLE part (remanence) approaches it at a rate proportional to
/// signal MOTION, opening a frequency-independent loop. AC-coupled so
/// remanence doesn't linger into silence.
pub struct Hysteresis {
    drive: f32,
    /// 0 = none (pure anhysteretic), 1 = wide loop.
    depth: f32,
    reversible: f32,
    hyst_rate: f32,
    mag: f32,
    last_x: f32,
    // f64 LF one-pole: at 4× rates the 5 Hz pole sits ~1.6e-4
    // from unity, where f32 quantizes the corner and the recursion
    // leaves an LF error floor (measured in coupler_conformance.rs).
    dc_r: f64,
    dc_x1: f64,
    dc_y1: f64,
}

impl Hysteresis {
    pub fn new(drive: f32, depth: f32) -> Self {
        debug_assert!(drive > 0.0, "drive must be positive");
        let mut h = Self {
            drive,
            depth: 0.0,
            reversible: 0.4,
            hyst_rate: 30.0,
            mag: 0.0,
            last_x: 0.0,
            dc_r: 0.9995,
            dc_x1: 0.0,
            dc_y1: 0.0,
        };
        h.set_depth(depth);
        h
    }
    pub fn set_depth(&mut self, depth: f32) {
        debug_assert!((0.0..=1.0).contains(&depth), "depth {depth}");
        self.depth = depth;
        // Deeper ⇒ slower path-approach ⇒ wider loop (as in Memory::Tape).
        self.hyst_rate = 30.0 * (1.0 - 0.9 * self.depth).powi(2) + 1.5;
    }
    pub fn set_drive(&mut self, drive: f32) {
        debug_assert!(drive > 0.0, "drive must be positive");
        self.drive = drive;
    }
}

impl Stage for Hysteresis {
    fn prepare(&mut self, rate: f32) {
        // 5 Hz AC coupling at the prepared rate.
        debug_assert!(rate > 0.0, "sample rate must be positive");
        self.dc_r = (-2.0 * core::f64::consts::PI * 5.0 / f64::from(rate)).exp();
        self.reset();
    }
    fn reset(&mut self) {
        self.mag = 0.0;
        self.last_x = 0.0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let an = (self.drive * x).tanh();
        let dx = (x - self.last_x).abs();
        let rate = (self.hyst_rate * dx).min(1.0);
        self.mag = crate::primitives::denormal::flush(self.mag + (an - self.mag) * rate);
        self.last_x = x;
        // Small-signal normalised (÷drive): unity below the knee, so a
        // host can fade the stage in without a level jump.
        let y = (self.reversible * an + (1.0 - self.reversible) * self.mag) / self.drive.max(0.05);
        // one-pole DC blocker — f64 state (see field note)
        let out = f64::from(y) - self.dc_x1 + self.dc_r * self.dc_y1;
        self.dc_x1 = f64::from(y);
        self.dc_y1 = crate::primitives::denormal::flush64(out);
        out as f32
    }
}
