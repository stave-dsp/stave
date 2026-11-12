//! Kani proof harnesses (bounded model checking) for the wrapper's
//! unsafe and index-heavy core: `cargo kani -p stave-core`
//! (scripts/kani.sh). Each harness states, over EVERY input within its
//! bounds, what the code guarantees; Kani also checks every pointer
//! dereference, slice index and arithmetic operation on the way for
//! undefined behaviour and panics.

use super::*;
use crate::info::{Info, Layout};
use crate::params::{NoParams, ParamId, Params};
use crate::process::{Block, ParamChange, ParamChangeKind};
use crate::{InputBuffer, OutputBuffer};
use stave_telemetry::Telemetry;
use std::sync::Arc;

struct Stereo;
impl Erased for Stereo {
    const INFO: Info = Info {
        id: "proof.stereo",
        name: "Stereo",
        vendor: "proofs",
        ..Info::EFFECT
    };
    const LAYOUTS: &'static [Layout] = &[Layout::STEREO];
    fn new(_telemetry: &Telemetry) -> Self {
        Stereo
    }
    fn params(&self) -> Arc<dyn Params> {
        Arc::new(NoParams)
    }
    fn process(&mut self, _block: &mut Block<'_>) {}
}

/// Frames per host channel in the harnesses' host buffer.
const HOST: usize = 3;
/// Host channels to point into (in-place hosts point inputs at outputs).
const CHANNELS: usize = 3;

/// `ranges_alias` is exactly interval overlap: any two pointers into one
/// buffer, any frame count, nulls skipped.
#[kani::proof]
#[kani::unwind(4)]
fn ranges_alias_is_interval_overlap() {
    let mut buf = [0.0f32; 8];
    let base = buf.as_mut_ptr();
    let (oa, ob): (usize, usize) = (kani::any(), kani::any());
    let frames: usize = kani::any();
    kani::assume(oa < 8 && ob < 8 && frames <= 8 - oa.max(ob));
    let (null_a, null_b): (bool, bool) = (kani::any(), kani::any());
    // SAFETY: both offsets are inside `buf`.
    let pa = if null_a { core::ptr::null_mut() } else { unsafe { base.add(oa) } };
    // SAFETY: as above.
    let pb = if null_b { core::ptr::null_mut() } else { unsafe { base.add(ob) } };
    let overlap = frames > 0 && !null_a && !null_b && oa < ob + frames && ob < oa + frames;
    assert_eq!(ranges_alias(&[pa], &[pb], frames), overlap);
    assert_eq!(ranges_alias(&[pb], &[pa], frames), overlap);
}

/// `sort_param_changes` is a stable sort by frame: the result is a
/// permutation, ordered by frame, and equal frames keep arrival order.
#[kani::proof]
#[kani::unwind(6)]
fn param_changes_sort_stably_by_frame() {
    const N: usize = 5;
    let mut changes: [ParamChange; N] = core::array::from_fn(|i| ParamChange {
        id: ParamId(i as u32), // the arrival order
        frame: kani::any_where(|f: &usize| *f < 4),
        value: 0.0,
        kind: ParamChangeKind::Value,
    });
    sort_param_changes(&mut changes);
    let mut seen = 0u32;
    for i in 0..N {
        seen |= 1 << changes[i].id.0;
        if i > 0 {
            let (a, b) = (&changes[i - 1], &changes[i]);
            assert!(a.frame <= b.frame);
            assert!(a.frame < b.frame || a.id.0 < b.id.0, "stable");
        }
    }
    assert_eq!(seen, (1 << N) - 1, "a permutation");
}

/// A host channel list: up to `max` (≤ 2) channels, each null or one of
/// the host buffer's channels.
fn host_list(base: *mut f32, out: &mut [*mut f32; 2], max: usize) -> usize {
    let n: usize = kani::any_where(|n: &usize| *n <= max);
    for slot in out.iter_mut().take(n) {
        let c: usize = kani::any_where(|c: &usize| *c < CHANNELS);
        *slot = if kani::any() {
            core::ptr::null_mut()
        } else {
            // SAFETY: channel `c` lies inside the host buffer.
            unsafe { base.add(c * HOST) }
        };
    }
    n
}

/// The wrapper's buffer split, for every host channel list the format
/// allows (nulls, in-place aliasing of inputs onto outputs, more
/// channels or frames than prepared): `split_for_block` hands back
/// lists with no nulls, inputs that never alias outputs, outputs that
/// never alias each other, and no more frames than the host gave (than
/// the scratch holds, when scratch was needed) — and every frame of
/// every list is then read (inputs) and written (outputs), so Kani
/// checks each access is in bounds of the host's channels or the
/// scratch.
#[kani::proof]
#[kani::unwind(5)]
fn split_for_block_hands_the_plugin_sound_buffers() {
    let mut host = [0.0f32; CHANNELS * HOST];
    let base = host.as_mut_ptr();
    let mut input = [core::ptr::null_mut(); 2];
    let mut output = [core::ptr::null_mut(); 2];
    // One input is enough to alias any output (in place) or be null.
    let n_in = host_list(base, &mut input, 1);
    let n_out = host_list(base, &mut output, 2);
    let (input, output) = (&input[..n_in], &output[..n_out]);
    // The host's contract: no two outputs overlap.
    if n_out == 2 && !output[0].is_null() {
        kani::assume(output[0] != output[1]);
    }
    let frames: usize = kani::any_where(|f: &usize| *f <= HOST);
    // Prepared for 2 frames: blocks of 0–3 frames fit or are cut.
    let max_frames = 2;

    let mut state = ProcessorState::new(Stereo);
    // `prepare` without the plugin's own work: the scratch is what is
    // under proof (in-channel count 0: the declared layout sizes it).
    state.prepare_scratch(0, max_frames);
    state.prepared = true;
    // SAFETY: every non-null pointer is a host channel of HOST >= frames
    // samples, and the outputs are distinct channels (assumed above).
    let Some(borrows) = (unsafe { state.split_for_block(input, output, &[], frames) }) else {
        panic!("a prepared state always splits");
    };
    let io = borrows.io;
    assert!(io.frames <= frames);
    assert!(io.truncated || io.frames == frames);
    assert!(io.input.len() <= n_in && io.output.len() <= n_out);
    assert!(io.input.iter().chain(io.output).all(|p| !p.is_null()));
    assert!(!ranges_alias(io.input, io.output, io.frames));
    if io.output.len() == 2 {
        assert!(!ranges_alias(&io.output[..1], &io.output[1..], io.frames));
    }
    for (&i, &o) in io.input.iter().zip(io.output) {
        for f in 0..io.frames {
            // SAFETY: just proved: no nulls, readable/writable for
            // `io.frames`, and the two never alias.
            unsafe { *o.add(f) = *i.add(f) };
        }
    }
}

/// The plugin's view of a block: for any channel counts (an
/// instrument's zero inputs included) and frame count, `Lanes` reads
/// and writes exactly the frames of the lanes it reports active, zeroes
/// the outputs past them, and never indexes outside a channel.
#[kani::proof]
#[kani::unwind(5)]
fn lanes_stay_inside_the_block() {
    let ins = [[0.5f32; HOST]; 3];
    let mut outs = [[1.0f32; HOST]; 3];
    let n_in: usize = kani::any_where(|n: &usize| *n <= 3);
    let n_out: usize = kani::any_where(|n: &usize| *n <= 3);
    let frames: usize = kani::any_where(|f: &usize| *f <= 2);
    let mut in_ptrs = [core::ptr::null_mut(); 3];
    let mut out_ptrs = [core::ptr::null_mut(); 3];
    let input = InputBuffer::from_slices(&mut in_ptrs[..n_in], ins.iter().take(n_in), frames);
    let output = OutputBuffer::from_slices(&mut out_ptrs[..n_out], outs.iter_mut().take(n_out), frames);
    let mut block = Block::new(48_000.0, frames, input, output);
    let lanes = block.lanes::<2>();
    let active = lanes.active();
    assert!(active <= 2 && active <= n_out);
    assert!(n_in == 0 || active <= n_in);
    lanes.run(|frame| {
        for lane in 0..frame.lanes() {
            let x = frame.input(lane);
            frame.write(lane, x);
        }
    });
    drop(block);
    for c in active..n_out {
        assert!(outs[c][..frames].iter().all(|&s| s == 0.0), "undriven outputs are zeroed");
    }
}

/// Before `prepare` (and after `release`) there is no way to the
/// plugin's audio-thread borrow, for any host lists.
#[kani::proof]
#[kani::unwind(4)]
fn an_unprepared_state_never_splits() {
    let mut host = [0.0f32; CHANNELS * HOST];
    let base = host.as_mut_ptr();
    let mut output = [core::ptr::null_mut(); 2];
    let n_out = host_list(base, &mut output, 2);
    let frames: usize = kani::any_where(|f: &usize| *f <= HOST);
    let mut state = ProcessorState::new(Stereo);
    if kani::any() {
        state.prepare_scratch(0, 2);
        state.prepared = true;
        state.release();
    }
    // SAFETY: host channels of HOST >= frames samples.
    assert!(unsafe { state.split_for_block(&[], &output[..n_out], &[], frames) }.is_none());
}
