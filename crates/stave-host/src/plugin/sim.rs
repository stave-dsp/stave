//! A deterministic simulated host (docs/overhaul.md V3): seeded
//! randomness over the VST3 call order — setup, activation, processing
//! with any block size (0, odd, past the declared maximum), sample-rate
//! changes, in-place and null channel lists, sample-accurate and
//! hostile parameter changes, state save/load (round trips, garbage,
//! truncation) — legal and illegal, with process calls on an audio
//! thread and the rest on the main thread. After every call an oracle
//! checks the plugin against the lifecycle model
//! (`stave_model::lifecycle`): what it must answer, that a block it
//! may not run is silent, that output is finite, that state round-trips
//! and a refused load changes nothing, that the plugin never logged a
//! panic. Any failure names its seed; [`simulate`] with that seed
//! replays it exactly.

use std::fmt;

use stave_model::lifecycle::{self, HostCall, Lifecycle};
use stave_vst3_sys::{
    AudioBusBuffers, ProcessData, ProcessSetup, K_INVALID_ARGUMENT, K_RESULT_OK,
    PROCESS_MODE_REALTIME, SAMPLE_SIZE_32,
};

use super::com::stream::StreamHandle;
use super::module::EntryPoints;
use super::LoadedPlugin;
use crate::config::{HostConfig, SampleRate};
use crate::instance::Instance;

/// How long a simulation runs and how hostile it is.
#[derive(Copy, Clone, Debug)]
pub struct SimConfig {
    /// Host calls per run.
    pub steps: usize,
    /// Of every 16 calls, how many may break the format's call-order
    /// contract (0: a compliant host).
    pub illegal_per_16: u32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            steps: 200,
            illegal_per_16: 3,
        }
    }
}

/// What a run did, for determinism checks and reports.
#[derive(Clone, Debug, PartialEq)]
pub struct SimReport {
    pub seed: u64,
    /// Every call in order, as the host made it.
    pub calls: Vec<HostCall>,
    /// FNV-1a over every output sample and every result code: two runs
    /// of one seed must agree.
    pub digest: u64,
}

/// A run that broke a law: replay with `simulate(entry, seed, config)`.
#[derive(Clone, Debug)]
pub struct SimFailure {
    pub seed: u64,
    pub step: usize,
    pub call: HostCall,
    pub law: String,
    /// The calls before the failing one.
    pub calls: Vec<HostCall>,
}

impl fmt::Display for SimFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "simulated host, seed {} (replay: STAVE_SIM_SEED={}), step {}: {:?}",
            self.seed, self.seed, self.step, self.call
        )?;
        writeln!(f, "  broke: {}", self.law)?;
        let from = self.calls.len().saturating_sub(12);
        writeln!(f, "  the last {} calls before it:", self.calls.len() - from)?;
        for (i, call) in self.calls.iter().enumerate().skip(from) {
            writeln!(f, "    {i:4} {call:?}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SimFailure {}

/// SplitMix64: tiny, seedable, and the same on every platform — or,
/// for a fuzzer, a tape of its bytes (zeros once it runs out).
struct Rng(u64, Option<(Vec<u8>, usize)>);

impl Rng {
    fn seeded(seed: u64) -> Self {
        Self(seed, None)
    }

    fn next(&mut self) -> u64 {
        if let Some((tape, at)) = &mut self.1 {
            let mut word = [0u8; 8];
            for b in &mut word {
                *b = tape.get(*at).copied().unwrap_or(0);
                *at += 1;
            }
            return u64::from_le_bytes(word);
        }
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Fnv(u64);

impl Fnv {
    fn mix(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
}

/// Frames per host buffer: room for the largest block the sim ever asks
/// for (past any declared maximum it picks).
const ROOM: usize = 2048;
const RATES: [f64; 6] = [22_050.0, 44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0];
const MAX_BLOCKS: [i32; 6] = [1, 7, 64, 333, 512, 1024];

/// Run `config.steps` seeded host calls against the VST3 plugin behind
/// `entry`, checking every law after each.
pub fn simulate(entry: EntryPoints, seed: u64, config: SimConfig) -> Result<SimReport, SimFailure> {
    run(entry, seed, Rng::seeded(seed), config)
}

/// As [`simulate`], every choice read from `tape` instead of a seeded
/// generator (zeros past its end): a coverage-guided fuzzer steers the
/// host (`fuzz/fuzz_targets/vst3_host.rs`). Failures report seed 0; the
/// tape is the replay.
pub fn simulate_tape(entry: EntryPoints, tape: &[u8], config: SimConfig) -> Result<SimReport, SimFailure> {
    run(entry, 0, Rng(0, Some((tape.to_vec(), 0))), config)
}

fn run(entry: EntryPoints, seed: u64, rng: Rng, config: SimConfig) -> Result<SimReport, SimFailure> {
    let host = HostConfig {
        sample_rate: SampleRate::new(48_000.0).expect("a valid rate"),
        max_block: std::num::NonZeroUsize::new(512).expect("non-zero"),
        ..HostConfig::default()
    };
    let plugin = LoadedPlugin::from_entry_points(entry, host).map_err(|e| SimFailure {
        seed,
        step: 0,
        call: HostCall::Setup {
            sample_rate: 48_000.0,
            max_block: 512,
        },
        law: format!("the plugin loads: {e}"),
        calls: Vec::new(),
    })?;
    let mut sim = Sim {
        model: Lifecycle::active(48_000.0, 512),
        rng,
        digest: Fnv(0xCBF2_9CE4_8422_2325),
        saved: Vec::new(),
        calls: Vec::with_capacity(config.steps),
        params: plugin.controls().iter().map(|c| c.id).collect(),
        messages_seen: 0,
        plugin: Some(plugin),
        audio: AudioThread::spawn(),
    };
    for step in 0..config.steps {
        let call = sim.choose(config.illegal_per_16);
        if let Err(law) = sim.apply(call.clone()) {
            return Err(SimFailure {
                seed,
                step,
                call,
                law,
                calls: sim.calls,
            });
        }
        sim.calls.push(call);
    }
    Ok(SimReport {
        seed,
        calls: sim.calls,
        digest: sim.digest.0,
    })
}

struct Sim {
    model: Lifecycle,
    rng: Rng,
    digest: Fnv,
    /// States the plugin saved, for later loads.
    saved: Vec<Vec<u8>>,
    calls: Vec<HostCall>,
    params: Vec<u32>,
    messages_seen: u64,
    /// `None` only while the audio thread holds it.
    plugin: Option<LoadedPlugin>,
    audio: AudioThread,
}

impl Sim {
    fn plugin(&mut self) -> &mut LoadedPlugin {
        self.plugin.as_mut().expect("on the main thread between calls")
    }

    /// A call the model allows now (most of the time), or any call.
    fn choose(&mut self, illegal_per_16: u32) -> HostCall {
        let any = self.rng.below(16) < u64::from(illegal_per_16);
        for _ in 0..64 {
            let call = self.random_call();
            if any || self.model.allows(&call) {
                return call;
            }
        }
        // A spent fuzzer tape repeats one choice: fall back to a call
        // that is always allowed.
        HostCall::SaveState
    }

    fn random_call(&mut self) -> HostCall {
        let r = &mut self.rng;
        match r.below(20) {
            0 => HostCall::Setup {
                sample_rate: r.pick(&RATES),
                max_block: r.pick(&MAX_BLOCKS) as usize,
            },
            1 | 2 => HostCall::SetActive(r.below(2) == 1),
            3 => HostCall::SetProcessing(r.below(2) == 1),
            4 | 5 => HostCall::SetParam {
                index: r.below(self.params.len() as u64) as usize,
                normalized: match r.below(4) {
                    0 => 0.0,
                    1 => 1.0,
                    2 => 0.5,
                    _ => r.unit(),
                },
            },
            6 => HostCall::SaveState,
            7 => match self.saved.len() {
                0 => HostCall::SaveState,
                n => HostCall::LoadSaved(r.below(n as u64) as usize),
            },
            8 => HostCall::LoadGarbage {
                seed: r.next(),
                len: r.pick(&[0usize, 1, 4, 13, 64, 300]),
            },
            9 => match self.saved.len() {
                0 => HostCall::SaveState,
                n => {
                    let slot = r.below(n as u64) as usize;
                    let cut = r.below(self.saved[slot].len() as u64) as usize;
                    HostCall::LoadTruncated { slot, len: cut }
                }
            },
            10 => HostCall::SaveDuringProcess {
                frames: r.pick(&[1usize, 64, 512]),
            },
            _ => {
                let max = self.model.max_block();
                let frames = match r.below(8) {
                    0 => 0,
                    1 => 1,
                    2 => max,
                    3 => max + 1 + r.below(7) as usize,
                    _ => 1 + r.below(max.max(1) as u64) as usize,
                }
                .min(ROOM);
                HostCall::Process {
                    frames,
                    shape: r.pick(&lifecycle::BufferShape::ALL),
                    changes: r.below(4) as usize,
                    change_seed: r.next(),
                }
            }
        }
    }

    fn apply(&mut self, call: HostCall) -> Result<(), String> {
        let expect = self.model.expect(&call);
        match call {
            HostCall::Setup {
                sample_rate,
                max_block,
            } => {
                let r = self.setup(sample_rate, max_block);
                self.check_result("setupProcessing", r, expect.result_ok)?;
            }
            HostCall::SetActive(on) => {
                let c = &self.plugin().component;
                // SAFETY: the component's own method on the live object.
                let r = unsafe { (c.vtbl().set_active)(c.as_ptr(), u8::from(on)) };
                self.check_result("setActive", r, expect.result_ok)?;
            }
            HostCall::SetProcessing(on) => {
                let p = &self.plugin().processor;
                // SAFETY: the processor's own method on the live object.
                let r = unsafe { (p.vtbl().set_processing)(p.as_ptr(), u8::from(on)) };
                self.check_result("setProcessing", r, expect.result_ok)?;
            }
            HostCall::SetParam { index, normalized } => self.set_param(index, normalized)?,
            HostCall::SaveState => {
                let bytes = self.save()?;
                if let Some(last) = self.saved.last() {
                    if self.model.state_unchanged_since_save() && *last != bytes {
                        return Err("two saves with no change between them differ".into());
                    }
                }
                self.saved.push(bytes);
            }
            HostCall::LoadSaved(slot) => {
                let bytes = self.saved[slot].clone();
                self.load(&bytes, true)?;
                if self.save()? != bytes {
                    return Err(format!("state saved after loading slot {slot} differs from it"));
                }
            }
            HostCall::LoadGarbage { seed, len } => {
                let mut g = Rng::seeded(seed);
                let bytes: Vec<u8> = (0..len).map(|_| g.next() as u8).collect();
                self.refused_load_changes_nothing(&bytes)?;
            }
            HostCall::LoadTruncated { slot, len } => {
                let bytes = self.saved[slot][..len].to_vec();
                self.refused_load_changes_nothing(&bytes)?;
            }
            HostCall::Process {
                frames,
                shape,
                changes,
                change_seed,
            } => {
                let plugin = self.plugin.take().expect("main thread holds the plugin");
                let (plugin, outcome) = self
                    .audio
                    .run(plugin, move |p| process(p, frames, shape, changes, change_seed));
                self.plugin = Some(plugin);
                self.check_block(frames, outcome, &expect)?;
            }
            HostCall::SaveDuringProcess { frames } => {
                // The main thread saves while the audio thread runs a
                // block: a host saving a project during playback.
                let c = &self.plugin().component;
                let (com, get_state) = (SendPtr(c.as_ptr()), c.vtbl().get_state);
                let plugin = self.plugin.take().expect("main thread holds the plugin");
                let stream = StreamHandle::new(Vec::new());
                let (plugin, outcome) = self.audio.run_with(plugin, frames, || {
                    // SAFETY: getState on the live component from the
                    // main thread, concurrent with process — a call the
                    // format allows (the wrapper serializes internally).
                    unsafe { get_state(com.get(), stream.as_funknown()) }
                });
                self.plugin = Some(plugin);
                let (block, r) = outcome;
                self.check_result("getState during process", r, true)?;
                self.check_block(frames, block, &expect)?;
                // What was saved must load back.
                let bytes = stream.bytes();
                self.load(&bytes, true)?;
            }
        }
        self.model.apply(&call);
        self.check_messages()
    }

    fn setup(&mut self, sample_rate: f64, max_block: usize) -> i32 {
        let p = &self.plugin().processor;
        let mut setup = ProcessSetup {
            process_mode: PROCESS_MODE_REALTIME,
            symbolic_sample_size: SAMPLE_SIZE_32,
            max_samples_per_block: max_block as i32,
            sample_rate,
        };
        // SAFETY: the processor's own method on the live object.
        unsafe { (p.vtbl().setup_processing)(p.as_ptr(), &mut setup) }
    }

    fn set_param(&mut self, index: usize, normalized: f64) -> Result<(), String> {
        let Some(&id) = self.params.get(index) else {
            return Ok(());
        };
        let Some(c) = self.plugin().controller.as_ref() else {
            return Ok(());
        };
        let com = c.com();
        // SAFETY: the controller's own methods on the live object.
        let (r, back) = unsafe {
            let r = (com.vtbl().set_param_normalized)(com.as_ptr(), id, normalized);
            (r, (com.vtbl().get_param_normalized)(com.as_ptr(), id))
        };
        self.digest.mix(&back.to_bits().to_le_bytes());
        if r != K_RESULT_OK {
            return Err(format!("setParamNormalized({id}, {normalized}) answered {r}"));
        }
        if !(0.0..=1.0).contains(&back) {
            return Err(format!("param {id} reads back {back}, outside [0, 1]"));
        }
        let steps = self
            .plugin()
            .controls()
            .into_iter()
            .find(|c| c.id == id)
            .map_or(0, |c| c.steps);
        let tolerance = if steps == 0 { 1e-9 } else { 0.5 / f64::from(steps) + 1e-9 };
        if (back - normalized).abs() > tolerance {
            return Err(format!("param {id} set to {normalized} reads back {back}"));
        }
        Ok(())
    }

    fn save(&mut self) -> Result<Vec<u8>, String> {
        let bytes = self
            .plugin()
            .save_state()
            .map_err(|e| format!("getState refused: {e}"))?;
        self.digest.mix(&bytes);
        Ok(bytes)
    }

    fn load(&mut self, bytes: &[u8], must_accept: bool) -> Result<bool, String> {
        let ok = self.plugin().load_state(bytes).is_ok();
        self.digest.mix(&[u8::from(ok)]);
        if must_accept && !ok {
            return Err(format!("setState refused a state the plugin saved ({} bytes)", bytes.len()));
        }
        Ok(ok)
    }

    fn refused_load_changes_nothing(&mut self, bytes: &[u8]) -> Result<(), String> {
        let before = self.save()?;
        if !self.load(bytes, false)? {
            let after = self.save()?;
            if after != before {
                return Err(format!(
                    "a refused setState ({} bytes) changed the state",
                    bytes.len()
                ));
            }
        }
        Ok(())
    }

    fn check_result(&mut self, call: &str, r: i32, want_ok: bool) -> Result<(), String> {
        self.digest.mix(&r.to_le_bytes());
        match (want_ok, r == K_RESULT_OK) {
            (true, false) => Err(format!("{call} answered {r}, the model says it succeeds")),
            _ => Ok(()),
        }
    }

    fn check_block(
        &mut self,
        frames: usize,
        outcome: Block,
        expect: &lifecycle::Expect,
    ) -> Result<(), String> {
        self.digest.mix(&outcome.result.to_le_bytes());
        for s in &outcome.output {
            self.digest.mix(&s.to_bits().to_le_bytes());
        }
        let want = match expect.block {
            lifecycle::BlockExpect::Refused => K_INVALID_ARGUMENT,
            _ => K_RESULT_OK,
        };
        if outcome.result != want {
            return Err(format!(
                "process({frames} frames) answered {}, the model says {want}",
                outcome.result
            ));
        }
        if let Some(i) = outcome.output.iter().position(|s| !s.is_finite()) {
            return Err(format!("output sample {i} is {}", outcome.output[i]));
        }
        if expect.block == lifecycle::BlockExpect::Silent {
            if let Some(i) = outcome.output.iter().position(|&s| s != 0.0) {
                return Err(format!(
                    "an unprepared block is silent, but sample {i} is {}",
                    outcome.output[i]
                ));
            }
        }
        Ok(())
    }

    /// No panic, ever: the wrapper logs one as a fatal message.
    fn check_messages(&mut self) -> Result<(), String> {
        let seen = self.messages_seen;
        let messages = self.plugin().messages(seen);
        if let Some(last) = messages.last() {
            self.messages_seen = last.index + 1;
        }
        match messages.iter().find(|m| m.text.contains("panicked")) {
            Some(m) => Err(format!("the plugin logged: {}", m.text)),
            None => Ok(()),
        }
    }
}

fn in_place_shape(shape: lifecycle::BufferShape) -> bool {
    shape == lifecycle::BufferShape::InPlace
}

/// A raw COM pointer handed to the main-thread side of a concurrent
/// step; the object is live for the whole step (the sim owns it).
#[derive(Copy, Clone)]
struct SendPtr(*mut stave_vst3_sys::FUnknown);

impl SendPtr {
    fn get(self) -> *mut stave_vst3_sys::FUnknown {
        self.0
    }
}
// SAFETY: used only to call a method the format allows from any thread
// while the plugin is alive.
unsafe impl Send for SendPtr {}
// SAFETY: as above.
unsafe impl Sync for SendPtr {}

/// One block's outcome.
struct Block {
    result: i32,
    /// Every output channel's frames, back to back.
    output: Vec<f32>,
}

/// One process call with buffers shaped as `shape`, plus `changes`
/// parameter points (out of order, some past the block) when there are
/// parameters.
fn process(
    p: &mut LoadedPlugin,
    frames: usize,
    shape: lifecycle::BufferShape,
    changes: usize,
    change_seed: u64,
) -> Block {
    use lifecycle::BufferShape;
    let (ins, outs) = (p.layout.inputs, p.layout.outputs);
    let mut g = Rng::seeded(change_seed);
    // One allocation for every channel: inputs then outputs. In-place
    // points the inputs at the outputs.
    let mut memory = vec![0.0f32; (ins + outs) * ROOM];
    for (i, s) in memory.iter_mut().enumerate().take(ins * ROOM) {
        *s = ((i as f32) * 0.013).sin() * 0.5;
    }
    if !in_place_shape(shape) {
        // Stale host memory: a block that must be silent has to write it.
        memory[ins * ROOM..].fill(0.25);
    }
    let base = memory.as_mut_ptr();
    let chan = |c: usize| {
        debug_assert!(c < ins + outs);
        // SAFETY: channel `c` of `ins + outs` lies inside `memory`.
        unsafe { base.add(c * ROOM) }
    };
    let in_place = shape == BufferShape::InPlace;
    let mut in_ptrs: Vec<*mut f32> = (0..ins)
        .map(|c| if in_place && c < outs { chan(ins + c) } else { chan(c) })
        .collect();
    let mut out_ptrs: Vec<*mut f32> = (0..outs).map(|c| chan(ins + c)).collect();
    if in_place {
        // The host's input is the output memory, holding the signal.
        for c in 0..ins.min(outs) {
            for f in 0..ROOM {
                // SAFETY: inside `memory`.
                unsafe { *chan(ins + c).add(f) = *chan(c).add(f) };
            }
        }
    }
    match shape {
        BufferShape::NullInput if ins > 0 => in_ptrs[0] = std::ptr::null_mut(),
        BufferShape::NullOutput if outs > 0 => out_ptrs[outs - 1] = std::ptr::null_mut(),
        _ => {}
    }
    p.changes.clear();
    let ids: Vec<u32> = p.controls().iter().map(|c| c.id).collect();
    for _ in 0..changes {
        if ids.is_empty() {
            break;
        }
        let id = ids[g.below(ids.len() as u64) as usize];
        let offset = g.below(frames as u64 + 3) as i32 - 1; // −1 and past the end too
        p.changes.push(id, offset, g.unit());
    }
    let mut in_bus = AudioBusBuffers {
        num_channels: ins as i32,
        silence_flags: 0,
        channel_buffers_32: in_ptrs.as_mut_ptr(),
    };
    let mut out_bus = AudioBusBuffers {
        num_channels: outs as i32,
        silence_flags: 0,
        channel_buffers_32: out_ptrs.as_mut_ptr(),
    };
    let mut data = ProcessData {
        process_mode: PROCESS_MODE_REALTIME,
        symbolic_sample_size: SAMPLE_SIZE_32,
        num_samples: frames as i32,
        num_inputs: i32::from(ins > 0),
        num_outputs: i32::from(outs > 0),
        inputs: if ins > 0 { &mut in_bus } else { std::ptr::null_mut() },
        outputs: if outs > 0 { &mut out_bus } else { std::ptr::null_mut() },
        input_parameter_changes: if p.changes.is_empty() {
            std::ptr::null_mut()
        } else {
            p.changes.as_raw()
        },
        output_parameter_changes: std::ptr::null_mut(),
        input_events: p.events.as_raw(),
        output_events: std::ptr::null_mut(),
        process_context: p.transport.next_block(frames),
    };
    let proc = &p.processor;
    // SAFETY: every pointer in `data` is live for the call; channels are
    // ROOM >= frames samples (or null, which the format allows).
    let result = unsafe { (proc.vtbl().process)(proc.as_ptr(), &mut data) };
    let output = out_ptrs
        .iter()
        .filter(|ptr| !ptr.is_null())
        // SAFETY: a live channel of ROOM >= frames samples in `memory`.
        .flat_map(|&ptr| unsafe { std::slice::from_raw_parts(ptr, frames) }.to_vec())
        .collect();
    Block { result, output }
}

type Job = Box<dyn FnOnce() + Send>;

/// The audio thread: a real second thread each process call runs on,
/// one at a time (deterministic order, real thread identity).
struct AudioThread {
    jobs: std::sync::mpsc::Sender<Job>,
}

impl AudioThread {
    fn spawn() -> Self {
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("stave-sim-audio".into())
            .spawn(move || {
                for job in rx {
                    job();
                }
            })
            .expect("spawn the audio thread");
        Self { jobs }
    }

    /// Move the plugin to the audio thread, run `f`, and move it back.
    fn run<R: Send + 'static>(
        &self,
        plugin: LoadedPlugin,
        f: impl FnOnce(&mut LoadedPlugin) -> R + Send + 'static,
    ) -> (LoadedPlugin, R) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.jobs
            .send(Box::new(move || {
                let mut plugin = plugin;
                let r = f(&mut plugin);
                let _ = tx.send((plugin, r));
            }))
            .expect("the audio thread is alive");
        rx.recv().expect("the audio thread returns the plugin")
    }

    /// As [`run`](Self::run) for a `frames` block, while `main` runs on
    /// this thread at the same time.
    fn run_with<R>(
        &self,
        plugin: LoadedPlugin,
        frames: usize,
        main: impl FnOnce() -> R,
    ) -> (LoadedPlugin, (Block, R)) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.jobs
            .send(Box::new(move || {
                let mut plugin = plugin;
                let block = process(&mut plugin, frames, lifecycle::BufferShape::Separate, 0, 0);
                let _ = tx.send((plugin, block));
            }))
            .expect("the audio thread is alive");
        let r = main();
        let (plugin, block) = rx.recv().expect("the audio thread returns the plugin");
        (plugin, (block, r))
    }
}
