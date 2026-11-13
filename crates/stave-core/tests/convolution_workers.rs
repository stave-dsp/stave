//! The convolver's worker threads (DSP.md F3b) against the inline
//! reference: bit-identical output when every deadline is met (waiting
//! offline, or a settled realtime run), a forced slow worker's miss
//! played as exactly one block of silence and reported, stress under
//! CPU load where every difference is explained by a reported miss, and
//! the audio thread's cost per block against the single-threaded
//! schedule's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use stave_core::workers::{Deadline, Workers};
use stave_dsp::primitives::spectral::{Head, Inline};
use stave_dsp::{Convolver, ImpulseResponse, Stage};

fn noise(seed: u32, n: usize) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// A decaying-noise response of `len` taps.
fn hall(len: usize) -> Vec<f32> {
    noise(7, len)
        .iter()
        .enumerate()
        .map(|(n, x)| x * (-(n as f32) / len as f32 * 6.0).exp())
        .collect()
}

fn run<W: stave_dsp::primitives::spectral::Worker>(conv: &mut Convolver<W>, x: &[f32]) -> Vec<f32> {
    x.iter().map(|&s| conv.tick(s)).collect()
}

/// Offline, the threaded worker waits for every block: its output is the
/// inline reference's, bit for bit — mono and true stereo, one to three
/// deferred sizes.
#[test]
fn offline_workers_match_inline_bit_for_bit() {
    let x = noise(3, 60_000);
    for (taps, largest, from) in [
        (20_000usize, 1_024usize, 256usize),
        (40_000, 4_096, 512),
        (8_000, 512, 128),
    ] {
        let h = hall(taps);
        let ir = ImpulseResponse::mono(&h);
        let mut inline = Convolver::deferred(&ir, 64, largest, Head::Direct, from);
        let mut threaded = Convolver::deferred(&ir, 64, largest, Head::Direct, from)
            .with_worker(Workers::new(Deadline::Offline));
        inline.prepare(48_000.0);
        threaded.prepare(48_000.0);
        let (a, b) = (run(&mut inline, &x), run(&mut threaded, &x));
        let same = a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits());
        println!(
            "{taps} taps, deferred from {from}: {} worker thread(s), schedule {:?}",
            threaded.deferred_runs(),
            threaded.schedule().collect::<Vec<_>>()
        );
        assert!(same, "{taps} taps: threaded output differs");
        assert_eq!(threaded.late_blocks(), 0);
        // A reset mid-stream restarts both the same way.
        inline.reset();
        threaded.reset();
        let (a, b) = (
            run(&mut inline, &x[..10_000]),
            run(&mut threaded, &x[..10_000]),
        );
        assert!(
            a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()),
            "after reset"
        );
    }
    // True stereo.
    let (h, g) = (hall(9_000), noise(11, 9_000));
    let ir = ImpulseResponse::true_stereo(&h, &g, &g, &h);
    let mut inline = Convolver::deferred(&ir, 32, 1_024, Head::Partitioned, 256);
    let mut threaded = Convolver::deferred(&ir, 32, 1_024, Head::Partitioned, 256)
        .with_worker(Workers::new(Deadline::Offline));
    inline.prepare(48_000.0);
    threaded.prepare(48_000.0);
    let r = noise(5, x.len());
    for (n, (&l, &r)) in x.iter().zip(&r).enumerate() {
        let (a, b) = (inline.tick_stereo(l, r), threaded.tick_stereo(l, r));
        assert_eq!(
            (a.0.to_bits(), a.1.to_bits()),
            (b.0.to_bits(), b.1.to_bits()),
            "sample {n}"
        );
    }
}

/// Realtime, with the worker given its period before every boundary
/// (the test settles it), nothing is late and the output is the
/// inline reference's, bit for bit.
#[test]
fn settled_realtime_workers_match_inline() {
    let ir = ImpulseResponse::mono(&hall(20_000));
    let x = noise(9, 40_000);
    let mut inline = Convolver::deferred(&ir, 64, 2_048, Head::Direct, 512);
    let mut threaded = Convolver::deferred(&ir, 64, 2_048, Head::Direct, 512)
        .with_worker(Workers::new(Deadline::Realtime));
    inline.prepare(48_000.0);
    threaded.prepare(48_000.0);
    let a = run(&mut inline, &x);
    let mut b = Vec::with_capacity(x.len());
    // Every deferred boundary falls on a multiple of the smallest
    // deferred size.
    for chunk in x.chunks(512) {
        b.extend(chunk.iter().map(|&s| threaded.tick(s)));
        threaded.worker().settle();
    }
    assert!(a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()));
    assert_eq!(threaded.late_blocks(), 0);
    let report = threaded.worker().report();
    assert!(
        report.iter().all(|r| r.late == 0 && r.dropped == 0),
        "{report:?}"
    );
}

/// A worker held up past its deadline: that one block of its run plays
/// as silence, the miss is counted by the convolver and the worker's
/// report, and from the next block on the output is exact again (the
/// late block still reached the run's history).
#[test]
fn a_slow_worker_misses_one_block_and_says_so() {
    const L: usize = 512;
    let ir = ImpulseResponse::mono(&hall(16_000));
    let x = noise(13, 16 * L);
    let held = Arc::new(AtomicBool::new(true));
    let hook = {
        let held = Arc::clone(&held);
        Arc::new(move |run: usize, seq: u64| {
            if run == 0 && seq == 3 {
                while held.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        })
    };
    let mut inline = Convolver::deferred(&ir, 64, L, Head::Direct, L);
    let mut threaded = Convolver::deferred(&ir, 64, L, Head::Direct, L)
        .with_worker(Workers::with_hook(Deadline::Realtime, hook));
    assert_eq!(threaded.deferred_runs(), 1);
    inline.prepare(48_000.0);
    threaded.prepare(48_000.0);
    let a = run(&mut inline, &x);
    let mut b = Vec::with_capacity(x.len());
    // Chunk c ends with exchange c: it hands over job c and collects
    // job c − 1, which plays through chunk c + 1. Job 3 is held, so
    // exchange 4 finds nothing and chunk 5 is that run's silence.
    for (c, chunk) in x.chunks(L).enumerate() {
        b.extend(chunk.iter().map(|&s| threaded.tick(s)));
        match c {
            3 => {} // job 3 is stuck: nothing to settle
            4 => {
                held.store(false, Ordering::Release);
                threaded.worker().settle();
            }
            _ => threaded.worker().settle(),
        }
    }
    let differs: Vec<usize> = (0..x.len() / L)
        .filter(|&c| (c * L..(c + 1) * L).any(|n| a[n].to_bits() != b[n].to_bits()))
        .collect();
    println!("chunks that differ from the inline reference: {differs:?}");
    assert_eq!(differs, [5], "exactly the missed block");
    assert_eq!(threaded.late_blocks(), 1);
    let report = threaded.worker().report();
    assert_eq!((report[0].late, report[0].dropped), (1, 0), "{report:?}");
    println!("{report:?}");
}

/// Hogs on every core while the convolver runs in realtime, paced like a
/// host (a 256-sample buffer per buffer period): every sample that
/// differs from the inline reference lies in a block reported late (or
/// after a reported drop, within the response's length), and offline
/// under the same load the output is still bit-identical.
#[test]
#[cfg_attr(miri, ignore)]
fn under_load_every_difference_is_reported() {
    const BUFFER: usize = 256;
    const RATE: f32 = 48_000.0;
    let h = hall(24_000);
    let ir = ImpulseResponse::mono(&h);
    let x = noise(17, 48_000);
    let stop = Arc::new(AtomicBool::new(false));
    let hogs: Vec<_> = (0..std::thread::available_parallelism().map_or(4, |n| n.get()))
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut v = 1u64;
                while !stop.load(Ordering::Relaxed) {
                    v = v.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    std::hint::black_box(v);
                }
            })
        })
        .collect();

    let mut inline = Convolver::deferred(&ir, 64, 4_096, Head::Direct, 256);
    inline.prepare(RATE);
    let a = run(&mut inline, &x);

    let mut offline = Convolver::deferred(&ir, 64, 4_096, Head::Direct, 256)
        .with_worker(Workers::new(Deadline::Offline));
    offline.prepare(RATE);
    let b = run(&mut offline, &x);
    assert!(
        a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()),
        "offline under load"
    );

    let mut live = Convolver::deferred(&ir, 64, 4_096, Head::Direct, 256)
        .with_worker(Workers::new(Deadline::Realtime));
    live.prepare(RATE);
    assert!(live.deferred_runs() > 0);
    let period = Duration::from_secs_f64(BUFFER as f64 / f64::from(RATE));
    let start = Instant::now();
    let mut y = Vec::with_capacity(x.len());
    // Per buffer: the late and dropped counts after it.
    let mut events = Vec::new();
    for (i, buffer) in x.chunks(BUFFER).enumerate() {
        y.extend(buffer.iter().map(|&s| live.tick(s)));
        let dropped: u64 = live.worker().report().iter().map(|r| r.dropped).sum();
        events.push((live.late_blocks(), dropped));
        let due = start + period * (i as u32 + 1);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    stop.store(true, Ordering::Relaxed);
    for hog in hogs {
        hog.join().unwrap();
    }
    let largest = 4_096;
    let mut unexplained = Vec::new();
    for (i, buffer) in y.chunks(BUFFER).enumerate() {
        let start = i * BUFFER;
        if buffer
            .iter()
            .zip(&a[start..])
            .all(|(p, q)| p.to_bits() == q.to_bits())
        {
            continue;
        }
        // A late block silences up to one period of its run after the
        // boundary; a drop restarts the run's history, so it may differ
        // for the response's length.
        let late_window = largest / BUFFER + 1;
        let drop_window = (h.len() + 2 * largest) / BUFFER + 1;
        // The cumulative counts after buffer `j` (0 before the first).
        let count = |j: Option<usize>, late: bool| {
            j.map_or(0, |j| if late { events[j].0 } else { events[j].1 })
        };
        // A count that rose in the `window` buffers before this one.
        let rose = |window: usize, late: bool| {
            let now = count(i.checked_sub(1), late);
            let then = count(i.checked_sub(window + 1), late);
            now > then
        };
        if !rose(late_window, true) && !rose(drop_window, false) {
            unexplained.push(i);
        }
    }
    let report = live.worker().report();
    println!(
        "under load: {} late blocks, report {report:?}, {} buffers differ",
        live.late_blocks(),
        y.chunks(BUFFER)
            .enumerate()
            .filter(|(i, b)| b
                .iter()
                .zip(&a[i * BUFFER..])
                .any(|(p, q)| p.to_bits() != q.to_bits()))
            .count()
    );
    assert!(
        unexplained.is_empty(),
        "differences no miss explains: buffers {unexplained:?}"
    );
}

/// The audio thread's cost per 64-sample block, paced in real time: the
/// single-threaded schedule pays its large partitions in the block where
/// they step (F3 measured 1.2 µs/sample for 96 k taps); with the worker
/// threads the worst block is the B-sized run and two buffer swaps.
/// Printed, not asserted (wall-clock on a shared machine); the late
/// count is.
#[test]
#[cfg_attr(miri, ignore)]
fn audio_thread_cost_per_block() {
    const RATE: f32 = 48_000.0;
    const BLOCK: usize = 64;
    let ir = ImpulseResponse::mono(&hall(96_000));
    let x = noise(19, 96_000);
    let measure = |conv: &mut dyn FnMut(&mut [f32])| {
        let mut buf = vec![0.0f32; BLOCK];
        let mut warm = Vec::new();
        let mut total = 0.0f64;
        let period = Duration::from_secs_f64(BLOCK as f64 / f64::from(RATE));
        let start = Instant::now();
        for (i, chunk) in x.chunks(BLOCK).enumerate() {
            buf.copy_from_slice(chunk);
            let t0 = Instant::now();
            conv(&mut buf);
            let ns = t0.elapsed().as_secs_f64() * 1e9 / BLOCK as f64;
            // Skip the first second's cold caches.
            if i * BLOCK >= 48_000 {
                warm.push(ns);
            }
            total += ns;
            let due = start + period * (i as u32 + 1);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        warm.sort_by(f64::total_cmp);
        let p99 = warm[warm.len() * 99 / 100];
        (total / (x.len() / BLOCK) as f64, p99, warm[warm.len() - 1])
    };
    let mut single: Convolver<Inline> = Convolver::non_uniform(&ir, 64, 8_192, Head::Direct);
    single.prepare(RATE);
    let (avg1, p1, worst1) = measure(&mut |b| single.process(b));
    let mut threaded = Convolver::deferred(&ir, 64, 8_192, Head::Direct, 512)
        .with_worker(Workers::new(Deadline::Realtime));
    threaded.prepare(RATE);
    let (avg2, p2, worst2) = measure(&mut |b| threaded.process(b));
    let report = threaded.worker().report();
    println!("96 k taps, B = 64, paced at 48 kHz, ns/sample on the audio thread:");
    println!("  single-threaded {:?}: average {avg1:.0}, 99th-percentile block {p1:.0}, worst block {worst1:.0}", single.schedule().collect::<Vec<_>>());
    println!("  worker threads  {:?}: average {avg2:.0}, 99th-percentile block {p2:.0}, worst block {worst2:.0}", threaded.schedule().collect::<Vec<_>>());
    for r in &report {
        println!(
            "  worker {}: {} blocks, worst compute {:?}, least slack {:.2} ms of {:.2} ms, {} late, {} dropped",
            r.size,
            r.blocks,
            r.worst_compute,
            r.least_slack_s.unwrap_or(f64::NAN) * 1e3,
            r.size as f64 / f64::from(RATE) * 1e3,
            r.late,
            r.dropped
        );
    }
    println!("  late blocks: {}", threaded.late_blocks());
}
