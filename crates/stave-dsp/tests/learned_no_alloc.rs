//! The learned networks are realtime after loading: `prepare`, `reset`,
//! `tick` and `process` never touch the allocator. A counting global
//! allocator, armed per thread, watches: only the measuring thread's
//! allocations count. (A process-wide counter also saw the test
//! harness's main thread, which allocates when it first blocks waiting
//! for the test thread — a race that failed this test on a loaded
//! runner.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use stave_dsp::learned::{nam, Gru, RatePolicy};
use stave_dsp::Stage;

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            COUNT.with(|c| c.set(c.get() + 1));
        }
    });
}

// SAFETY: every method forwards to `System` with the caller's arguments,
// unchanged; counting touches only thread-local cells.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc`, passed through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc_zeroed`, passed through.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` via the methods above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `realloc`, passed through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations_in(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

/// The whole audio-path life of a freshly loaded network, its first
/// `prepare` and first `tick` included, counted.
fn assert_no_alloc(what: &str, stage: &mut impl Stage) {
    let mut block = [0.0f32; 256];
    for (i, x) in block.iter_mut().enumerate() {
        *x = (i as f32 * 0.05).sin() * 0.7;
    }
    let n = allocations_in(|| {
        stage.prepare(48_000.0);
        stage.reset();
        for &x in &block {
            std::hint::black_box(stage.tick(x));
        }
        stage.process(&mut block);
        stage.prepare(44_100.0); // a refused rate: still no allocation
        std::hint::black_box(stage.tick(0.1));
    });
    assert_eq!(n, 0, "{what} allocated {n} times on the audio path");
}

/// The counter sees this thread's allocations: a check that cannot fail
/// is not a check.
#[test]
#[cfg_attr(miri, ignore)]
fn the_counter_counts() {
    let n = allocations_in(|| drop(std::hint::black_box(vec![1u8, 2, 3])));
    assert_eq!(n, 1);
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

/// `RatePolicy::Interpolated` allocates its delay line when chosen;
/// after that, preparing at other rates and running is allocation-free.
#[test]
#[cfg_attr(miri, ignore)]
fn interpolated_recurrent_networks_process_without_allocating() {
    let mut lstm = nam::parse(include_str!("reference/nam/lstm.nam"))
        .unwrap()
        .network;
    lstm.set_rate_policy(RatePolicy::Interpolated);
    let mut gru = Gru::parse(include_str!("reference/nam/gru.json")).unwrap();
    gru.set_rate_policy(RatePolicy::Interpolated);
    for (name, stage) in [
        ("lstm", &mut lstm as &mut dyn Stage),
        ("gru", &mut gru as &mut dyn Stage),
    ] {
        let n = allocations_in(|| {
            for rate in [44_100.0, 96_000.0, 192_000.0] {
                stage.prepare(rate);
                for i in 0..256 {
                    std::hint::black_box(stage.tick((i as f32 * 0.05).sin() * 0.7));
                }
            }
        });
        assert_eq!(n, 0, "{name} allocated {n} times on the audio path");
    }
}

/// Every size of a slimmable file is built at load: choosing one only
/// clones, and the clone runs allocation-free.
#[test]
#[cfg_attr(miri, ignore)]
fn slimmed_networks_process_without_allocating() {
    let file = nam::parse(include_str!("reference/nam/slimmable_gated.nam")).unwrap();
    for i in 0..file.sizes().len() {
        let mut net = file.network_at(i);
        assert_no_alloc("slimmable_gated", &mut net);
    }
}

/// A `SlimmableContainer` switches size on the audio thread: every
/// submodel was built and prepared up front, and the switch resets the
/// chosen one without touching the allocator (NAM's own switch is
/// "thread-safe, not real-time safe").
#[test]
#[cfg_attr(miri, ignore)]
fn container_switches_without_allocating() {
    let mut net = nam::parse(include_str!("reference/nam/container.nam"))
        .unwrap()
        .network;
    assert_no_alloc("container", &mut net);
    net.prepare(48_000.0);
    let mut block = [0.0f32; 64];
    let n = allocations_in(|| {
        for (k, value) in [0.0, 0.5, 1.0, 0.3, 0.7, 0.29, 0.2, 2.0, -1.0]
            .into_iter()
            .enumerate()
        {
            net.set_size(value);
            for (i, x) in block.iter_mut().enumerate() {
                *x = ((i + 64 * k) as f32 * 0.05).sin() * 0.7;
            }
            net.process(&mut block);
            std::hint::black_box(net.tick(0.1));
        }
    });
    assert_eq!(n, 0, "the container's switch allocated {n} times");
}

/// Block models (DSP.md M3) are realtime once built: preparing at any
/// rate (the sections re-derived in place), resetting to rest, ADAA on
/// or off, and running never allocate.
#[test]
#[cfg_attr(miri, ignore)]
fn block_models_process_without_allocating() {
    use stave_dsp::learned::block::{Linear, Nonlinearity, ParallelHammerstein, Section, WienerHammerstein};
    let section = Section {
        b: [0.2, 0.4, 0.2],
        a: [-0.6, 0.2],
    };
    let mut wh = WienerHammerstein::new(
        48_000.0,
        Linear::Rational(vec![section; 2]),
        Nonlinearity::Spline {
            knots: vec![-2.0, -0.5, 0.0, 0.4, 2.0],
            values: vec![-0.9, -0.4, 0.05, 0.4, 0.95],
        },
        Linear::Fir(vec![0.5, 0.3, 0.2]),
    );
    wh.set_rate_policy(RatePolicy::Any);
    assert_no_alloc("wiener_hammerstein", &mut wh);
    wh.set_antialiasing(true);
    assert_no_alloc("wiener_hammerstein with ADAA", &mut wh);
    let mut ph = ParallelHammerstein::new(48_000.0, vec![Linear::Rational(vec![section]), Linear::Fir(vec![0.1; 16])])
        .with_limit(1.0);
    assert_no_alloc("parallel_hammerstein", &mut ph);
}
