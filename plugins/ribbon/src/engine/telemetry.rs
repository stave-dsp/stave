//! Telemetry — where the loop is (0..1 of a turn: the face draws the
//! ribbon and its splice there, and turns the rollers by it) and how
//! fast the motor runs. Zero-cost with no bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

use crate::{LOOP_TAP, SPEED_TAP};

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    phase: Tap,
    speed: Tap,
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self { bus: None, phase: Tap::new(LOOP_TAP), speed: Tap::new(SPEED_TAP) }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    pub(super) fn publish(&self, phase: f64, speed: f32) {
        if let Some(bus) = &self.bus {
            self.phase.set(bus, phase);
            self.speed.set(bus, f64::from(speed));
        }
    }
}
