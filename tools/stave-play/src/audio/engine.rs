//! The realtime engine: everything the audio callback does, separate
//! from the device. Allocation-free per callback (buffers, note list and
//! slice tables are sized once), and generic over [`Instance`] so it is
//! tested with pure-Rust doubles instead of a sound card.
//!
//! Per callback: drain GUI note events → the plugin AND the sine bank
//! (so effects are playable); per block: fill inputs (one-shot sample +
//! sines), tap the input, process, tap the output, interleave out.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_queue::ArrayQueue;
use stave_host::instance::Instance;
use stave_host::note::NoteEvent;

use crate::analysis::{AnalysisRing, MAX_CHANNELS};
use crate::debug_log::DebugLog;
use crate::samples::SampleTrigger;
use crate::synth::SineBank;

/// Channels the engine hands the plugin (beyond this the plugin's own
/// spare buffers absorb them).
const MAX_IO: usize = 8;
/// Note events drained per callback.
pub const MAX_EVENTS: usize = 256;

/// What the engine shares with the UI thread.
#[derive(Clone)]
pub struct Shared {
    pub events: Arc<ArrayQueue<NoteEvent>>,
    pub samples: Arc<ArrayQueue<SampleTrigger>>,
    pub peak: Arc<AtomicU32>,
    pub output_tap: Arc<AnalysisRing>,
    pub input_tap: Arc<AnalysisRing>,
    pub log: Arc<DebugLog>,
}

/// A one-shot sample playing through the plugin's inputs.
struct Voice {
    trigger: SampleTrigger,
    pos: usize,
}

impl Voice {
    /// Frame `f` of channel `c` (wrapping onto the sample's channels);
    /// `None` once the sample is over.
    fn frame(&self, f: usize, c: usize) -> Option<f32> {
        let stride = usize::from(self.trigger.channels.max(1));
        let i = (self.pos + f) * stride + c % stride;
        self.trigger.data.get(i).copied()
    }
}

pub struct Engine<P: Instance> {
    plugin: P,
    shared: Shared,
    ins: Vec<Vec<f32>>,
    outs: Vec<Vec<f32>>,
    notes: Vec<NoteEvent>,
    sines: SineBank,
    voice: Option<Voice>,
    rate: f64,
}

impl<P: Instance> Engine<P> {
    pub fn new(plugin: P, shared: Shared) -> Self {
        let l = plugin.layout();
        shared
            .output_tap
            .set_channels(l.outputs.clamp(1, MAX_CHANNELS));
        shared
            .input_tap
            .set_channels(l.inputs.clamp(1, MAX_CHANNELS));
        Self {
            ins: vec![vec![0.0; l.max_block]; l.inputs.min(MAX_IO)],
            outs: vec![vec![0.0; l.max_block]; l.outputs.clamp(1, MAX_IO)],
            notes: Vec::with_capacity(MAX_EVENTS),
            sines: SineBank::new(l.sample_rate as f32),
            voice: None,
            rate: l.sample_rate,
            plugin,
            shared,
        }
    }

    /// Fill one interleaved device buffer of `channels` channels.
    pub fn fill(&mut self, out: &mut [f32], channels: usize) {
        let t0 = Instant::now();
        let channels = channels.max(1);
        let total = out.len() / channels;
        self.drain_events();
        while let Some(t) = self.shared.samples.pop() {
            self.voice = Some(Voice { trigger: t, pos: 0 }); // newest wins
        }
        let block = self.plugin.layout().max_block;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(block);
            self.fill_inputs(n);
            self.process(n);
            self.interleave(
                &mut out[done * channels..(done + n) * channels],
                channels,
                n,
            );
            self.notes.clear(); // events land in the first block only
            done += n;
        }
        if self.plugin.layout().is_instrument() {
            self.mix_sample_after(out, channels);
        }
        self.account(out, total, t0);
    }

    fn drain_events(&mut self) {
        self.notes.clear();
        while self.notes.len() < MAX_EVENTS {
            let Some(e) = self.shared.events.pop() else {
                break;
            };
            self.sines.handle(&e);
            self.notes.push(NoteEvent { offset: 0, ..e });
        }
        let n = self.notes.len() as u64;
        self.shared
            .log
            .consumed_events
            .fetch_add(n, Ordering::Relaxed);
    }

    /// Effects hear the one-shot sample and the keyboard's sines.
    fn fill_inputs(&mut self, n: usize) {
        for (c, ch) in self.ins.iter_mut().enumerate() {
            for (f, s) in ch[..n].iter_mut().enumerate() {
                *s = self
                    .voice
                    .as_ref()
                    .and_then(|v| v.frame(f, c))
                    .unwrap_or(0.0);
            }
        }
        if let Some(v) = self.voice.as_mut() {
            v.pos += n;
            if v.frame(0, 0).is_none() {
                self.voice = None;
            }
        }
        self.sines.mix_into(&mut self.ins, n);
        if self.ins.is_empty() {
            return;
        }
        let tap: [&[f32]; MAX_CHANNELS] =
            std::array::from_fn(|c| self.ins.get(c).map_or(&[][..], |b| &b[..n]));
        self.shared
            .input_tap
            .write_block(&tap[..self.ins.len().min(MAX_CHANNELS)]);
    }

    fn process(&mut self, n: usize) {
        let (ni, no) = (self.ins.len(), self.outs.len());
        let ins: [&[f32]; MAX_IO] =
            std::array::from_fn(|c| self.ins.get(c).map_or(&[][..], |b| &b[..n]));
        let mut it = self.outs.iter_mut();
        let mut outs: [&mut [f32]; MAX_IO] =
            std::array::from_fn(|_| it.next().map_or(&mut [][..], |b| &mut b[..n]));
        self.plugin
            .process(&ins[..ni], &mut outs[..no], &self.notes);
        let tap: [&[f32]; MAX_CHANNELS] =
            std::array::from_fn(|c| self.outs.get(c).map_or(&[][..], |b| &b[..n]));
        self.shared
            .output_tap
            .write_block(&tap[..no.min(MAX_CHANNELS)]);
    }

    /// Plugin channels → device channels (extra device channels repeat
    /// the last plugin channel), clamped to [−1, 1].
    fn interleave(&self, out: &mut [f32], channels: usize, n: usize) {
        let last = self.outs.len() - 1;
        for f in 0..n {
            for c in 0..channels {
                out[f * channels + c] = self.outs[c.min(last)][f].clamp(-1.0, 1.0);
            }
        }
    }

    /// Instruments have no input: the sample plays over their output.
    fn mix_sample_after(&mut self, out: &mut [f32], channels: usize) {
        let Some(v) = self.voice.as_mut() else { return };
        let frames = out.len() / channels;
        for f in 0..frames {
            for c in 0..channels {
                let Some(s) = v.frame(f, c) else {
                    self.voice = None;
                    return;
                };
                out[f * channels + c] = (out[f * channels + c] + s * 0.8).clamp(-1.0, 1.0);
            }
        }
        v.pos += frames;
    }

    fn account(&self, out: &[f32], frames: usize, t0: Instant) {
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.shared.peak.store(peak.to_bits(), Ordering::Relaxed);
        let took_us = t0.elapsed().as_micros() as u32;
        // A callback slower than its own realtime span is a dropout.
        if f64::from(took_us) > frames as f64 * 1e6 / self.rate {
            self.shared
                .log
                .deadline_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        self.shared.log.record_callback(frames as u64, took_us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_host::control::Control;
    use stave_host::instance::Layout;

    /// A minimal instance: out = in (or a constant for instruments).
    struct Pass(Layout);
    impl Instance for Pass {
        fn layout(&self) -> Layout {
            self.0
        }
        fn process(&mut self, i: &[&[f32]], o: &mut [&mut [f32]], _: &[NoteEvent]) {
            for (c, out) in o.iter_mut().enumerate() {
                match i.get(c) {
                    Some(inp) => out.copy_from_slice(inp),
                    None => out.fill(0.25),
                }
            }
        }
        fn controls(&self) -> Vec<Control> {
            vec![]
        }
    }

    fn shared() -> Shared {
        Shared {
            events: Arc::new(ArrayQueue::new(MAX_EVENTS)),
            samples: Arc::new(ArrayQueue::new(4)),
            peak: Arc::new(AtomicU32::new(0)),
            output_tap: AnalysisRing::new(),
            input_tap: AnalysisRing::new(),
            log: DebugLog::new(),
        }
    }

    fn layout(inputs: usize, outputs: usize) -> Layout {
        Layout {
            inputs,
            outputs,
            sample_rate: 48_000.0,
            max_block: 64,
        }
    }

    #[test]
    fn a_held_key_is_audible_through_an_effect() {
        let sh = shared();
        let mut e = Engine::new(Pass(layout(2, 2)), sh.clone());
        let a4 = NoteEvent {
            offset: 0,
            pitch: 69,
            velocity: 1.0,
            kind: stave_host::note::NoteKind::On,
        };
        sh.events.push(a4).unwrap();
        let mut out = vec![0.0; 2 * 1000]; // not a multiple of the block
        e.fill(&mut out, 2);
        assert!(out.iter().any(|&v| v.abs() > 0.1));
        assert_eq!(sh.log.consumed_events.load(Ordering::Relaxed), 1);
        assert!(f32::from_bits(sh.peak.load(Ordering::Relaxed)) > 0.1);
    }

    #[test]
    fn mono_plugin_fills_every_device_channel() {
        let mut e = Engine::new(Pass(layout(0, 1)), shared());
        let mut out = vec![0.0; 3 * 10];
        e.fill(&mut out, 3);
        assert!(out.iter().all(|&v| v == 0.25));
    }

    #[test]
    fn sample_plays_once_through_an_effect() {
        let sh = shared();
        let mut e = Engine::new(Pass(layout(1, 1)), sh.clone());
        let data = Arc::new(vec![0.5f32; 100]);
        sh.samples
            .push(SampleTrigger { data, channels: 1 })
            .unwrap();
        let mut out = vec![0.0; 128];
        e.fill(&mut out, 1);
        assert!(out[..100].iter().all(|&v| v == 0.5) && out[100..].iter().all(|&v| v == 0.0));
        let mut again = vec![1.0; 64];
        e.fill(&mut again, 1);
        assert!(again.iter().all(|&v| v == 0.0), "one-shot");
    }
}
