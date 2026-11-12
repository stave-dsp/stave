//! Loudness per ITU-R BS.1770-4 / EBU R128: K-weighted, channel-weighted
//! mean square over sliding blocks; integrated loudness gated at −70 LUFS
//! absolute and −10 LU relative.

use super::{KWeighting, LevelMeasure, LevelMeter, StreamSpec};

const LOUDNESS_OFFSET_DB: f64 = -0.691;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;
const STEP_SECONDS: f64 = 0.1;
const SURROUND_WEIGHT: f64 = 1.41;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LufsWindow {
    /// The loudest 400 ms block.
    Momentary,
    /// The loudest 3 s block.
    ShortTerm,
    /// The gated whole programme.
    Integrated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lufs {
    pub window: LufsWindow,
    /// Per-channel weights; `None` uses the BS.1770 defaults for the
    /// channel count (1.0 each; 5.1 = L R C LFE Ls Rs → 1, 1, 1, 0, 1.41, 1.41).
    pub channel_weights: Option<Vec<f64>>,
}

impl Lufs {
    pub fn momentary() -> Self {
        Self {
            window: LufsWindow::Momentary,
            channel_weights: None,
        }
    }

    pub fn short_term() -> Self {
        Self {
            window: LufsWindow::ShortTerm,
            channel_weights: None,
        }
    }

    pub fn integrated() -> Self {
        Self {
            window: LufsWindow::Integrated,
            channel_weights: None,
        }
    }

    fn weights(&self, channels: usize) -> Vec<f64> {
        match &self.channel_weights {
            Some(w) => w.clone(),
            None if channels == 6 => vec![1.0, 1.0, 1.0, 0.0, SURROUND_WEIGHT, SURROUND_WEIGHT],
            None => vec![1.0; channels],
        }
    }

    fn block_seconds(&self) -> f64 {
        match self.window {
            LufsWindow::ShortTerm => 3.0,
            LufsWindow::Momentary | LufsWindow::Integrated => 0.4,
        }
    }
}

impl LevelMeasure for Lufs {
    fn name(&self) -> &'static str {
        match self.window {
            LufsWindow::Momentary => "lufs-m",
            LufsWindow::ShortTerm => "lufs-s",
            LufsWindow::Integrated => "lufs-i",
        }
    }

    fn unit(&self) -> &'static str {
        "LUFS"
    }

    fn meter(&self, stream: &StreamSpec) -> Box<dyn LevelMeter> {
        let StreamSpec {
            channels,
            sample_rate,
            ..
        } = *stream;
        let step_len = (STEP_SECONDS * sample_rate).round().max(1.0) as usize;
        Box::new(LufsMeter {
            window: self.window,
            filters: vec![KWeighting::new(sample_rate); channels],
            weights: self.weights(channels),
            step_len,
            steps_per_block: (self.block_seconds() / STEP_SECONDS).round() as usize,
            pending_energy: 0.0,
            pending_frames: 0,
            step_energies: Vec::new(),
            block_powers: Vec::new(),
        })
    }
}

fn loudness(mean_square: f64) -> f64 {
    LOUDNESS_OFFSET_DB + 10.0 * mean_square.log10()
}

fn integrated(block_powers: &[f64]) -> Option<f64> {
    let gated_mean = |threshold: f64| {
        let kept: Vec<f64> = block_powers
            .iter()
            .copied()
            .filter(|&p| loudness(p) > threshold)
            .collect();
        (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
    };
    let relative_gate = loudness(gated_mean(ABSOLUTE_GATE_LUFS)?) + RELATIVE_GATE_LU;
    gated_mean(relative_gate.max(ABSOLUTE_GATE_LUFS)).map(loudness)
}

/// K-weighted, channel-weighted energy summed per 100 ms step; a block is
/// the last `steps_per_block` steps.
struct LufsMeter {
    window: LufsWindow,
    filters: Vec<KWeighting>,
    weights: Vec<f64>,
    step_len: usize,
    steps_per_block: usize,
    pending_energy: f64,
    pending_frames: usize,
    step_energies: Vec<f64>,
    block_powers: Vec<f64>,
}

impl LufsMeter {
    fn complete_step(&mut self) {
        self.step_energies.push(self.pending_energy);
        self.pending_energy = 0.0;
        self.pending_frames = 0;
        if let Some(block) = self.step_energies.rchunks(self.steps_per_block).next() {
            if block.len() == self.steps_per_block {
                let frames = (self.steps_per_block * self.step_len) as f64;
                self.block_powers.push(block.iter().sum::<f64>() / frames);
            }
        }
    }
}

impl LevelMeter for LufsMeter {
    fn push(&mut self, block: &[&[f32]]) {
        let frames = block.iter().map(|c| c.len()).min().unwrap_or(0);
        for i in 0..frames {
            let energy: f64 = block
                .iter()
                .zip(&mut self.filters)
                .zip(&self.weights)
                .map(|((channel, filter), weight)| {
                    let y = filter.process(channel[i]);
                    weight * y * y
                })
                .sum();
            self.pending_energy += energy;
            self.pending_frames += 1;
            if self.pending_frames == self.step_len {
                self.complete_step();
            }
        }
    }

    fn level(&self) -> Option<f64> {
        match self.window {
            LufsWindow::Momentary | LufsWindow::ShortTerm => self
                .block_powers
                .iter()
                .copied()
                .map(loudness)
                .reduce(f64::max),
            LufsWindow::Integrated => integrated(&self.block_powers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn sine(dbfs: f64, seconds: f64) -> Vec<f32> {
        let amp = 10f64.powf(dbfs / 20.0);
        (0..(seconds * RATE) as usize)
            .map(|i| (amp * (std::f64::consts::TAU * 1_000.0 * i as f64 / RATE).sin()) as f32)
            .collect()
    }

    fn stereo(x: &[f32]) -> [&[f32]; 2] {
        [x, x]
    }

    #[test]
    fn ebu_3341_case_1_stereo_sine_at_minus_23() {
        let x = sine(-23.0, 20.0);
        for lufs in [Lufs::momentary(), Lufs::short_term(), Lufs::integrated()] {
            let got = lufs.measure(&stereo(&x), RATE).unwrap();
            assert!((got + 23.0).abs() < 0.1, "{}: {got}", lufs.name());
        }
    }

    #[test]
    fn ebu_3341_case_2_stereo_sine_at_minus_33() {
        let got = Lufs::integrated()
            .measure(&stereo(&sine(-33.0, 20.0)), RATE)
            .unwrap();
        assert!((got + 33.0).abs() < 0.1, "{got}");
    }

    #[test]
    fn ebu_3341_case_3_relative_gate_drops_the_quiet_ends() {
        let x: Vec<f32> = [sine(-36.0, 10.0), sine(-23.0, 60.0), sine(-36.0, 10.0)].concat();
        let got = Lufs::integrated().measure(&stereo(&x), RATE).unwrap();
        assert!((got + 23.0).abs() < 0.1, "{got}");
    }

    #[test]
    fn absolute_gate_ignores_silence() {
        let x: Vec<f32> = [vec![0.0; 10 * RATE as usize], sine(-23.0, 20.0)].concat();
        let got = Lufs::integrated().measure(&stereo(&x), RATE).unwrap();
        assert!((got + 23.0).abs() < 0.1, "{got}");
        assert_eq!(
            Lufs::integrated().measure(&stereo(&vec![0.0; 48_000]), RATE),
            None
        );
    }

    #[test]
    fn mono_full_scale_997_hz_reads_minus_three() {
        let x: Vec<f32> = (0..(10.0 * RATE) as usize)
            .map(|i| (std::f64::consts::TAU * 997.0 * i as f64 / RATE).sin() as f32)
            .collect();
        let got = Lufs::integrated().measure(&[&x], RATE).unwrap();
        assert!((got + 3.01).abs() < 0.05, "{got}");
    }

    #[test]
    fn surround_weights_by_channel_count() {
        assert_eq!(
            Lufs::integrated().weights(6),
            [1.0, 1.0, 1.0, 0.0, 1.41, 1.41]
        );
        assert_eq!(Lufs::integrated().weights(2), [1.0, 1.0]);
    }

    #[test]
    fn too_short_for_a_block_is_undefined() {
        assert_eq!(
            Lufs::short_term().measure(&stereo(&sine(-23.0, 1.0)), RATE),
            None
        );
    }
}
