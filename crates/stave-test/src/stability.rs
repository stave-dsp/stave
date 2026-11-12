//! The sustained-drive stability invariant, reusable: drive a
//! recirculating system with a resonant tone at torture settings and
//! assert the energy TREND is flat. The measurement doctrine is
//! carved from eddy's field incidents (an unnormalized bleed product
//! at 1e24, a mesh rotation eigenvalue riding 115×, plate wander
//! pumping the hold loop 1.16×):
//! * a real pump COMPOUNDS — the caught ones ran 2–4%/s, 1.6×+ over
//!   twenty seconds — while moving voices' frequency responses
//!   WANDER, and short windows read that amplitude wander as growth;
//! * so the metric is energy over long windows far apart, both taken
//!   well past settling.

use stave_measure::signal::{Signal, Sine};

#[derive(Clone, Copy, Debug)]
pub struct StabilityConfig {
    /// Before the first window. 15 s: τ = 3 s at feedback 0.95, past 4τ.
    pub settle_seconds: f32,
    pub window_seconds: f32,
    /// From the first window's start to the second's.
    pub spacing_seconds: f32,
    /// Late over early energy. 1.35 clears measured voice wander with
    /// margin while a compounding pump blows through it.
    pub max_growth: f64,
    /// 173 Hz sits off every common delay/lattice harmonic: a tone ON a
    /// loop resonance flatters the system; one beside it makes the loop
    /// work.
    pub tone_hz: f64,
    pub tone_amplitude: f64,
}

impl Default for StabilityConfig {
    fn default() -> Self {
        Self {
            settle_seconds: 15.0,
            window_seconds: 5.0,
            spacing_seconds: 20.0,
            max_growth: 1.35,
            tone_hz: 173.0,
            tone_amplitude: 0.5,
        }
    }
}

impl StabilityConfig {
    /// Audio the trend needs: settle, then both windows.
    pub fn seconds(&self) -> f32 {
        self.settle_seconds + self.spacing_seconds + self.window_seconds
    }
}

/// The two windows and their ratio; `ratio > 1` is growth.
#[derive(Clone, Copy, Debug)]
pub struct StabilityReport {
    pub early: f64,
    pub late: f64,
    pub ratio: f64,
}

pub fn energy_trend(out: &[f32], sample_rate: f32, config: &StabilityConfig) -> StabilityReport {
    let frames = |s: f32| (s * sample_rate) as usize;
    let w = frames(config.window_seconds);
    let early_at = frames(config.settle_seconds);
    let late_at = frames(config.settle_seconds + config.spacing_seconds);
    assert!(
        out.len() >= late_at + w,
        "stability check needs {} s of audio ({} frames given, {} required)",
        config.seconds(),
        out.len(),
        late_at + w
    );
    let energy = |at: usize| -> f64 {
        out[at..at + w]
            .iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum()
    };
    let early = energy(early_at);
    let late = energy(late_at);
    StabilityReport {
        early,
        late,
        ratio: late / early.max(1e-30),
    }
}

/// The config's tone, as long as the trend needs.
pub fn resonant_tone(sample_rate: f32, config: &StabilityConfig) -> Vec<f32> {
    let tone = Sine {
        frequency_hz: config.tone_hz,
        amplitude: config.tone_amplitude,
        sample_rate: f64::from(sample_rate),
    };
    tone.samples(0, (config.seconds() * sample_rate) as usize)
}

/// The invariant: sustained drive must not TREND up. Panics with the
/// measured ratio; `label` names the configuration under torture.
pub fn assert_no_pump(out: &[f32], sample_rate: f32, config: &StabilityConfig, label: &str) {
    let r = energy_trend(out, sample_rate, config);
    assert!(
        r.ratio < config.max_growth,
        "{label}: sustained drive TRENDING UP ({:.3}× over {} s — a pump, not wander)",
        r.ratio,
        config.spacing_seconds
    );
}
