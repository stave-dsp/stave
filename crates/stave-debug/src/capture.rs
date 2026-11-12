//! The capture frame: the latest block of output per channel.

use core::sync::atomic::{AtomicI32, AtomicU16, AtomicU32, AtomicU64, Ordering};

use crate::{DebugBus, CAPTURE_CHANNELS, CAPTURE_LEN};

/// Snapshot of plugin output. Written by the wrapper each block. The GUI reads
/// it with tearing accepted (last writer wins; not strictly atomic across
/// channels — fine for a visualiser).
#[repr(C)]
pub struct CaptureFrame {
    pub channels: AtomicU16,
    pub frames: AtomicU32,
    pub block: AtomicU64,
    /// Sample storage: planar by channel, padded to CAPTURE_LEN per channel.
    /// f32 bit-patterns stored as AtomicI32 so we can update from the audio
    /// thread without UB; tearing across samples is acceptable.
    pub samples: [AtomicI32; CAPTURE_CHANNELS * CAPTURE_LEN],
}

impl CaptureFrame {
    pub(crate) const fn zeroed() -> Self {
        // `AtomicI32` isn't `Copy`, so `[AtomicI32::new(0); N]` won't
        // compile; an array-repeat of a named `const` item is allowed
        // (each element is a fresh instance). All-zero bits == 0.0_f32.
        #[allow(clippy::declare_interior_mutable_const)]
        const ZERO: AtomicI32 = AtomicI32::new(0);
        Self {
            channels: AtomicU16::new(0),
            frames: AtomicU32::new(0),
            block: AtomicU64::new(0),
            samples: [ZERO; CAPTURE_CHANNELS * CAPTURE_LEN],
        }
    }
}

impl DebugBus {
    /// Wrapper entry: copy a block of per-channel samples into the capture
    /// frame. Called once per `process()` from the format wrapper, after the
    /// plugin has filled its output.
    pub fn write_capture(&self, channels: &[&[f32]], block: u64) {
        let n_ch = channels.len().min(CAPTURE_CHANNELS);
        if n_ch == 0 {
            return;
        }
        let n_frames = channels[0].len().min(CAPTURE_LEN);
        self.capture.channels.store(n_ch as u16, Ordering::Relaxed);
        self.capture
            .frames
            .store(n_frames as u32, Ordering::Relaxed);
        for ci in 0..n_ch {
            let base = ci * CAPTURE_LEN;
            for fi in 0..n_frames {
                self.capture.samples[base + fi]
                    .store(channels[ci][fi].to_bits() as i32, Ordering::Relaxed);
            }
        }
        self.capture.block.store(block, Ordering::Release);
    }

    /// GUI reader: copy out a channel of the capture frame as f32.
    pub fn read_capture_channel(&self, ch: usize, out: &mut [f32]) -> (u32, u64) {
        if ch >= CAPTURE_CHANNELS {
            return (0, 0);
        }
        let n = out.len().min(CAPTURE_LEN);
        let avail = self.capture.frames.load(Ordering::Acquire) as usize;
        let take = n.min(avail);
        let base = ch * CAPTURE_LEN;
        for i in 0..take {
            out[i] = f32::from_bits(self.capture.samples[base + i].load(Ordering::Relaxed) as u32);
        }
        (take as u32, self.capture.block.load(Ordering::Acquire))
    }
}
