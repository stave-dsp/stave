//! A plugin's CPU budget (DSP.md P6): what one channel may cost, in
//! nanoseconds of CPU a sample at 48 kHz, declared beside its identity
//! and held by `stave_test::plugin_checks!`'s release perf check.
//!
//! The budget is the plugin author's statement, never the framework's:
//! a plugin that declares none ([`Budget::UNDECLARED`], the default) has
//! its cost measured and printed against real time, and nothing fails.
//! There is no default cap, because no number would fit both a one-knob
//! EQ and a circuit model oversampled 16×.
//!
//! ```
//! use stave_core::Budget;
//!
//! // 40 ns a sample a channel at 48 kHz: 0.19% of one core's real time.
//! const TILT: Budget = Budget::per_channel(40.0);
//! assert_eq!(TILT.ns_per_sample(), Some(40.0));
//! assert!((TILT.share_of_realtime().unwrap() - 0.00192).abs() < 1e-5);
//!
//! // The same statement as a share: a twentieth of a core a channel.
//! const AMP: Budget = Budget::share_per_channel(0.05);
//! assert!((AMP.ns_per_sample().unwrap() - 1041.67).abs() < 0.01);
//!
//! assert_eq!(Budget::UNDECLARED.ns_per_sample(), None);
//! ```

/// What one channel of a plugin may cost: CPU nanoseconds a sample at
/// [`Budget::SAMPLE_RATE`], in the plugin's worst parameter state.
///
/// Per channel: the measured cost of a frame over the larger of the
/// main input and output channel counts, so a stereo plugin's two
/// channels each get the budget. At 48 kHz whatever the host runs at,
/// and with the plugin's own oversampling: a plugin that oversamples
/// pays for it inside its budget.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Budget {
    ns_per_sample: Option<f64>,
}

impl Budget {
    /// The rate every budget is stated at.
    pub const SAMPLE_RATE: f64 = 48_000.0;

    /// One sample period at [`SAMPLE_RATE`](Self::SAMPLE_RATE): a whole
    /// core's real time, 20 833 ns.
    pub const REALTIME_NS: f64 = 1e9 / Self::SAMPLE_RATE;

    /// No budget: the perf check measures and reports, and never fails.
    pub const UNDECLARED: Budget = Budget { ns_per_sample: None };

    /// At most `ns_per_sample` nanoseconds of CPU a sample a channel at
    /// 48 kHz. Positive and finite, or a compile error in a `const`.
    pub const fn per_channel(ns_per_sample: f64) -> Budget {
        assert!(
            ns_per_sample.is_finite() && ns_per_sample > 0.0,
            "a budget is a positive, finite ns a sample"
        );
        Budget { ns_per_sample: Some(ns_per_sample) }
    }

    /// At most `share` of one core's real time a channel at 48 kHz (1 is
    /// the whole core). In (0, 1], or a compile error in a `const`.
    pub const fn share_per_channel(share: f64) -> Budget {
        assert!(share > 0.0 && share <= 1.0, "a share of real time is in (0, 1]");
        Budget::per_channel(share * Self::REALTIME_NS)
    }

    /// The budget in ns a sample a channel; `None` when undeclared.
    pub const fn ns_per_sample(self) -> Option<f64> {
        self.ns_per_sample
    }

    /// The budget as a share of one core's real time a channel.
    pub fn share_of_realtime(self) -> Option<f64> {
        self.ns_per_sample.map(|ns| ns / Self::REALTIME_NS)
    }

    /// Whether the plugin declared a budget.
    pub const fn is_declared(self) -> bool {
        self.ns_per_sample.is_some()
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::UNDECLARED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_its_fraction_of_the_sample_period() {
        let b = Budget::share_per_channel(0.5);
        assert!((b.ns_per_sample().unwrap() - 10_416.666).abs() < 1e-2);
        assert!((b.share_of_realtime().unwrap() - 0.5).abs() < 1e-12);
        assert!(!Budget::default().is_declared());
    }

    #[test]
    #[should_panic(expected = "positive, finite")]
    fn a_budget_of_nothing_is_refused() {
        let _ = Budget::per_channel(0.0);
    }
}
