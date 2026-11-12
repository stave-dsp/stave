//! `Scope` — framework-owned capture scratch and load meter, so engines
//! do not hand-roll capture buffers and CPU smoothing. Two channels
//! because the capture wire is transfer pairs (input vs output);
//! allocation happens in `prepare` (message thread), never on the audio
//! path.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::Telemetry;

pub struct Scope {
    ch0: Vec<f32>,
    ch1: Vec<f32>,
    load: f64,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub const fn new() -> Self {
        Self {
            ch0: Vec::new(),
            ch1: Vec::new(),
            load: 0.0,
        }
    }

    /// Size for the block bound (allocates — message thread).
    pub fn prepare(&mut self, max_frames: usize) {
        self.ch0.resize(max_frames.max(16), 0.0);
        self.ch1.resize(max_frames.max(16), 0.0);
        self.load = 0.0;
    }

    /// Record one frame's capture pair (audio thread; silently drops
    /// frames beyond the prepared bound).
    #[inline]
    pub fn write(&mut self, frame: usize, a: f32, b: f32) {
        if frame < self.ch0.len() {
            self.ch0[frame] = a;
            self.ch1[frame] = b;
        }
    }

    /// Ship this block's pair to the capture wire.
    pub fn publish(&self, telemetry: &Telemetry, frames: usize) {
        let n = frames.min(self.ch0.len()).min(self.ch1.len());
        if n > 0 {
            telemetry.write_capture(&[&self.ch0[..n], &self.ch1[..n]], telemetry.block());
        }
    }

    /// Fold one block's elapsed/budget into the smoothed load figure
    /// (fast attack, slow release) and return it as a percentage.
    pub fn load_pct(&mut self, elapsed_s: f64, budget_s: f64) -> f64 {
        let load = elapsed_s / budget_s;
        let coeff = if load > self.load { 0.5 } else { 0.05 };
        self.load += coeff * (load - self.load);
        self.load * 100.0
    }

    /// [`publish`](Self::publish) when telemetry is attached.
    pub fn publish_to(&self, telemetry: &Option<Arc<Telemetry>>, frames: usize) {
        if let Some(telemetry) = telemetry {
            self.publish(telemetry, frames);
        }
    }
}
