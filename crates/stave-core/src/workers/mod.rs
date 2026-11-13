//! Worker threads for a [`Convolver`]'s deferred runs (DSP.md F3b): one
//! thread per partition size, each with a deadline of one period, fed
//! and drained through a lock-free, allocation-free [`handoff()`].
//!
//! stave-dsp is pure (no threads, atomics or clocks), so it defines the
//! [`Worker`] contract and computes deferred runs [`Inline`]; this is
//! the threaded worker, in the imperative shell the plugin owns:
//!
//! ```
//! use stave_core::workers::{Deadline, Workers};
//! use stave_dsp::primitives::spectral::Head;
//! use stave_dsp::{Convolver, ImpulseResponse, Stage};
//!
//! let hall = ImpulseResponse::mono(&vec![0.001; 48_000]);
//! // Built off the audio thread: the 2048-sample runs go to a thread.
//! let mut conv = Convolver::deferred(&hall, 64, 4_096, Head::Direct, 1_024)
//!     .with_worker(Workers::new(Deadline::Offline));
//! conv.prepare(48_000.0);
//! let y: Vec<f32> = (0..256).map(|n| conv.tick(if n == 0 { 1.0 } else { 0.0 })).collect();
//! assert!((y[0] - 0.001).abs() < 1e-6);
//! assert_eq!(conv.late_blocks(), 0);
//! ```
//!
//! [`Convolver`]: stave_dsp::Convolver
//! [`Worker`]: stave_dsp::primitives::spectral::Worker
//! [`Inline`]: stave_dsp::primitives::spectral::Inline

mod handoff;

pub use handoff::{handoff, Receiver, Sender};

use std::sync::atomic::{fence, AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use stave_dsp::primitives::spectral::{Run, Worker};

/// Jobs in flight per run: the audio thread may run this many periods
/// ahead of a worker before a block is dropped.
const JOBS: usize = 4;
/// Result slots per run: more than jobs, so a worker never waits to
/// deliver.
const RESULTS: usize = 2 * JOBS;

/// What the audio thread does when a deferred block is not ready at its
/// boundary.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Deadline {
    /// Never wait: that run's block plays as silence, the miss is
    /// counted (`Convolver::late_blocks`, [`RunReport::late`]), and the
    /// late block, when it arrives, is discarded. The run's history is
    /// intact, so the next block on time is exact again. For live
    /// processing.
    Realtime,
    /// Wait for the block (a bounded spin, then yielding): the output is
    /// bit-identical to the [`Inline`] worker's whatever the scheduling.
    /// For offline bounces and tests; never on a live audio thread.
    ///
    /// [`Inline`]: stave_dsp::primitives::spectral::Inline
    Offline,
}

/// One run's timing, from [`Workers::report`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RunReport {
    /// The partition size: the run's period and deadline, in samples.
    pub size: usize,
    /// Blocks the worker computed.
    pub blocks: u64,
    /// Blocks not ready at their boundary (played as silence).
    pub late: u64,
    /// Blocks the audio thread could not hand over because the worker
    /// was four periods behind; the worker then restarted the run's
    /// history, so its part of the response rebuilt from silence.
    pub dropped: u64,
    /// The longest one block took to compute.
    pub worst_compute: Duration,
    /// The least time any block had to spare against its deadline (one
    /// period after it was handed over), seconds; negative when a block
    /// finished late. `None` before `prepare` or the first block.
    pub least_slack_s: Option<f64>,
}

/// A worker's job and result: one block, its sequence number, the
/// reset epoch it belongs to, and when the audio thread handed it over.
struct Job {
    data: Box<[f32]>,
    seq: u64,
    epoch: u64,
    /// The run's history must restart (a job before it was dropped).
    restart: bool,
    sent: Option<Instant>,
}

impl Job {
    fn new(len: usize) -> Self {
        Self {
            data: vec![0.0; len].into_boxed_slice(),
            seq: 0,
            epoch: 0,
            restart: false,
            sent: None,
        }
    }
}

/// What a worker thread and the audio thread share about one run.
struct Shared {
    stop: AtomicBool,
    sleeping: AtomicBool,
    blocks: AtomicU64,
    late: AtomicU64,
    dropped: AtomicU64,
    worst_compute_ns: AtomicU64,
    least_slack_ns: AtomicI64,
    /// One period, ns (0 before `prepare`).
    deadline_ns: AtomicU64,
}

/// A hook a test may run before each block (run index, sequence
/// number), to make a worker slow on purpose.
pub type Hook = Arc<dyn Fn(usize, u64) + Send + Sync>;

/// One run's audio-thread side.
struct Port {
    size: usize,
    jobs: Sender<Job>,
    results: Receiver<Job>,
    shared: Arc<Shared>,
    thread: Thread,
    handle: Option<JoinHandle<()>>,
    /// The job being staged; its buffer is swapped with the input.
    spare: Job,
    next_seq: u64,
    /// The sequence number whose block the next boundary plays.
    expect: Option<u64>,
    epoch: u64,
    restart: bool,
}

/// A [`Worker`] with one thread per deferred run (per partition size),
/// after Battenberg & Avizienis 2011: a run of size `L` gets each input
/// block at its boundary and must deliver the output block by the next,
/// one period (`L` samples) later; the smallest run stays on the audio
/// thread, inside the convolver.
///
/// The audio thread's side of [`exchange`](Worker::exchange) is two
/// swaps through [`handoff()`] rings, a clock read and, when the worker is
/// asleep, an unpark (a futex or ulock wake): no lock, no allocation, no
/// waiting under [`Deadline::Realtime`]. A missed deadline plays that
/// run's block as silence and is counted; nothing is ever played late or
/// twice. The worker threads run at the default priority; the host's
/// audio thread outranks them, and each size having its own thread means
/// a long block never delays a short one's deadline.
pub struct Workers {
    deadline: Deadline,
    hook: Option<Hook>,
    sample_rate: f32,
    ports: Vec<Port>,
}

impl core::fmt::Debug for Workers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Workers")
            .field("deadline", &self.deadline)
            .field(
                "runs",
                &self.ports.iter().map(|p| p.size).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Workers {
    /// No threads until a convolver [`assign`](Worker::assign)s its runs.
    pub fn new(deadline: Deadline) -> Self {
        Self {
            deadline,
            hook: None,
            sample_rate: 0.0,
            ports: Vec::new(),
        }
    }

    /// With `hook` run on the worker thread before each block (for tests
    /// that make a worker slow on purpose).
    pub fn with_hook(deadline: Deadline, hook: Hook) -> Self {
        let mut workers = Self::new(deadline);
        workers.hook = Some(hook);
        workers
    }

    /// Each run's timing so far. Allocates: not on the audio thread.
    pub fn report(&self) -> Vec<RunReport> {
        self.ports
            .iter()
            .map(|p| {
                let s = &p.shared;
                let slack = s.least_slack_ns.load(Ordering::Relaxed);
                RunReport {
                    size: p.size,
                    blocks: s.blocks.load(Ordering::Relaxed),
                    late: s.late.load(Ordering::Relaxed),
                    dropped: s.dropped.load(Ordering::Relaxed),
                    worst_compute: Duration::from_nanos(s.worst_compute_ns.load(Ordering::Relaxed)),
                    least_slack_s: (slack != i64::MAX).then_some(slack as f64 * 1e-9),
                }
            })
            .collect()
    }

    /// Wait until every block handed over has been computed (for tests
    /// and offline bounces; it spins).
    pub fn settle(&self) {
        for port in &self.ports {
            while port.jobs.in_flight() > 0 {
                std::hint::spin_loop();
                std::thread::yield_now();
            }
        }
    }

    fn stop(&mut self) {
        for port in &mut self.ports {
            port.shared.stop.store(true, Ordering::SeqCst);
            port.thread.unpark();
            if let Some(handle) = port.handle.take() {
                // A worker that panicked has nothing left to clean up.
                let _ = handle.join();
            }
        }
        self.ports.clear();
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Worker for Workers {
    fn assign(&mut self, runs: Vec<Run>) {
        self.stop();
        for (index, run) in runs.into_iter().enumerate() {
            let len = run.block_len();
            let size = run.size();
            let (jobs, jobs_rx) = handoff((0..JOBS).map(|_| Job::new(len)).collect());
            let (results_tx, results) = handoff((0..RESULTS).map(|_| Job::new(len)).collect());
            let shared = Arc::new(Shared {
                stop: AtomicBool::new(false),
                sleeping: AtomicBool::new(false),
                blocks: AtomicU64::new(0),
                late: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
                worst_compute_ns: AtomicU64::new(0),
                least_slack_ns: AtomicI64::new(i64::MAX),
                deadline_ns: AtomicU64::new(period_ns(size, self.sample_rate)),
            });
            let thread_shared = Arc::clone(&shared);
            let hook = self.hook.clone();
            let handle = std::thread::Builder::new()
                .name(format!("stave-convolver-{size}"))
                .spawn(move || serve(index, run, jobs_rx, results_tx, &thread_shared, hook))
                .expect("spawn a convolver worker thread");
            self.ports.push(Port {
                size,
                jobs,
                results,
                shared,
                thread: handle.thread().clone(),
                handle: Some(handle),
                spare: Job::new(len),
                next_seq: 0,
                expect: None,
                epoch: 0,
                restart: false,
            });
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for port in &self.ports {
            port.shared
                .deadline_ns
                .store(period_ns(port.size, sample_rate), Ordering::Relaxed);
        }
    }

    fn reset(&mut self) {
        for port in &mut self.ports {
            // Jobs of the old epoch still in flight are computed and
            // discarded; the worker restarts the run's history on the
            // first job of the new one.
            port.epoch += 1;
            port.expect = None;
            port.restart = false;
        }
    }

    fn exchange(&mut self, index: usize, input: &mut Box<[f32]>, output: &mut Box<[f32]>) -> bool {
        let deadline = self.deadline;
        let port = &mut self.ports[index];
        // Collect the block computed from the previous boundary's input.
        let mut on_time = true;
        match port.expect.take() {
            None => output.fill(0.0),
            Some(want) => {
                let mut spins = 0u32;
                let got = loop {
                    let taken = port.results.peek(|job| {
                        if job.seq == want {
                            core::mem::swap(output, &mut job.data);
                            Some(true)
                        } else if job.seq < want {
                            Some(false) // late, superseded: discard
                        } else {
                            None
                        }
                    });
                    match taken {
                        Some(Some(true)) => {
                            port.results.release();
                            break true;
                        }
                        Some(Some(false)) => port.results.release(),
                        Some(None) => break false,
                        None if deadline == Deadline::Offline => {
                            spins += 1;
                            if spins < 64 {
                                std::hint::spin_loop();
                            } else {
                                std::thread::yield_now();
                            }
                        }
                        None => break false,
                    }
                };
                if !got {
                    output.fill(0.0);
                    port.shared.late.fetch_add(1, Ordering::Relaxed);
                    on_time = false;
                }
            }
        }
        // Hand over this boundary's input.
        core::mem::swap(input, &mut port.spare.data);
        port.spare.seq = port.next_seq;
        port.spare.epoch = port.epoch;
        port.spare.restart = port.restart;
        port.spare.sent = Some(Instant::now());
        let sent = if port.jobs.send_swap(&mut port.spare) {
            port.expect = Some(port.next_seq);
            port.next_seq += 1;
            port.restart = false;
            true
        } else {
            // The worker is a full ring behind: drop this block and have
            // it restart the run's history on the next.
            port.shared.dropped.fetch_add(1, Ordering::Relaxed);
            port.restart = true;
            false
        };
        // `input` takes back a spare buffer (or its own data, unsent).
        core::mem::swap(input, &mut port.spare.data);
        if sent {
            // Dekker with the worker's sleep: publish, then look.
            fence(Ordering::SeqCst);
            if port.shared.sleeping.load(Ordering::Relaxed) {
                port.thread.unpark();
            }
        }
        on_time
    }
}

fn period_ns(size: usize, sample_rate: f32) -> u64 {
    if sample_rate > 0.0 {
        (size as f64 / f64::from(sample_rate) * 1e9) as u64
    } else {
        0
    }
}

/// A worker thread: compute each job into a result, in order, until
/// stopped; sleep when there is nothing to do.
fn serve(
    index: usize,
    mut run: Run,
    mut jobs: Receiver<Job>,
    mut results: Sender<Job>,
    shared: &Shared,
    hook: Option<Hook>,
) {
    let mut out = Job::new(run.block_len());
    let mut epoch = 0u64;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let started = Instant::now();
        let done = jobs.peek(|job| {
            if job.epoch != epoch || job.restart {
                run.reset();
                epoch = job.epoch;
            }
            if let Some(hook) = &hook {
                hook(index, job.seq);
            }
            run.step(&job.data, &mut out.data);
            out.seq = job.seq;
            out.epoch = job.epoch;
            job.sent
        });
        let Some(sent) = done else {
            // Dekker with the audio thread's hand-over: announce, then
            // look once more before sleeping.
            shared.sleeping.store(true, Ordering::Relaxed);
            fence(Ordering::SeqCst);
            if jobs.peek(|_| ()).is_none() && !shared.stop.load(Ordering::SeqCst) {
                std::thread::park();
            }
            shared.sleeping.store(false, Ordering::Relaxed);
            continue;
        };
        let finished = Instant::now();
        while !results.send_swap(&mut out) {
            // Never expected (twice the slots of the jobs ring); yield
            // until the audio thread drains one.
            if shared.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::yield_now();
        }
        jobs.release();
        let compute = finished.duration_since(started).as_nanos() as u64;
        shared.blocks.fetch_add(1, Ordering::Relaxed);
        shared
            .worst_compute_ns
            .fetch_max(compute, Ordering::Relaxed);
        let deadline = shared.deadline_ns.load(Ordering::Relaxed);
        if let (Some(sent), true) = (sent, deadline > 0) {
            let taken = finished.saturating_duration_since(sent).as_nanos() as i64;
            shared
                .least_slack_ns
                .fetch_min(deadline as i64 - taken, Ordering::Relaxed);
        }
    }
}
