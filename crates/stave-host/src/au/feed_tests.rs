//! The input callback against what a real unit hands it (audit U3/U4):
//! a buffer list smaller than [`AudioBufferList`], a buffer smaller than
//! the frame count, and a feed with no channels. None may write out of
//! bounds or panic through `extern "C"`.

// Every `unsafe` block here plays the Audio Unit: it calls the callback
// with a list and buffers the test owns.
#![allow(clippy::undocumented_unsafe_blocks)]

use super::*;

const GUARD: f32 = 777.0;

/// The C layout a unit allocates for one buffer: the count plus a
/// one-element array, smaller than this crate's `AudioBufferList`.
#[repr(C)]
struct OneBuffer {
    count: u32,
    buffer: AuBuffer,
}

fn feed_of(channels: Vec<Vec<f32>>, frames: usize) -> Feed {
    Feed { channels, frames }
}

unsafe fn pull(feed: &mut Feed, frames: u32, list: *mut AudioBufferList) -> OSStatus {
    unsafe {
        super::feed(
            (feed as *mut Feed).cast(),
            ptr::null_mut(),
            ptr::null(),
            0,
            frames,
            list,
        )
    }
}

#[test]
fn a_one_buffer_list_is_filled_without_touching_past_it() {
    let mut f = feed_of(vec![vec![0.5; 8], vec![-0.5; 8]], 8);
    let mut out = [GUARD; 8];
    let mut list = OneBuffer {
        count: 1,
        buffer: AuBuffer {
            mNumberChannels: 1,
            mDataByteSize: 32,
            mData: out.as_mut_ptr().cast(),
        },
    };
    let r = unsafe { pull(&mut f, 8, (&mut list as *mut OneBuffer).cast()) };
    assert_eq!(r, 0);
    assert_eq!(out, [0.5; 8]);
    assert_eq!(list.buffer.mDataByteSize, 32);
}

#[test]
fn a_buffer_smaller_than_the_frame_count_is_never_overrun() {
    let mut f = feed_of(vec![vec![0.25; 8]], 8);
    // The unit's buffer is the first 4 floats of `backing`: a write
    // past them lands in the guard.
    let mut backing = [GUARD; 16];
    let mut list = OneBuffer {
        count: 1,
        buffer: AuBuffer {
            mNumberChannels: 1,
            mDataByteSize: 16,
            mData: backing.as_mut_ptr().cast(),
        },
    };
    let r = unsafe { pull(&mut f, 8, (&mut list as *mut OneBuffer).cast()) };
    assert_eq!(r, 0);
    assert_eq!(backing[..4], [0.25; 4]);
    assert!(backing[4..].iter().all(|&s| s == GUARD), "wrote past the unit's buffer: {backing:?}");
    assert_eq!(list.buffer.mDataByteSize, 16, "reports what it wrote");
}

#[test]
fn a_feed_with_no_channels_is_an_error_not_an_abort() {
    let mut f = feed_of(Vec::new(), 8);
    let mut out = [GUARD; 8];
    let mut list = OneBuffer {
        count: 1,
        buffer: AuBuffer {
            mNumberChannels: 1,
            mDataByteSize: 32,
            mData: out.as_mut_ptr().cast(),
        },
    };
    let r = unsafe { pull(&mut f, 8, (&mut list as *mut OneBuffer).cast()) };
    assert_ne!(r, 0);
    assert_eq!(out, [GUARD; 8]);
}
