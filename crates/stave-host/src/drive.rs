//! Running: drive any [`Instance`] block by block from a [`Signal`]
//! and a [`NoteSchedule`], handing each processed block to a callback
//! (with the CPU time `process` took). Bench phases and validation checks are
//! all a `Driver` plus what they do with the blocks.

use std::time::Duration;

use crate::instance::Instance;
use crate::note::{NoteEvent, NoteSchedule};
use stave_measure::signal::Signal;

/// This thread's CPU time so far.
fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid out-pointer; the clock exists on every unix
    // the host builds for.
    let failed = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) } != 0;
    debug_assert!(!failed, "CLOCK_THREAD_CPUTIME_ID unavailable");
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// One processed block, as handed to a run callback.
pub struct BlockOut<'a> {
    start: u64,
    frames: usize,
    inputs: &'a [Vec<f32>],
    outputs: &'a [Vec<f32>],
    process_time: Duration,
}

impl BlockOut<'_> {
    /// Absolute frame of the block's first sample (driver-relative).
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Frames in this block.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// CPU time the `process` call took on this thread. Not wall time:
    /// other threads and processes sharing the machine (tests run in
    /// parallel) would be charged to the plugin.
    pub fn process_time(&self) -> Duration {
        self.process_time
    }

    pub fn output(&self, channel: usize) -> &[f32] {
        &self.outputs[channel][..self.frames]
    }

    /// Every output channel, this block's frames (allocates nothing).
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = &[f32]> + Clone + '_ {
        self.outputs.iter().map(|o| &o[..self.frames])
    }

    pub fn input(&self, channel: usize) -> &[f32] {
        &self.inputs[channel][..self.frames]
    }
}

/// Owns the buffers and the running frame count for one instance.
pub struct Driver<'i> {
    inst: &'i mut dyn Instance,
    block: usize,
    inputs: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
    notes: Vec<NoteEvent>,
    frame: u64,
    processed: u64,
}

impl<'i> Driver<'i> {
    /// Blocks of the instance's `max_block`.
    pub fn new(inst: &'i mut dyn Instance) -> Self {
        let block = inst.layout().max_block;
        Self::with_block(inst, block)
    }

    pub fn with_block(inst: &'i mut dyn Instance, block: usize) -> Self {
        let l = inst.layout();
        let block = block.clamp(1, l.max_block);
        Self {
            inst,
            block,
            inputs: vec![vec![0.0; block]; l.inputs],
            outputs: vec![vec![0.0; block]; l.outputs],
            notes: Vec::new(),
            frame: 0,
            processed: 0,
        }
    }

    pub fn instance(&mut self) -> &mut dyn Instance {
        self.inst
    }

    /// The clock: the frame the next block starts at. Signals and note
    /// schedules are read at this frame.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Frames processed in total (never reset).
    pub fn processed(&self) -> u64 {
        self.processed
    }

    /// Restart the clock at 0 (the instance keeps its state): the next
    /// run plays its signal and notes from their beginning.
    pub fn reset_clock(&mut self) {
        self.frame = 0;
    }

    pub fn block(&self) -> usize {
        self.block
    }

    /// Run `frames` frames, calling `on_block` after each block.
    pub fn run(
        &mut self,
        frames: u64,
        signal: &dyn Signal,
        notes: &NoteSchedule,
        mut on_block: impl FnMut(&BlockOut),
    ) {
        let end = self.frame + frames;
        while self.frame < end {
            let n = (end - self.frame).min(self.block as u64) as usize;
            signal.fill(self.frame, &mut self.inputs, n);
            self.notes.clear();
            notes.block(self.frame, n, &mut self.notes);
            let ins: Vec<&[f32]> = self.inputs.iter().map(|b| &b[..n]).collect();
            let mut outs: Vec<&mut [f32]> = self.outputs.iter_mut().map(|b| &mut b[..n]).collect();
            let t0 = thread_cpu_time();
            self.inst.process(&ins, &mut outs, &self.notes);
            let process_time = thread_cpu_time().saturating_sub(t0);
            on_block(&BlockOut {
                start: self.frame,
                frames: n,
                inputs: &self.inputs,
                outputs: &self.outputs,
                process_time,
            });
            self.frame += n as u64;
            self.processed += n as u64;
        }
    }

    /// Run and discard (warm-up, settling).
    pub fn skip(&mut self, frames: u64, signal: &dyn Signal, notes: &NoteSchedule) {
        self.run(frames, signal, notes, |_| {});
    }

    /// Run and keep every output channel.
    pub fn capture(&mut self, frames: u64, signal: &dyn Signal, notes: &NoteSchedule) -> Capture {
        let channels = self.outputs.len();
        let mut out = vec![Vec::with_capacity(frames as usize); channels];
        let start = self.frame;
        self.run(frames, signal, notes, |b| {
            for (c, ch) in out.iter_mut().enumerate() {
                ch.extend_from_slice(b.output(c));
            }
        });
        Capture {
            start,
            channels: out,
        }
    }

    /// Seconds → frames at the instance's rate.
    pub fn frames_for(&self, seconds: f64) -> u64 {
        (seconds.max(0.0) * self.inst.layout().sample_rate).round() as u64
    }
}

/// Captured output, per channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// Driver frame of the first captured sample.
    pub start: u64,
    pub channels: Vec<Vec<f32>>,
}

impl Capture {
    pub fn channel(&self, c: usize) -> &[f32] {
        &self.channels[c]
    }

    pub fn slices(&self) -> Vec<&[f32]> {
        self.channels.iter().map(Vec::as_slice).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Note;
    use crate::testing::{Delay, Gain, Synth, BLOCK};
    use stave_measure::signal::{Impulse, Silence};

    #[test]
    fn capture_is_continuous_across_blocks() {
        let mut d = Delay::new(1, 5);
        let mut drv = Driver::new(&mut d);
        let cap = drv.capture(
            3 * BLOCK as u64 + 7,
            &Impulse { frame: 100 },
            &NoteSchedule::default(),
        );
        assert_eq!(cap.channel(0).len(), 3 * BLOCK + 7);
        assert_eq!(cap.channel(0).iter().position(|&v| v == 1.0), Some(105));
        assert_eq!(drv.frame(), 3 * BLOCK as u64 + 7);
    }

    #[test]
    fn signal_frames_continue_between_runs() {
        let mut d = Delay::new(1, 1);
        let mut drv = Driver::new(&mut d);
        drv.skip(10, &Silence, &NoteSchedule::default());
        let cap = drv.capture(10, &Impulse { frame: 12 }, &NoteSchedule::default());
        assert_eq!(
            (cap.start, cap.channel(0)[3]),
            (10, 1.0),
            "impulse at 12 → out at 13"
        );
    }

    #[test]
    fn reset_clock_replays_the_signal_from_its_start() {
        let mut g = Gain::new(1, 1);
        let mut drv = Driver::new(&mut g);
        drv.skip(1000, &Silence, &NoteSchedule::default());
        drv.reset_clock();
        let cap = drv.capture(8, &Impulse { frame: 2 }, &NoteSchedule::default());
        assert_eq!((cap.start, cap.channel(0)[2]), (0, 1.0));
    }

    #[test]
    fn notes_are_delivered_in_their_blocks() {
        let mut s = Synth::new();
        let mut drv = Driver::new(&mut s);
        let rate = drv.instance().layout().sample_rate;
        let on = (BLOCK as f64 * 2.5) / rate;
        let notes = NoteSchedule::new(
            &[Note {
                pitch: 60,
                velocity: 1.0,
                on_s: on,
                off_s: 1.0,
            }],
            rate,
        );
        let cap = drv.capture(4 * BLOCK as u64, &Silence, &notes);
        let first = cap.channel(0).iter().position(|&v| v != 0.0);
        assert_eq!(first, Some(BLOCK * 5 / 2));
    }

    #[test]
    fn callback_sees_timing_and_inputs() {
        let mut g = Gain::new(1, 1);
        let mut drv = Driver::with_block(&mut g, 16);
        let mut blocks = 0;
        drv.run(40, &Impulse { frame: 17 }, &NoteSchedule::default(), |b| {
            blocks += 1;
            assert!(b.frames() <= 16);
            assert_eq!(b.input(0), b.output(0), "unity gain");
        });
        assert_eq!(blocks, 3);
    }
}
