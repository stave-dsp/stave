//! The no-alloc checks as a library: every project's colocated
//! no_alloc test drives its full process loop under an
//! allocation-panicking guard while `wiggle` params sweep per block.
//! The TEST BINARY must install `assert_no_alloc::AllocDisabler` as
//! its global allocator — the guard cannot fire otherwise.

use assert_no_alloc::assert_no_alloc;
use stave_core::params::AnyParam;
use stave_core::{Event, EventKind, NoteEvent, NoteKind, Plugin, Telemetry};
use stave_measure::signal::{Noise, Signal, Sine};

use crate::PluginHarness;

#[derive(Clone, Debug)]
pub struct NoAllocConfig {
    pub sample_rate: f32,
    pub block_size: usize,
    /// Outside the guard: first-block registrations, initial structural
    /// builds, ring priming.
    pub warmup_blocks: usize,
    /// Inside the guard, with every wiggled param moving each block.
    pub guarded_blocks: usize,
    /// Effects hear noise plus a tone on every input channel.
    pub noise: Noise,
    pub tone_hz: f64,
    pub tone_amplitude: f64,
    /// Instruments run under a NOTE STORM: each block a new note on and
    /// the note from `storm_release_after` blocks ago off, cycling keys
    /// past any sane voice pool, or the steal path never runs under the
    /// guard.
    pub storm_release_after: usize,
    pub velocity: f64,
}

impl Default for NoAllocConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_size: 256,
            warmup_blocks: 32,
            guarded_blocks: 64,
            noise: Noise {
                seed: 0x2545_F491,
                amplitude: 0.4,
            },
            tone_hz: 100.0,
            tone_amplitude: 0.3,
            storm_release_after: 3,
            velocity: 0.9,
        }
    }
}

/// Keys the storm cycles through: a fifth apart, wrapping over four
/// octaves from C2.
fn storm_key(block: usize) -> i16 {
    36 + (block as i16 * 7) % 48
}

pub fn check_no_alloc<P: Plugin>(name: &str, wiggle: &[&str], config: &NoAllocConfig) {
    let block = config.block_size;
    let total = config.warmup_blocks + config.guarded_blocks;
    let mut h = PluginHarness::<P>::new(config.sample_rate, block);
    h.plugin_mut().attach_telemetry(Telemetry::new());
    let is_instrument = h.input_channels() == 0;

    let note = |kind, b: usize| Event {
        frame: 0,
        kind: EventKind::Note(NoteEvent {
            kind,
            key: storm_key(b),
            channel: 0,
            velocity: config.velocity,
            note_id: b as i32,
        }),
    };
    // Built outside the guard, replayed inside.
    let storm: Vec<Vec<Event>> = (0..total)
        .map(
            |b| match (is_instrument, b.checked_sub(config.storm_release_after)) {
                (false, _) => vec![],
                (true, None) => vec![note(NoteKind::On, b)],
                (true, Some(old)) => vec![note(NoteKind::On, b), note(NoteKind::Off, old)],
            },
        )
        .collect();
    let tone = Sine {
        frequency_hz: config.tone_hz,
        amplitude: config.tone_amplitude,
        sample_rate: f64::from(config.sample_rate),
    };
    let input: Vec<f32> = (0..(total * block) as u64)
        .map(|f| config.noise.at(0, f) + tone.at(0, f))
        .collect();

    let table = h.plugin().param_table();
    let params: Vec<&dyn AnyParam> = wiggle
        .iter()
        .map(|n| {
            table
                .by_name(n)
                .unwrap_or_else(|| panic!("{name}: param '{n}' unknown"))
        })
        .collect();
    let run_block = |h: &mut PluginHarness<P>, b: usize| {
        for ch in h.inputs_mut() {
            ch.copy_from_slice(&input[b * block..(b + 1) * block]);
        }
        h.run_block(&storm[b]);
        assert!(
            h.outputs().iter().all(|ch| ch[block - 1].is_finite()),
            "{name}: non-finite output"
        );
    };

    for b in 0..config.warmup_blocks {
        run_block(&mut h, b);
    }
    assert_no_alloc(|| {
        for b in 0..config.guarded_blocks {
            // Each param on its own sine through its full range.
            for (k, p) in params.iter().enumerate() {
                let t = (b as f64 * 0.13 + k as f64 * 0.37).sin() * 0.5 + 0.5;
                p.set_plain(p.normalized_to_plain(t));
            }
            run_block(&mut h, config.warmup_blocks + b);
        }
    });
}
