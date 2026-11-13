//! Realtime safety of the delays, reverbs and dynamics: after `prepare`,
//! `tick`, setters that are documented as cheap, and `reset` never
//! allocate. A counting global allocator, armed per thread, watches.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use stave_dsp::effects::dynamics::{
    Compressor, GainComputer, Limiter, Peak, Peaks, ProgramDependent, Rms, Sidechain, Topology,
};
use stave_dsp::effects::reverb::{Dattorro, Fdn, Mixing, Schroeder};
use stave_dsp::primitives::time::delay::Delay;
use stave_dsp::primitives::time::interpolators::{Lagrange3, Lagrange5, Sinc, Thiran};
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
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` via `alloc`/`realloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `realloc`, passed through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn allocations_in(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

/// Prepare `stage`, then run, reset and run again with counting armed.
fn no_allocation(name: &str, stage: &mut dyn Stage) {
    stage.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..4_096 {
            stage.tick(((i * 37) % 101) as f32 / 25.0 - 2.0);
        }
        stage.reset();
        for _ in 0..256 {
            stage.tick(1.5);
        }
    });
    assert_eq!(n, 0, "{name} allocated {n} times after prepare");
}

#[test]
#[cfg_attr(miri, ignore)]
fn effects_never_allocate_after_prepare() {
    no_allocation("Delay<Lagrange3>", &mut Delay::<Lagrange3>::new(20.0, 7.3));
    no_allocation("Delay<Lagrange5>", &mut Delay::<Lagrange5>::new(20.0, 7.3));
    no_allocation("Delay<Thiran>", &mut Delay::<Thiran>::new(20.0, 7.3));
    no_allocation("Delay<Sinc<16>>", &mut Delay::<Sinc<16>>::new(20.0, 7.3));
    no_allocation("Fdn", &mut Fdn::new(Mixing::Hadamard));
    no_allocation("Dattorro", &mut Dattorro::new(50.0));
    no_allocation("Schroeder", &mut Schroeder::new());
    no_allocation("Schroeder::moorer", &mut Schroeder::moorer());
    no_allocation("Peak", &mut Peak::new(1.0, 50.0));
    no_allocation("Rms", &mut Rms::new(10.0));
    no_allocation("ProgramDependent", &mut ProgramDependent::new(1.0, 50.0, 800.0, 400.0));
    no_allocation("Sidechain", &mut Sidechain::new(Some(80.0), Some(9_000.0)));
    let curve = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
    no_allocation("Compressor", &mut Compressor::new(curve, Topology::FeedForward));
    no_allocation("Compressor (feedback)", &mut Compressor::new(curve, Topology::Feedback));
    no_allocation("Limiter (true peak)", &mut Limiter::new(Peaks::True, -1.0));
    no_allocation("Limiter (sample peak)", &mut Limiter::new(Peaks::Sample, -1.0));

    // Setters a modulation path calls per sample.
    let mut voice = Delay::<Lagrange3>::new(20.0, 10.0);
    voice.prepare(48_000.0);
    let mut room = Fdn::new(Mixing::Householder);
    room.prepare(48_000.0);
    let n = allocations_in(|| {
        for i in 0..1_000 {
            voice.set_ms(10.0 + (i as f32 * 0.01).sin());
            voice.tick(0.1);
            room.set_t60_s(1.0 + i as f32 * 1e-3);
            room.tick_stereo(0.1);
        }
    });
    assert_eq!(n, 0, "setters allocated {n} times");
}
