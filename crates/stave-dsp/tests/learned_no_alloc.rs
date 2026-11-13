//! The learned networks are realtime after loading: `prepare`, `reset`,
//! `tick` and `process` never touch the allocator. A counting global
//! allocator watches; this file holds one test so nothing else
//! allocates concurrently.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use stave_dsp::learned::{nam, Gru};
use stave_dsp::Stage;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to `System` unchanged; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract for `alloc` is passed through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract for `realloc` is passed through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn assert_no_alloc(what: &str, stage: &mut impl Stage) {
    let mut block = [0.0f32; 256];
    for (i, x) in block.iter_mut().enumerate() {
        *x = (i as f32 * 0.05).sin() * 0.7;
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    stage.prepare(48_000.0);
    stage.reset();
    for &x in &block {
        std::hint::black_box(stage.tick(x));
    }
    stage.process(&mut block);
    stage.prepare(44_100.0); // a refused rate: still no allocation
    std::hint::black_box(stage.tick(0.1));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(after, before, "{what} allocated on the audio path");
}

#[test]
#[cfg_attr(miri, ignore)]
fn networks_process_without_allocating() {
    for (name, text) in [
        (
            "wavenet_standard",
            include_str!("reference/nam/wavenet_standard.nam"),
        ),
        (
            "wavenet_packed",
            include_str!("reference/nam/wavenet_packed.nam"),
        ),
        (
            "wavenet_mixed",
            include_str!("reference/nam/wavenet_mixed.nam"),
        ),
        ("lstm", include_str!("reference/nam/lstm.nam")),
    ] {
        let mut net = nam::parse(text).unwrap().network;
        assert_no_alloc(name, &mut net);
    }
    let mut gru = Gru::parse(include_str!("reference/nam/gru.json")).unwrap();
    assert_no_alloc("gru", &mut gru);
}
