//! A block's audio: [`InputBuffer`] (read only) and [`OutputBuffer`]
//! (written), non-owning views over the host's channels.
//!
//! Layout: `channel(c)[f]` is the sample at frame `f` of channel `c`.
//! Channels never alias one another, and an input never aliases an
//! output (the wrappers stage in-place hosts through scratch first), so
//! [`OutputBuffer::channels_mut`] can lend every channel mutably at once.

use core::marker::PhantomData;
use core::slice;

/// Read-only channels: the main input, or an aux (sidechain) input.
pub struct InputBuffer<'a> {
    ptrs: &'a [*mut f32],
    frames: usize,
    _samples: PhantomData<&'a [f32]>,
}

/// Writable channels: the main output.
pub struct OutputBuffer<'a> {
    ptrs: &'a [*mut f32],
    frames: usize,
    _samples: PhantomData<&'a mut [f32]>,
}

/// Fill `ptrs` with one pointer per channel, checking each holds
/// `frames` samples.
fn point_at<'a, C>(
    ptrs: &mut [*mut f32],
    channels: impl IntoIterator<Item = &'a mut C>,
    frames: usize,
) where
    C: AsMut<[f32]> + ?Sized + 'a,
{
    let mut count = 0;
    for channel in channels {
        let channel = channel.as_mut();
        assert!(channel.len() >= frames, "a channel shorter than `frames`");
        assert!(count < ptrs.len(), "one pointer slot per channel");
        ptrs[count] = channel.as_mut_ptr();
        count += 1;
    }
    assert_eq!(count, ptrs.len(), "one pointer slot per channel");
}

/// The checkable half of the raw contract: channels are pairwise
/// disjoint.
fn debug_check_disjoint(ptrs: &[*mut f32], frames: usize) {
    #[cfg(debug_assertions)]
    for (i, &a) in ptrs.iter().enumerate() {
        if a.is_null() {
            continue;
        }
        debug_assert!(
            !crate::runtime::ranges_alias(&[a], &ptrs[i + 1..], frames),
            "buffer channels alias (channel {i})"
        );
    }
    let _ = (ptrs, frames);
}

/// `ptrs[idx]` as a slice of `frames`; empty for a null pointer.
///
/// # Safety
/// A non-null `ptrs[idx]` is valid for `frames` reads for `'s`.
unsafe fn channel_at<'s>(ptrs: &[*mut f32], idx: usize, frames: usize) -> &'s [f32] {
    let ptr = ptrs[idx];
    if ptr.is_null() {
        return &[];
    }
    // SAFETY: this function's contract.
    unsafe { slice::from_raw_parts(ptr, frames) }
}

impl<'a> InputBuffer<'a> {
    /// No channels: what an unconnected aux bus, or an instrument's main
    /// input, looks like.
    pub const fn empty() -> Self {
        Self {
            ptrs: &[],
            frames: 0,
            _samples: PhantomData,
        }
    }

    /// A view of `channels`' first `frames` samples, through `ptrs` (one
    /// slot per channel, overwritten). The borrows prove what
    /// [`from_raw`](Self::from_raw) is told (input channels may alias
    /// each other: they are only read).
    pub fn from_slices<C>(
        ptrs: &'a mut [*mut f32],
        channels: impl IntoIterator<Item = &'a C>,
        frames: usize,
    ) -> Self
    where
        C: AsRef<[f32]> + ?Sized + 'a,
    {
        let mut count = 0;
        for channel in channels {
            let channel = channel.as_ref();
            assert!(channel.len() >= frames, "a channel shorter than `frames`");
            assert!(count < ptrs.len(), "one pointer slot per channel");
            // Never written through: an input is read only.
            ptrs[count] = channel.as_ptr().cast_mut();
            count += 1;
        }
        assert_eq!(count, ptrs.len(), "one pointer slot per channel");
        // SAFETY: each pointer comes from a channel borrowed for `'a` of
        // at least `frames` samples, and an `InputBuffer` only reads.
        unsafe { Self::from_raw(ptrs, frames) }
    }

    /// A null pointer is not a channel: [`channel`](Self::channel) yields
    /// an empty slice for it. The wrappers never pass one (they
    /// substitute zeroed scratch).
    ///
    /// # Safety
    /// Each non-null pointer in `ptrs` is valid for `frames` reads for
    /// `'a`, and nothing writes through it meanwhile.
    pub unsafe fn from_raw(ptrs: &'a [*mut f32], frames: usize) -> Self {
        Self {
            ptrs,
            frames,
            _samples: PhantomData,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.ptrs.len()
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Channel `idx`. Panics if out of range.
    pub fn channel(&self, idx: usize) -> &[f32] {
        // SAFETY: `from_raw`'s contract, for the borrow of `self`.
        unsafe { channel_at(self.ptrs, idx, self.frames) }
    }

    /// Every channel, in order.
    pub fn channels(&self) -> impl Iterator<Item = &[f32]> + '_ {
        (0..self.ptrs.len()).map(|c| self.channel(c))
    }
}

impl<'a> OutputBuffer<'a> {
    /// No channels.
    pub const fn empty() -> Self {
        Self {
            ptrs: &[],
            frames: 0,
            _samples: PhantomData,
        }
    }

    /// A view of `channels`' first `frames` samples, through `ptrs` (one
    /// slot per channel, overwritten).
    pub fn from_slices<C>(
        ptrs: &'a mut [*mut f32],
        channels: impl IntoIterator<Item = &'a mut C>,
        frames: usize,
    ) -> Self
    where
        C: AsMut<[f32]> + ?Sized + 'a,
    {
        point_at(ptrs, channels, frames);
        // SAFETY: each pointer comes from a distinct `&'a mut` channel of
        // at least `frames` samples.
        unsafe { Self::from_raw(ptrs, frames) }
    }

    /// As [`InputBuffer::from_raw`], for writing.
    ///
    /// # Safety
    /// Each non-null pointer in `ptrs` is valid for `frames` reads and
    /// writes for `'a`, no two channels overlap, and nothing else reads
    /// or writes them meanwhile.
    pub unsafe fn from_raw(ptrs: &'a [*mut f32], frames: usize) -> Self {
        debug_check_disjoint(ptrs, frames);
        Self {
            ptrs,
            frames,
            _samples: PhantomData,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.ptrs.len()
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Channel `idx`, read only. Panics if out of range.
    pub fn channel(&self, idx: usize) -> &[f32] {
        // SAFETY: `from_raw`'s contract, for the borrow of `self`.
        unsafe { channel_at(self.ptrs, idx, self.frames) }
    }

    /// Channel `idx`, to write. Panics if out of range; empty for a null
    /// channel.
    pub fn channel_mut(&mut self, idx: usize) -> &mut [f32] {
        let ptr = self.ptrs[idx];
        if ptr.is_null() {
            return &mut [];
        }
        // SAFETY: `from_raw`'s contract; the borrow of `self` is unique.
        unsafe { slice::from_raw_parts_mut(ptr, self.frames) }
    }

    /// Every channel, mutably, at once (`let [l, r] = …` style code
    /// takes two from the iterator).
    pub fn channels_mut(&mut self) -> impl Iterator<Item = &mut [f32]> + '_ {
        let frames = self.frames;
        self.ptrs.iter().map(move |&ptr| {
            if ptr.is_null() {
                return &mut [][..];
            }
            // SAFETY: `from_raw`'s contract: channels never overlap, so
            // the slices this yields (one per channel, each once) are
            // disjoint, and the iterator holds the unique borrow of
            // `self` while any of them lives.
            unsafe { slice::from_raw_parts_mut(ptr, frames) }
        })
    }

    /// Zero every channel.
    pub fn silence(&mut self) {
        for c in self.channels_mut() {
            c.fill(0.0);
        }
    }

    /// The raw channel list, for the framework's lane walk.
    pub(crate) fn ptrs(&self) -> &'a [*mut f32] {
        self.ptrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A null channel pointer reads as an empty channel, never through
    /// null (Miri checks the second half).
    #[test]
    fn a_null_channel_is_empty_not_a_null_slice() {
        let mut data = [1.0f32; 4];
        let ptrs = [core::ptr::null_mut(), data.as_mut_ptr()];
        // SAFETY: the non-null pointer is `data`, valid for 4 frames.
        let mut out = unsafe { OutputBuffer::from_raw(&ptrs, 4) };
        assert!(out.channel(0).is_empty());
        assert!(out.channel_mut(0).is_empty());
        assert_eq!(out.channel(1), &[1.0; 4]);
    }

    /// Two output channels written at once through safe code.
    #[test]
    fn every_output_channel_borrows_mutably_at_once() {
        let mut l = [0.0f32; 3];
        let mut r = [0.0f32; 3];
        let mut ptrs = [core::ptr::null_mut(); 2];
        let mut out = OutputBuffer::from_slices(&mut ptrs, [&mut l[..], &mut r[..]], 3);
        let mut chans = out.channels_mut();
        let (a, b) = (chans.next().unwrap(), chans.next().unwrap());
        a[0] = 1.0;
        b[0] = 2.0;
        a[2] = b[0] + a[0];
        drop(chans);
        assert_eq!(out.channel(0), &[1.0, 0.0, 3.0]);
        assert_eq!(out.channel(1), &[2.0, 0.0, 0.0]);
        let input_data = [0.5f32; 3];
        let mut in_ptrs = [core::ptr::null_mut(); 1];
        let input = InputBuffer::from_slices(&mut in_ptrs, [&input_data[..]], 3);
        assert_eq!(input.channels().map(|c| c[1]).sum::<f32>(), 0.5);
    }
}
