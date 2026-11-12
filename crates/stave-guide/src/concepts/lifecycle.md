# The plugin lifecycle

A plugin is one trait, [`Plugin`](stave::Plugin), and the framework
calls it in a fixed order: `new` once, `prepare` whenever the host's
conditions change, then `process` once per block on the audio thread,
with `reset` when the host asks for a clean start. Everything around
those calls (bypass, state, presets, denormals, panics and the account
of what went wrong) is the framework's, not the plugin's.

## The trait

Only `INFO`, `Params`, `new` and `process` are required; everything else
has a default.

| Item | Default | Called |
|---|---|---|
| `const INFO: Info` | none | read by hosts before any instance exists |
| `const LAYOUTS` | `&[Layout::STEREO]` | read when the host negotiates buses |
| `const STATE_VERSION` | `0` | written into every saved state |
| `type Params` | none | built by its `Default`, once per instance |
| `new(params, telemetry)` | none | once per instance, off the audio thread |
| `prepare(env)` | does nothing | before processing, and whenever the conditions change |
| `reset()` | does nothing | when the host asks for a clean start |
| `process(block)` | none | once per block, on the audio thread |
| `latency()` | `0` | after each `prepare` |
| `tail()` | `Tail::None` | after each `prepare` |
| `face(params)` | `None` | when a face opens |
| `presets(params)` | no presets | once per instance |
| `migrations()` | no migrations | when loading state |
| `save()` / `load(bytes, version)` | no extra state | when the host saves or loads a session |

**Declarations are data.** `INFO`, `LAYOUTS`, the params' `Default`,
`face` and `presets` are pure: no behaviour and no state. The plugin
value is the realtime half: what `new` builds, `prepare` sizes and
`process` runs.

## `new`: build everything

The framework builds `Self::Params` with its `Default`, wraps it in an
`Arc` shared with the host and the face, and calls `new` with it and the
instance's [`Telemetry`](stave::Telemetry). Build the plugin's running
state here: a [`Smoother`](stave::params::Smoother) or
[`Steps`](stave::params::Steps) per param, the `#[derive(Taps)]` table
on the telemetry, filters and buffers. `new` may allocate.

## `prepare`: derive from the conditions

[`Environment`](stave::Environment) is the host's conditions: the
`sample_rate` (Hz, `f32`), `max_block` (the most frames any one `process`
call will carry) and the bus `layout` the host chose. Every coefficient
that depends on them is derived here, from this one value, so nothing in
the plugin can run against a stale rate. `prepare` may allocate, and it
runs again whenever the conditions change (a new sample rate, a larger
block, another layout).

When a host switches a plugin on (VST3 `setActive`, and its CLAP
counterpart), the wrapper calls `prepare` and then `reset`, so a fresh
start never rings with stale state; then it reads `latency()` and
`tail()` and reports them to the host. Report latency as it is at the
conditions just prepared; when it changes from one preparation to the
next, the VST3 wrapper asks the host to re-read it.

## `process`: one block, in real time

`process` gets a [`Block`](stave::Block): the block's input and output
audio, its timed parameter changes and events, the transport, and any
aux (sidechain) inputs. It runs on the audio thread under a deadline,
so it never allocates, locks or blocks; `stave_test::plugin_checks!`
proves the first by running `process` under an allocator that aborts.

`block.lanes::<MAX>().run(|frame| ...)` owns the per-frame plumbing: it
decides how many lanes are live (the smaller of the inputs, the outputs
and `MAX` for an effect; the outputs for an instrument, whose inputs read
silence), zeroes outputs it does not drive before the body runs, and
returns the block's input and output [`Peaks`](stave::Peaks). The plugin
keeps the decision of what to do at each lane count, and the DSP.

A useful shape for `process` is **bind, process, observe**: block-rate
decisions first (ramps, steps, watched rebuilds), then the per-frame
loop, then publishing to taps.

## `reset`: forget the past

`reset` clears DSP state (delay lines, filters, envelopes) and leaves
params alone. It is realtime. Besides following every `prepare`, it runs
when a CLAP host asks for a reset (a transport jump, for instance).

## Latency and tail

`latency()` is the samples of delay the host compensates for: a
lookahead limiter, a linear-phase filter. `tail()` says how long output
rings after the input stops, as a [`Tail`](stave::Tail): `None` (output
stops with input), `Samples(n)` (below −120 dB within `n` samples), or
`Infinite` (generators, unbounded feedback). Hosts use the tail to decide
when they may stop calling a silent plugin.

## State: save, load, migrate

The framework saves every param itself, as plain values keyed by
[`ParamId`](stave::ParamId), with `STATE_VERSION`. `save` and `load` are
for state that is not a param (rare: a captured sample, a learned
curve); `load` receives the bytes and the version they were written at,
after the params are restored. Loading is all or nothing: a malformed
chunk is an error and changes nothing. Ids and sections a newer version
wrote are kept and written back out, so opening and re-saving a session
in an older version loses nothing. Old sessions reach new params through
`migrations()`; see [Parameters and smoothing](crate::concepts::params).

State calls happen off the audio thread. The wrapper waits up to 50 ms
for the block in flight to finish before it reaches the plugin's
`save`/`load`; a save that cannot wait drops only the extra state (and
logs it), never the params.

The whole order, driven in-process through
[`PluginHarness`](stave_test::PluginHarness) (which builds and prepares
the plugin like a wrapper, without the bypass gate or a `reset` after
`prepare`):

```
use std::sync::atomic::{AtomicU32, Ordering};
use stave::prelude::*;
use stave_test::PluginHarness;

#[derive(Params)]
pub struct CounterParams {
    pub level: FloatParam,
}
# impl Default for CounterParams {
#     fn default() -> Self {
#         CounterParams { level: FloatParam::new(ParamId(1), "Level", Range::linear(0.0, 1.0)) }
#     }
# }

/// Counts the samples it has seen; keeps the count as extra state.
pub struct Counter {
    seen: u64,
    rate: f32,
}

static NEW_CALLS: AtomicU32 = AtomicU32::new(0);

impl Plugin for Counter {
    const INFO: Info = Info { id: "com.example.counter", name: "Counter", vendor: "Example", ..Info::EFFECT };
    type Params = CounterParams;

    fn new(_params: &Arc<CounterParams>, _telemetry: &Telemetry) -> Self {
        NEW_CALLS.fetch_add(1, Ordering::Relaxed);
        Counter { seen: 0, rate: 0.0 }
    }
    fn prepare(&mut self, env: &Environment) {
        self.rate = env.sample_rate;
    }
    fn reset(&mut self) {
        self.seen = 0;
    }
    fn process(&mut self, block: &mut Block<'_>) {
        self.seen += block.frames() as u64;
    }
    fn latency(&self) -> u32 {
        64
    }
    fn tail(&self) -> Tail {
        Tail::Samples(self.rate as u32) // one second
    }
    fn save(&self) -> Vec<u8> {
        self.seen.to_le_bytes().to_vec()
    }
    fn load(&mut self, bytes: &[u8], _version: u32) {
        self.seen = u64::from_le_bytes(bytes.try_into().unwrap());
    }
}

let mut h = PluginHarness::<Counter>::new(48_000.0, 128); // new, then prepare
assert_eq!(NEW_CALLS.load(Ordering::Relaxed), 1);
assert_eq!(h.plugin().rate, 48_000.0);
assert_eq!((h.latency(), h.tail()), (64, Tail::Samples(48_000)));

h.process(&[]);
h.process_frames(0, &[], &[]); // hosts send empty blocks too
assert_eq!(h.plugin().seen, 128);

let saved = h.save();
h.reset();
assert_eq!(h.plugin().seen, 0);
h.load(&saved).unwrap();
assert_eq!(h.plugin().seen, 128);
assert!(h.load(b"not a state chunk").is_err()); // and nothing changed
assert_eq!(h.plugin().seen, 128);

h.prepare(96_000.0, 512); // the host's conditions changed
assert_eq!(h.tail(), Tail::Samples(96_000));
```

## Threads

A host calls a plugin from more than one thread: `process` on the audio
thread, everything else on its main thread (or another of its
choosing), and parameter writes from wherever the automation or the
face happens to run. The framework turns that into rules a plugin can
rely on:

- **The plugin value has one owner at a time.** The wrapper reaches the
  plugin only through an atomic claim, so `process`, `prepare`, `reset`,
  `save` and `load` never overlap; a host that tries to overlap them gets
  a refused call, logged, rather than a data race.
- **Params are shared, lock-free.** A param's value is an atomic: hosts,
  the face and state restores write it from any thread, and `process`
  reads it without waiting. That is why `process` reads params through
  `value()` and smoothers instead of keeping copies.
- **In-place hosts are handled.** When a host passes the same memory as
  input and output, the wrapper copies the input to preallocated scratch
  first, so a block's input and output never alias.

## What the framework owns

- **Bypass.** A param declared with
  [`BoolParam::bypass`](stave::params::BoolParam::bypass) is the host's
  bypass switch (VST3 `kIsBypass`, CLAP `is_bypass`). The plugin never
  reads it: the wrapper keeps a copy of the input delayed by the plugin's
  latency and crossfades to it over 10 ms with an equal-power curve, so
  bypass is click-free and time-aligned. A bypassed instrument plays
  silence. Bypass stays engaged through a re-`prepare`.
- **Denormals.** Every block runs with the CPU's flush-to-zero mode on
  (x86_64 and aarch64), a backstop behind the DSP library's own flushes,
  so recursive state that decays toward zero never hits the slow path for
  denormal numbers.
- **Panics.** A panic never reaches the host. Every entry point that runs
  plugin code is wrapped in `catch_unwind`; the first panic disables the
  instance, which from then on outputs silence and refuses every call
  without touching the plugin's state again. A panic on the audio thread
  is only recorded there (no lock, no allocation, no file); the next
  main-thread call writes it to the log.
- **Diagnostics.** Every call the wrapper refuses is logged as
  `"<call>: <error>"` before the host gets the format's failure code. The
  audio thread only counts what it could not do (for instance a block longer than
  declared, parameter changes or events past the wrapper's capacity, a
  block before `prepare`, a second concurrent `process`), and the next
  main-thread call reports the counts. The log is the plugin's file,
  `<log dir>/<plugin id>.log`: the directory is `$STAVE_LOG_DIR` when
  set (cargo sets it to `target/logs` for anything it runs), else
  `~/Library/Logs/stave` on macOS, `%LOCALAPPDATA%\stave\logs` on
  Windows and `$XDG_STATE_HOME/stave` (or `~/.local/state/stave`) on
  Linux. Messages also reach the instance's telemetry, where stave-host
  reads them.
- **Presets.** `presets(params)` is the host's program list and the
  face's preset browser.
- **The face's window.** `face(params)` is a declaration; the wrapper
  opens it in the host's window and turns its edits into host gestures.
