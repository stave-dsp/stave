//! Telemetry — each string's ring level, on a 48 dB window (0 = still,
//! 1 = full), one tap per pitch class, plus the harp's CLOCK (seconds
//! of audio played): the face draws each string vibrating at its
//! level, in time.
//! Zero-cost with no bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

use super::detect::CLASSES;
use crate::{CLOCK_TAP, LED_TAPS};

const WINDOW_DB: f32 = 48.0;

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    leds: [Tap; CLASSES],
    clock: Tap,
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self { bus: None, leds: core::array::from_fn(|c| Tap::new(LED_TAPS[c])), clock: Tap::new(CLOCK_TAP) }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    pub(super) fn publish(&self, levels: &[f32; CLASSES], clock: f64) {
        if let Some(bus) = &self.bus {
            self.clock.set(bus, clock);
            for (tap, l) in self.leds.iter().zip(levels) {
                let db = 20.0 * l.max(1e-7).log10();
                tap.set(bus, f64::from((1.0 + db / WINDOW_DB).clamp(0.0, 1.0)));
            }
        }
    }
}
