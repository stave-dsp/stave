use core::slice;

/// A non-owning view over a multi-channel block of audio samples provided by
/// the host. Channels are stored as raw pointers (one per channel) which we
/// expose as `&mut [f32]` slices on demand.
///
/// Layout: `channel(c)[f]` is the sample at frame `f` of channel `c`.
pub struct AudioBuffer<'a> {
    channel_ptrs: &'a [*mut f32],
    frames: usize,
}

impl<'a> AudioBuffer<'a> {
    /// A zero-channel buffer — what an unconnected declared aux port
    /// looks like to the plugin (the `AuxIn` contract: fully
    /// functional with every aux absent).
    pub const fn empty() -> Self {
        Self {
            channel_ptrs: &[],
            frames: 0,
        }
    }

    /// A view of `channels`' first `frames` samples, through `ptrs` (the
    /// pointer array the view reads: one slot per channel, overwritten).
    /// The borrows prove what [`from_raw`](Self::from_raw) must be told:
    /// each channel is valid and no two alias.
    pub fn from_slices<C>(
        ptrs: &'a mut [*mut f32],
        channels: impl IntoIterator<Item = &'a mut C>,
        frames: usize,
    ) -> Self
    where
        C: AsMut<[f32]> + ?Sized + 'a,
    {
        let mut count = 0;
        for channel in channels {
            let channel = channel.as_mut();
            assert!(channel.len() >= frames, "a channel shorter than `frames`");
            ptrs[count] = channel.as_mut_ptr();
            count += 1;
        }
        assert_eq!(count, ptrs.len(), "one pointer slot per channel");
        // SAFETY: each pointer comes from a distinct `&'a mut` channel of at
        // least `frames` samples.
        unsafe { Self::from_raw(ptrs, frames) }
    }

    /// A null pointer is not a channel: [`channel`](Self::channel) and
    /// [`channel_ref`](Self::channel_ref) yield an empty slice for it (so
    /// a plugin indexing it panics — caught by the wrapper's panic guard —
    /// rather than reading through null). The wrappers never pass one:
    /// they substitute zeroed scratch for a host's null channels
    /// ([`ProcessorState::split_for_block`](crate::ProcessorState::split_for_block)).
    ///
    /// # Safety
    /// Each non-null pointer in `channel_ptrs` must be valid for `frames`
    /// f32 reads and writes for the lifetime `'a`, and channels must not
    /// alias.
    pub unsafe fn from_raw(channel_ptrs: &'a [*mut f32], frames: usize) -> Self {
        // Checkable half of the contract: channel ranges are pairwise
        // disjoint. (In-place hosts alias *across* the input/output
        // buffers — wrappers stage those through scratch before
        // constructing either buffer; see `stave_core::wrapper`.)
        #[cfg(debug_assertions)]
        for (i, &a) in channel_ptrs.iter().enumerate() {
            if a.is_null() {
                continue;
            }
            debug_assert!(
                !crate::wrapper::ranges_alias(&[a], &channel_ptrs[i + 1..], frames),
                "AudioBuffer channels alias (channel {i})"
            );
        }
        Self {
            channel_ptrs,
            frames,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.channel_ptrs.len()
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Returns a mutable slice for channel `idx`. Panics if out of bounds;
    /// empty for a null channel pointer (see [`from_raw`](Self::from_raw)).
    pub fn channel(&mut self, idx: usize) -> &mut [f32] {
        let ptr = self.channel_ptrs[idx];
        if ptr.is_null() {
            return &mut [];
        }
        // SAFETY: non-null, so from_raw's caller guarantees the pointer is
        // valid for `frames` samples and that channels do not alias. We
        // borrow through &mut self so the slice cannot outlive this borrow.
        unsafe { slice::from_raw_parts_mut(ptr, self.frames) }
    }

    /// Read-only view; useful when multiple non-overlapping borrows are
    /// needed without going through the channel index. Empty for a null
    /// channel pointer.
    pub fn channel_ref(&self, idx: usize) -> &[f32] {
        let ptr = self.channel_ptrs[idx];
        if ptr.is_null() {
            return &[];
        }
        // SAFETY: same invariants as `channel`, scoped to &self.
        unsafe { slice::from_raw_parts(ptr, self.frames) }
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
        let mut buffer = unsafe { AudioBuffer::from_raw(&ptrs, 4) };
        assert!(buffer.channel_ref(0).is_empty());
        assert!(buffer.channel(0).is_empty());
        assert_eq!(buffer.channel_ref(1), &[1.0; 4]);
    }
}
