//! Telemetry — everything eddy reports out (the other end of the
//! framework's `attach_telemetry` seam): the wave view's published
//! cycle and its live phase cursor. Zero-cost with no bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    lfo_phase: Tap,
    tail: Tap,
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self {
            bus: None,
            lfo_phase: Tap::new("lfo_phase"),
            tail: Tap::new("tail"),
        }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    /// Publish the shaped cycle for the wave view. `draw` fills the
    /// slots only when a bus is attached — the picture costs nothing
    /// headless.
    pub(super) fn publish_cycle(&self, draw: impl FnOnce(&mut [f32])) {
        if let Some(bus) = &self.bus {
            let mut ys = [0.0f32; stave_core::debug::CURVE_POINTS];
            draw(&mut ys);
            bus.publish_curve(&ys);
        }
    }

    /// The wave view's cursor: phase 0..1 through the cycle.
    pub(super) fn publish_phase(&self, phase01: f32) {
        if let Some(bus) = &self.bus {
            self.lfo_phase.set(bus, phase01 as f64);
        }
    }

    /// The tail view's feed: the block's wet peak, mapped through a
    /// 60 dB window to 0..1 (ears are logarithmic — a linear peak
    /// would show only the first repeat or two).
    pub(super) fn publish_tail(&self, wet_peak: f32) {
        if let Some(bus) = &self.bus {
            let db = 20.0 * wet_peak.max(1e-6).log10();
            self.tail
                .set(bus, f64::from((1.0 + db / 60.0).clamp(0.0, 1.0)));
        }
    }
}
