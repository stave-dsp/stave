//! Audio-thread analysis tap: a planar-by-channel ring shared between
//! the audio callback (single producer) and the GUI (single consumer).
//! The audio thread writes each block; the GUI snapshots the newest
//! window for scope, spectrum and meter views.
//!
//! Lock-free and sound: samples are stored as f32 bits in relaxed
//! atomics (plain loads/stores on every target we ship), published by a
//! Release store of `write_idx`. A snapshot racing a write may mix old
//! and new samples at its leading edge — harmless for a visualiser, and
//! never undefined behavior.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Per-channel capacity in samples (a power of two: indices mask). 8192
/// samples ≈ 170 ms at 48 kHz — a comfortable FFT window and scope trace.
pub const CAPACITY: usize = 8192;
const MASK: usize = CAPACITY - 1;

/// Channels tapped. Most plugins are 1–2; the cap keeps the buffer small.
pub const MAX_CHANNELS: usize = 2;

pub struct AnalysisRing {
    channels: AtomicUsize,
    /// Total frames written (monotonic).
    write_idx: AtomicUsize,
    /// Planar: `storage[ch * CAPACITY + (frame & MASK)]`, f32 bits.
    storage: Box<[AtomicU32]>,
    /// Diagnostics for the debug panel.
    pub blocks_written: AtomicU64,
    pub max_block_len: AtomicU32,
}

impl AnalysisRing {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            channels: AtomicUsize::new(1),
            write_idx: AtomicUsize::new(0),
            storage: (0..MAX_CHANNELS * CAPACITY)
                .map(|_| AtomicU32::new(0))
                .collect(),
            blocks_written: AtomicU64::new(0),
            max_block_len: AtomicU32::new(0),
        })
    }

    pub fn set_channels(&self, n: usize) {
        self.channels
            .store(n.clamp(1, MAX_CHANNELS), Ordering::Relaxed);
    }

    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed)
    }

    pub fn frames_written(&self) -> usize {
        self.write_idx.load(Ordering::Acquire)
    }

    /// Audio thread: append one block, one equal-length slice per channel
    /// (channels past `MAX_CHANNELS` are ignored).
    pub fn write_block(&self, channels: &[&[f32]]) {
        let Some(frames) = channels.first().map(|c| c.len()).filter(|&n| n > 0) else {
            return;
        };
        let start = self.write_idx.load(Ordering::Relaxed);
        for (ch, src) in channels.iter().take(MAX_CHANNELS).enumerate() {
            let base = ch * CAPACITY;
            for (i, &s) in src.iter().enumerate() {
                self.storage[base + ((start + i) & MASK)].store(s.to_bits(), Ordering::Relaxed);
            }
        }
        self.write_idx.store(start + frames, Ordering::Release);
        self.blocks_written.fetch_add(1, Ordering::Relaxed);
        self.max_block_len
            .fetch_max(frames as u32, Ordering::Relaxed);
    }

    /// GUI thread: the newest `out.len()` (≤ CAPACITY) samples of
    /// `channel`, oldest first. Frames never written read as 0.
    pub fn snapshot(&self, channel: usize, out: &mut [f32]) {
        let n = out.len().min(CAPACITY);
        let base = channel.min(MAX_CHANNELS - 1) * CAPACITY;
        let end = self.write_idx.load(Ordering::Acquire);
        let start = end.wrapping_sub(n);
        for (i, o) in out[..n].iter_mut().enumerate() {
            let bits = self.storage[base + (start.wrapping_add(i) & MASK)].load(Ordering::Relaxed);
            *o = f32::from_bits(bits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(from: usize, n: usize) -> Vec<f32> {
        (from..from + n).map(|v| v as f32).collect()
    }

    #[test]
    fn snapshot_is_the_newest_window_oldest_first() {
        let r = AnalysisRing::new();
        r.write_block(&[&ramp(0, 100)]);
        let mut out = [0.0; 4];
        r.snapshot(0, &mut out);
        assert_eq!(out, [96.0, 97.0, 98.0, 99.0]);
    }

    #[test]
    fn wraps_around_capacity() {
        let r = AnalysisRing::new();
        r.write_block(&[&ramp(0, CAPACITY - 2)]);
        r.write_block(&[&ramp(CAPACITY - 2, 5)]);
        let mut out = [0.0; 5];
        r.snapshot(0, &mut out);
        assert_eq!(out, ramp(CAPACITY - 2, 5)[..]);
        assert_eq!(r.frames_written(), CAPACITY + 3);
        assert_eq!(
            r.max_block_len.load(Ordering::Relaxed),
            (CAPACITY - 2) as u32
        );
    }

    #[test]
    fn channels_are_planar_and_clamped() {
        let r = AnalysisRing::new();
        r.write_block(&[&[1.0, 1.0], &[2.0, 2.0], &[3.0, 3.0]]);
        let mut out = [0.0; 2];
        r.snapshot(1, &mut out);
        assert_eq!(out, [2.0, 2.0]);
        r.snapshot(7, &mut out);
        assert_eq!(out, [2.0, 2.0], "out-of-range channel reads the last");
        r.set_channels(9);
        assert_eq!(r.channels(), MAX_CHANNELS);
    }

    #[test]
    fn unwritten_frames_read_silent() {
        let mut out = [9.0; 3];
        AnalysisRing::new().snapshot(0, &mut out);
        assert_eq!(out, [0.0; 3]);
    }
}
