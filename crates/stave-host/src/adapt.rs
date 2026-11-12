//! Adapters between the two shapes things are measured in: an
//! [`Instance`] (multichannel, hosted) and a [`System`] (mono, sample in
//! → sample out), plus any `stave-dsp` [`Stage`] as an instance.

use stave_dsp::Stage;
use stave_measure::system::System;

use std::num::NonZeroUsize;

use crate::config::SampleRate;
use crate::instance::{Instance, Layout, Tail};
use crate::note::NoteEvent;

/// One output channel of an instance, driven with the same signal on
/// every input channel.
pub struct InstanceChannel<'a> {
    pub instance: &'a mut dyn Instance,
    pub channel: usize,
}

impl System for InstanceChannel<'_> {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let layout = self.instance.layout();
        let mut outputs = vec![vec![0.0f32; layout.max_block]; layout.outputs];
        for (block_in, block_out) in input
            .chunks(layout.max_block)
            .zip(output.chunks_mut(layout.max_block))
        {
            let n = block_in.len();
            let inputs = vec![block_in; layout.inputs];
            let mut outs: Vec<&mut [f32]> = outputs.iter_mut().map(|o| &mut o[..n]).collect();
            self.instance.process(&inputs, &mut outs, &[]);
            block_out.copy_from_slice(&outputs[self.channel][..n]);
        }
    }
}

/// Builds a [`StageInstance`]: one stage per channel, prepared at
/// `sample_rate`.
pub struct StageInstanceConfig<S> {
    pub stages: Vec<S>,
    pub sample_rate: SampleRate,
    pub max_block: NonZeroUsize,
}

/// A `stave-dsp` stage per channel, hosted as an [`Instance`], so the
/// bench, validation and every measurement run on raw DSP modules.
pub struct StageInstance<S> {
    stages: Vec<S>,
    layout: Layout,
}

impl<S: Stage> StageInstance<S> {
    pub fn stages(&self) -> &[S] {
        &self.stages
    }

    pub fn stages_mut(&mut self) -> &mut [S] {
        &mut self.stages
    }

    pub fn new(config: StageInstanceConfig<S>) -> Self {
        let mut stages = config.stages;
        for stage in &mut stages {
            stage.prepare(config.sample_rate.hz() as f32);
        }
        let layout = Layout {
            inputs: stages.len(),
            outputs: stages.len(),
            sample_rate: config.sample_rate.hz(),
            max_block: config.max_block.get(),
        };
        Self { stages, layout }
    }
}

impl<S: Stage> Instance for StageInstance<S> {
    fn layout(&self) -> Layout {
        self.layout
    }

    fn latency(&self) -> u32 {
        self.stages.first().map_or(0, Stage::latency)
    }

    fn tail(&self) -> Tail {
        match self.stages.first().map(|s| s.tail().to_samples()) {
            Some(None) => Tail::Infinite,
            Some(Some(n)) => Tail::Samples(n),
            None => Tail::Samples(0),
        }
    }

    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        for ((stage, input), output) in self.stages.iter_mut().zip(inputs).zip(outputs.iter_mut()) {
            output.copy_from_slice(input);
            stage.process(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HostConfig;
    use crate::testing::Gain;
    use crate::validate::{self, Status};
    use stave_dsp::{Biquad, FilterSpec};
    use stave_measure::tone_test::ToneTest;

    fn peak_filter(channels: usize) -> StageInstance<Biquad> {
        let peak = FilterSpec::Peak {
            hz: 1_000.0,
            db: 6.0,
            q: 1.0,
        };
        StageInstance::new(StageInstanceConfig {
            stages: (0..channels).map(|_| Biquad::new(peak)).collect(),
            sample_rate: HostConfig::default().sample_rate,
            max_block: NonZeroUsize::new(64).unwrap(),
        })
    }

    #[test]
    fn a_dsp_stage_measured_through_the_host_reads_its_own_boost() {
        let mut filter = peak_filter(2);
        let at_centre = ToneTest {
            bin: 100,
            window: 4800,
            sample_rate: 48_000.0,
            amplitude: 0.25,
            warmup_windows: 4,
        };
        let gain = at_centre.gain_db(InstanceChannel {
            instance: &mut filter,
            channel: 1,
        });
        assert!((gain - 6.0).abs() < 0.05, "{gain}");
    }

    #[test]
    fn validation_runs_on_a_dsp_stage() {
        let mut fresh = || Ok(Box::new(peak_filter(2)) as Box<dyn Instance>);
        let report = validate::Suite::default().run(&mut fresh, |_| {});
        assert!(report.passed(), "{:#?}", report.checks);
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.status == Status::NotApplicable),
            "no controls or state to test"
        );
    }

    #[test]
    fn instance_channels_carry_hosted_gain() {
        let mut g = Gain::new(1, 2);
        g.set_control("gain", 0.5).unwrap();
        let mut ch = InstanceChannel {
            instance: &mut g,
            channel: 1,
        };
        let mut out = [0.0; 200];
        ch.process(&[1.0; 200], &mut out);
        assert!(out.iter().all(|&y| y == 0.5));
    }
}
