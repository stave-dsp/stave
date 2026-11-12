//! The taps every format wrapper publishes for a stave plugin.

use crate::telemetry::{Tap, Taps};

/// Published by the wrapper around every `process` call: peak level in
/// and out (dBFS, −120 floor; all channels, left, right) for the editor's
/// meter rails, and CPU load — the plugin's process time over the block's
/// realtime budget (0..1+). Hosts and tools read them by id
/// (`WrapperTaps::CPU`).
#[derive(Taps)]
pub struct WrapperTaps {
    pub io_in: Tap,
    pub io_out: Tap,
    pub io_in_l: Tap,
    pub io_in_r: Tap,
    pub io_out_l: Tap,
    pub io_out_r: Tap,
    pub cpu: Tap,
}

impl WrapperTaps {
    /// The level every rail reads in silence.
    pub const FLOOR_DB: f64 = -120.0;

    /// Publish one block's peaks: (all, left, right) in and out.
    pub fn publish_peaks(&self, input: [f64; 3], output: [f64; 3]) {
        self.io_in.set(input[0]);
        self.io_in_l.set(input[1]);
        self.io_in_r.set(input[2]);
        self.io_out.set(output[0]);
        self.io_out_l.set(output[1]);
        self.io_out_r.set(output[2]);
    }
}
