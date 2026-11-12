//! Hysteresis — path-dependent magnetization (tape/transformer).
use crate::kernel::node::{Stage, Tail};
use crate::primitives::filters::one_pole::DcBlocker;

/// The Jiles-Atherton split, behaviourally: a reversible part follows
/// the anhysteretic curve instantly; the irreversible part (remanence)
/// approaches it at a rate proportional to signal motion, opening a
/// frequency-independent loop. AC-coupled (5 Hz) so remanence doesn't
/// linger into silence.
pub struct Hysteresis {
    drive: f32,
    /// 0 = none (pure anhysteretic), 1 = wide loop.
    depth: f32,
    reversible: f32,
    hyst_rate: f32,
    mag: f32,
    last_x: f32,
    dc: DcBlocker,
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
            dc: DcBlocker::new(5.0),
        };
        h.set_depth(depth);
        h
    }
    pub fn set_depth(&mut self, depth: f32) {
        debug_assert!((0.0..=1.0).contains(&depth), "depth {depth}");
        self.depth = depth;
        // Deeper ⇒ slower path-approach ⇒ wider loop.
        self.hyst_rate = 30.0 * (1.0 - 0.9 * self.depth).powi(2) + 1.5;
    }
    pub fn set_drive(&mut self, drive: f32) {
        debug_assert!(drive > 0.0, "drive must be positive");
        self.drive = drive;
    }
}

impl Stage for Hysteresis {
    fn prepare(&mut self, sample_rate: f32) {
        self.dc.prepare(sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.mag = 0.0;
        self.last_x = 0.0;
        self.dc.reset();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let an = (self.drive * x).tanh();
        let dx = (x - self.last_x).abs();
        let rate = (self.hyst_rate * dx).min(1.0);
        self.mag = crate::kernel::denormal::flush(self.mag + (an - self.mag) * rate);
        self.last_x = x;
        // Small-signal normalised (÷drive): unity below the knee, so a
        // host can fade the stage in without a level jump.
        let y = (self.reversible * an + (1.0 - self.reversible) * self.mag) / self.drive.max(0.05);
        self.dc.tick(y)
    }
    /// Remanence freezes when the input stops moving; the DC blocker
    /// drains what it leaves at the output.
    fn tail(&self) -> Tail {
        self.dc.tail()
    }
}
