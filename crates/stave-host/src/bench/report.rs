//! What a bench run reports. Every field is measured, never inferred;
//! `phases` and `warnings` say how the numbers were obtained.

use crate::instance::TapReading;
use stave_measure::latency::ImpulseLatency;
use stave_measure::loudness::Reading;
use stave_measure::stats::Summary;
use stave_measure::step::StepResponse;
use stave_measure::stereo::StereoImage;
use stave_measure::tone::ToneReport;

use super::stimulus::Stimulus;

#[derive(Clone, Debug, serde::Serialize)]
pub struct BenchReport {
    pub sample_rate: f64,
    pub block: usize,
    pub stimulus: Stimulus,
    pub cpu: Option<CpuReport>,
    pub latency: LatencyReport,
    pub output: OutputReport,
    /// The plugin's own taps, read right after the timed run (so
    /// they describe it, not a later pass).
    pub taps: Vec<TapReading>,
    pub phases: Vec<PhaseTiming>,
    pub warnings: Vec<String>,
}

/// `process()` wall time over the timed run, three ways.
#[derive(Clone, Debug, serde::Serialize)]
pub struct CpuReport {
    pub blocks: usize,
    /// The realtime budget for one block, µs.
    pub budget_us: f64,
    pub us_per_block: Summary,
    pub ns_per_sample: Summary,
    /// Percent of the realtime budget.
    pub rt_pct: Summary,
}

/// The realtime deadline one block of `block_frames` must meet.
#[derive(Copy, Clone, Debug)]
pub struct CpuBudget {
    pub block_frames: usize,
    pub sample_rate: f64,
}

impl CpuReport {
    /// `None` when no blocks were timed.
    pub fn from_block_ns(block_ns: &[f64], budget: &CpuBudget) -> Option<Self> {
        let ns = Summary::of(block_ns)?;
        let block = budget.block_frames;
        let budget_ns = block as f64 / budget.sample_rate * 1e9;
        Some(Self {
            blocks: ns.count,
            budget_us: budget_ns / 1e3,
            us_per_block: ns.map(|v| v / 1e3),
            ns_per_sample: ns.map(|v| v / block as f64),
            rt_pct: ns.map(|v| v / budget_ns * 100.0),
        })
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LatencyReport {
    /// What the plugin declares for delay compensation.
    pub reported: u32,
    /// Per output channel; `None` where no impulse arrived. Absent for
    /// instruments (no input to put the impulse in).
    pub measured: Option<Vec<Option<ImpulseLatency>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct OutputReport {
    /// One reading per configured level measure.
    pub levels: Vec<Reading>,
    pub non_finite_samples: u64,
    /// Per channel: RMS of the second half of a 1 s silent run.
    pub silence_residual_dbfs: Vec<f64>,
    /// Per channel, when the stimulus was a sine.
    pub tone: Option<Vec<ToneReport>>,
    pub stereo: Option<StereoImage>,
    pub dc: Vec<DcReport>,
    /// Per channel, for effects.
    pub step: Option<Vec<StepResponse>>,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Serialize)]
pub struct DcReport {
    pub offset: f64,
    /// Second half of the analysis window minus the first.
    pub drift: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PhaseTiming {
    pub phase: &'static str,
    pub frames: u64,
    pub wall_ms: f64,
}
