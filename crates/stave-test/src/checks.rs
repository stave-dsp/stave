//! The host-behaviour checks every plugin runs in-process (what
//! pluginval and clap-validator probe, without a bundle): block sizes
//! from 0 to past the declared maximum's odd neighbours, sample-rate
//! changes, prepare/reset cycles, seeded random automation (smoothed and
//! sample-accurate), a state round trip that sounds the same, and
//! determinism. Each one is a function over a [`PluginHarness`]; the
//! [`plugin_checks!`](macro@crate::plugin_checks) macro mounts them all.
//!
//! Multi-threaded call interleavings and crash isolation need a real
//! host's threads; they belong to the simulated host (ledger V3).

use stave::params::{AnyParam, ParamId};
use stave::{Event, EventKind, NoteEvent, NoteKind, Plugin};
use stave_measure::signal::{Noise, Signal};

use crate::harness::change;
use crate::PluginHarness;

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

const NOISE: Noise = Noise { seed: 0x5EED, amplitude: 0.5 };

/// What drives the plugin: noise on every input, and for an instrument a
/// note on in the first block.
fn feed<P: Plugin>(h: &mut PluginHarness<P>, start: u64) {
    for (c, ch) in h.inputs_mut().enumerate() {
        for (i, s) in ch.iter_mut().enumerate() {
            *s = NOISE.at(c, start + i as u64);
        }
    }
}

fn notes<P: Plugin>(h: &PluginHarness<P>, block: usize) -> Vec<Event> {
    if h.input_channels() > 0 || block % 8 != 0 {
        return Vec::new();
    }
    let kind = if block % 16 == 0 { NoteKind::On } else { NoteKind::Off };
    vec![Event {
        frame: 0,
        kind: EventKind::Note(NoteEvent { kind, key: 48 + (block / 16 % 24) as i16, channel: 0, velocity: 0.8, note_id: (block / 16) as i32 }),
    }]
}

fn assert_finite<P: Plugin>(h: &PluginHarness<P>, frames: usize, what: &str) {
    for (c, ch) in h.outputs().iter().enumerate() {
        if let Some(i) = ch[..frames].iter().position(|y| !y.is_finite() || y.abs() > 1e3) {
            panic!("{}: {what}: output channel {c} frame {i} is {}", P::INFO.name, ch[i]);
        }
    }
}

/// Blocks of 0, 1, odd and full sizes in a shuffled order, and a
/// re-prepare to a larger maximum: output stays finite.
pub fn block_sizes<P: Plugin>() {
    let mut h = PluginHarness::<P>::builder().block_size(1031).build();
    let mut frame = 0u64;
    for (b, n) in [0usize, 1, 7, 64, 0, 1031, 13, 512, 3, 1000, 1].into_iter().enumerate() {
        feed(&mut h, frame);
        let ev = notes(&h, b * 8);
        h.process_frames(n, &[], &ev);
        assert_finite(&h, n, &format!("a {n}-frame block"));
        frame += n as u64;
    }
    h.prepare(48_000.0, 4096);
    feed(&mut h, frame);
    h.process(&[]);
    assert_finite(&h, 4096, "after a re-prepare to 4096-frame blocks");
}

/// Prepared at every common rate (and the extremes hosts use), in a
/// changing order: output stays finite.
pub fn sample_rates<P: Plugin>() {
    let mut h = PluginHarness::<P>::new(44_100.0, 256);
    for rate in [22_050.0, 192_000.0, 44_100.0, 8_000.0, 96_000.0, 384_000.0, 48_000.0, 88_200.0, 768_000.0, 11_025.0] {
        h.prepare(rate, 256);
        for b in 0..16 {
            feed(&mut h, b as u64 * 256);
            let ev = notes(&h, b);
            h.process(&ev);
            assert_finite(&h, 256, &format!("at {rate} Hz"));
        }
    }
}

/// Prepare, reset and process interleaved as hosts do on activation
/// toggles; after a reset the plugin sounds as it did fresh.
pub fn prepare_and_reset_cycles<P: Plugin>() {
    let blocks = |h: &mut PluginHarness<P>| -> Vec<f32> {
        let mut out = Vec::new();
        for b in 0..8 {
            feed(h, b as u64 * 128);
            let ev = notes(h, b * 8);
            h.process(&ev);
            out.extend_from_slice(&h.outputs()[0]);
        }
        out
    };
    let mut fresh = PluginHarness::<P>::new(48_000.0, 128);
    let first = blocks(&mut fresh);
    let mut h = PluginHarness::<P>::new(48_000.0, 128);
    for k in 0..6 {
        let _ = blocks(&mut h);
        match k % 3 {
            0 => h.reset(),
            1 => h.prepare(96_000.0, 64),
            _ => h.prepare(48_000.0, 128),
        }
    }
    h.prepare(48_000.0, 128);
    h.reset();
    let again = blocks(&mut h);
    assert!(again.iter().all(|y| y.is_finite()), "{}: non-finite output after prepare/reset cycles", P::INFO.name);
    let same = first.iter().zip(&again).all(|(a, b)| (a - b).abs() <= 1e-5 * (1.0 + a.abs()));
    assert!(same, "{}: after prepare + reset the plugin does not sound as it did fresh", P::INFO.name);
}

/// Every parameter moved at random every block — through the table (a
/// smoothed edit) and as sample-accurate changes — for each seed.
pub fn random_automation<P: Plugin>(seeds: core::ops::Range<u64>) {
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut h = PluginHarness::<P>::new(48_000.0, 256);
        let table = h.table();
        let ids: Vec<ParamId> = (0..table.count()).filter_map(|i| table.by_index(i)).map(|p| p.meta().id).collect();
        for b in 0..64 {
            let mut changes = Vec::new();
            for &id in &ids {
                let p: &dyn AnyParam = table.by_id(id).expect("listed");
                match rng.unit() {
                    u if u < 0.3 => p.set_value(p.denormalize(rng.unit())),
                    u if u < 0.6 => {
                        let mut at: Vec<usize> = (0..3).map(|_| (rng.unit() * 256.0) as usize).collect();
                        at.sort_unstable();
                        changes.extend(at.into_iter().map(|f| change(id, f, p.denormalize(rng.unit()))));
                    }
                    _ => {}
                }
            }
            changes.sort_by_key(|c| c.frame);
            feed(&mut h, b as u64 * 256);
            let ev = notes(&h, b);
            h.process_with(&changes, &ev);
            assert_finite(&h, 256, &format!("random automation, seed {seed}, block {b}"));
        }
    }
}

/// Random values saved, loaded into a fresh instance: every value comes
/// back, and both sound the same from there.
pub fn state_round_trip<P: Plugin>() {
    let mut rng = Rng::new(7);
    let mut a = PluginHarness::<P>::new(48_000.0, 256);
    let table = a.table();
    for p in (0..table.count()).filter_map(|i| table.by_index(i)) {
        p.restore(p.denormalize(rng.unit()));
    }
    let bytes = a.save();
    let mut b = PluginHarness::<P>::new(48_000.0, 256);
    b.load(&bytes).unwrap_or_else(|e| panic!("{}: its own state does not load: {e}", P::INFO.name));
    let tb = b.table();
    for p in (0..table.count()).filter_map(|i| table.by_index(i)) {
        let q = tb.by_id(p.meta().id).expect("same table");
        assert!((p.value() - q.value()).abs() <= 1e-9 * (1.0 + p.value().abs()), "{}: {} does not survive a save and load", P::INFO.name, p.meta().name);
    }
    a.reset();
    b.reset();
    for blk in 0..8 {
        feed(&mut a, blk * 256);
        feed(&mut b, blk * 256);
        let ev = notes(&a, blk as usize);
        a.process(&ev);
        b.process(&ev);
        assert_eq!(a.outputs(), b.outputs(), "{}: a loaded state sounds different, block {blk}", P::INFO.name);
    }
}

/// Two fresh instances fed the same input give the same output, bit for
/// bit (no hidden global state, no uninitialised reads).
pub fn deterministic<P: Plugin>() {
    let mut a = PluginHarness::<P>::new(48_000.0, 256);
    let mut b = PluginHarness::<P>::new(48_000.0, 256);
    for blk in 0..16 {
        feed(&mut a, blk * 256);
        feed(&mut b, blk * 256);
        let ev = notes(&a, blk as usize);
        a.process(&ev);
        b.process(&ev);
        assert_eq!(a.outputs(), b.outputs(), "{}: two fresh instances differ at block {blk}", P::INFO.name);
    }
}
