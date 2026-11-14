//! A stage's cost as telemetry taps (DSP.md P5): what each block of a
//! stage took, in nanoseconds a sample and as a share of the real-time
//! budget, so `stave-play`, `stave-bench --describe` and a plugin's face
//! show where the time goes in a DAW session — beside each circuit's
//! solver health ([`solver_taps`](crate::solver_taps)).
//!
//! A plugin registers one [`CostTaps`] per stage it wants seen when it is
//! built, gives it the sample rate when it is prepared, and wraps the
//! stage's block in [`measure`](CostTaps::measure) in `process`:
//!
//! ```
//! use stave_core::cost_taps::{CostTapIds, CostTaps};
//! use stave_core::telemetry::Telemetry;
//! use stave_dsp::{Biquad, FilterSpec, Stage};
//!
//! let telemetry = Telemetry::new();
//! let mut cost = CostTaps::new(&telemetry, CostTapIds::DEFAULT); // in Plugin::new
//! cost.prepare(48_000.0);                                        // in prepare
//!
//! let mut filter = Biquad::new(FilterSpec::Lowpass { frequency: Hz::new(1_000.0), q: 0.707 });
//! filter.prepare(48_000.0);
//! let mut block = [0.25f32; 64];
//! cost.measure(block.len(), || filter.process(&mut block));     // in process
//!
//! let ids = CostTapIds::DEFAULT;
//! let ns = telemetry.value(ids.ns_per_sample).unwrap();
//! let load = telemetry.value(ids.load).unwrap();
//! assert!(ns >= 0.0 && (load - ns * 48_000.0 / 1e9).abs() < 1e-9);
//! ```
//!
//! A plugin with more than one stage names each one's taps with
//! [`cost_tap_ids!`](crate::cost_tap_ids): `cost_tap_ids!("tape")`
//! publishes `tape_ns_per_sample` and `tape_load`.
//!
//! What each tap carries, per measured block:
//!
//! | Tap | Value |
//! |---|---|
//! | `ns_per_sample` | the block's wall-clock time over its samples, ns |
//! | `load` | that against the sample period: 1 is one core's whole real-time budget |
//!
//! Each slot also keeps its minimum, maximum and hit count, so the worst
//! block of a session stays visible. The time is wall clock on the audio
//! thread — what the host's deadline sees, preemption and cache misses
//! included — read from the monotonic clock (`std::time::Instant`: a
//! user-space read on macOS, Linux and Windows, no lock, no allocation).
//! For a deterministic cost, the datasheet and `scripts/icount.sh`
//! measure the same stages offline.

use std::time::Instant;

use crate::telemetry::{Tap, TapId, Telemetry};

/// The ids one stage's cost taps publish under.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CostTapIds {
    pub ns_per_sample: TapId,
    pub load: TapId,
}

impl CostTapIds {
    /// `cost_ns_per_sample`, `cost_load`.
    pub const DEFAULT: Self = crate::cost_tap_ids!("cost");

    /// Every id, in field order.
    pub const fn all(self) -> [TapId; 2] {
        [self.ns_per_sample, self.load]
    }
}

/// The [`CostTapIds`] for a stage called `$name` (a string literal):
/// `$name_ns_per_sample`, `$name_load`. A name that makes either id
/// longer than a telemetry slot holds is a compile error in a `const`.
///
/// ```
/// const TAPE: stave_core::cost_taps::CostTapIds = stave_core::cost_tap_ids!("tape");
/// assert_eq!(TAPE.load.name(), "tape_load");
/// ```
#[macro_export]
macro_rules! cost_tap_ids {
    ($name:literal) => {
        $crate::cost_taps::CostTapIds {
            ns_per_sample: $crate::telemetry::TapId::new(concat!($name, "_ns_per_sample")),
            load: $crate::telemetry::TapId::new(concat!($name, "_load")),
        }
    };
}

/// One stage's cost taps: register when the plugin is built,
/// [`prepare`](Self::prepare) with the sample rate, then
/// [`measure`](Self::measure) each block (realtime: two clock reads and
/// two tap writes).
pub struct CostTaps {
    pub ns_per_sample: Tap,
    pub load: Tap,
    /// The sample rate over 1e9: load per nanosecond a sample.
    per_ns: f64,
}

impl CostTaps {
    /// Register `ids` on `telemetry` (off the audio thread), prepared at
    /// 48 kHz.
    pub fn new(telemetry: &Telemetry, ids: CostTapIds) -> Self {
        Self {
            ns_per_sample: Tap::register(telemetry, ids.ns_per_sample),
            load: Tap::register(telemetry, ids.load),
            per_ns: 48_000.0 / 1e9,
        }
    }

    /// Taps bound to nothing (writes dropped).
    pub const fn detached() -> Self {
        Self {
            ns_per_sample: Tap::detached(),
            load: Tap::detached(),
            per_ns: 48_000.0 / 1e9,
        }
    }

    /// The rate `load` is measured against.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.per_ns = f64::from(sample_rate) / 1e9;
    }

    /// Run `block` — a stage's work on `samples` samples — and publish
    /// what it took. Returns what `block` does. A block of no samples
    /// publishes nothing.
    #[inline]
    pub fn measure<R>(&self, samples: usize, block: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let result = block();
        let nanos = start.elapsed().as_nanos() as f64;
        if samples > 0 {
            self.publish(nanos / samples as f64);
        }
        result
    }

    /// Publish a cost measured elsewhere, ns a sample.
    pub fn publish(&self, ns_per_sample: f64) {
        self.ns_per_sample.set(ns_per_sample);
        self.load.set(ns_per_sample * self.per_ns);
    }
}

impl Default for CostTaps {
    fn default() -> Self {
        Self::detached()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_is_the_cost_against_the_sample_period() {
        let telemetry = Telemetry::new();
        let ids = crate::cost_tap_ids!("amp");
        let mut cost = CostTaps::new(&telemetry, ids);
        cost.prepare(96_000.0);
        cost.publish(2_500.0);
        assert_eq!(telemetry.value(ids.ns_per_sample), Some(2_500.0));
        assert!((telemetry.value(ids.load).unwrap() - 0.24).abs() < 1e-12);
        // A measured block publishes; an empty one does not move the taps.
        let sum = cost.measure(64, || (0..64).map(f64::from).sum::<f64>());
        assert_eq!(sum, 2016.0);
        let measured = telemetry.value(ids.ns_per_sample).unwrap();
        assert!(measured >= 0.0 && measured != 2_500.0);
        cost.measure(0, || ());
        assert_eq!(telemetry.value(ids.ns_per_sample), Some(measured));
    }

    #[test]
    fn detached_taps_drop_their_writes() {
        let cost = CostTaps::detached();
        assert_eq!(cost.measure(16, || 7), 7);
    }
}
