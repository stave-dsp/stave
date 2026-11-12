//! Telemetry — the display's feed, PACKED: the debug bus has 64 probe
//! slots and the face needs ~150 values, so several quantised values
//! ride in each probe (`crate::pack`, exact in an f64's 53-bit
//! integer range). Out go: each line's energy (48 dB window), every
//! node's live position, what the ear hears (chroma), every memory's
//! attention weight, whether it holds a memory (and a percussive one),
//! how loudly it is recalling, how NOVEL what you play is to it, the
//! onset pulse, and how tonal what you play is. Zero-cost with no
//! bus attached.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};

use super::bank::{Bank, SLOTS};
use super::geometry::Geometry;
use super::listen::CLASSES;
use super::room::{Room, LINES};
use super::space_model::SPACES;
use crate::{pack, FACE_TAPS, TAP_HEARD_AT, TAP_SPACE, TAP_SPACE_INFO, TAP_ATTN, TAP_ENERGY, TAP_FILLED, TAP_HEARING, TAP_NOVELTY, TAP_PERC, TAP_POS, TAP_PULSE, TAP_RECALLING, TAP_TONAL};

pub(super) struct Telemetry {
    bus: Option<Arc<DebugBus>>,
    taps: [Tap; FACE_TAPS.len()],
}

fn db01(x: f32) -> f64 {
    let db = 20.0 * x.max(1e-7).log10();
    f64::from((1.0 + db / 48.0).clamp(0.0, 1.0))
}

impl Telemetry {
    pub(super) fn new() -> Self {
        Self { bus: None, taps: core::array::from_fn(|i| Tap::new(FACE_TAPS[i])) }
    }

    pub(super) fn attach(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    pub(super) fn publish(&self, room: &Room, geometry: &Geometry, bank: &Bank, recalling: f32, pulse: f32, tonal: f32) {
        let Some(bus) = &self.bus else { return };
        let set = |k: usize, v: f64| self.taps[k].set(bus, v);
        for g in 0..LINES / 4 {
            let v: [f64; 4] = core::array::from_fn(|j| db01(room.energy(4 * g + j)));
            set(TAP_ENERGY + g, pack(&v, 13));
        }
        for g in 0..2 * LINES / 4 {
            let v: [f64; 4] = core::array::from_fn(|j| {
                let k = 4 * g + j;
                f64::from(geometry.pos(k / 2)[k % 2])
            });
            set(TAP_POS + g, pack(&v, 13));
        }
        let q = &bank.query().chroma;
        for g in 0..CLASSES / 4 {
            let v: [f64; 4] = core::array::from_fn(|j| f64::from(q[4 * g + j]));
            set(TAP_HEARING + g, pack(&v, 13));
        }
        for g in 0..SLOTS.div_ceil(6) {
            let v: [f64; 6] = core::array::from_fn(|j| {
                let k = 6 * g + j;
                if k < SLOTS { f64::from(bank.weight(k)).sqrt() } else { 0.0 }
            });
            set(TAP_ATTN + g, pack(&v, 8));
        }
        for half in 0..2 {
            let (mut m, mut p) = (0u64, 0u64);
            for b in 0..32 {
                if bank.filled(32 * half + b) {
                    m |= 1 << b;
                }
                if bank.percussive(32 * half + b) {
                    p |= 1 << b;
                }
            }
            set(TAP_FILLED + half, m as f64);
            set(TAP_PERC + half, p as f64);
        }
        set(TAP_PULSE, f64::from(pulse.clamp(0.0, 1.0)));
        set(TAP_TONAL, f64::from(tonal.clamp(0.0, 1.0)));
        set(TAP_RECALLING, db01(recalling));
        set(TAP_NOVELTY, f64::from(bank.novelty()));
    }

    /// The SPACE window's feed: the room on the map, where memory pulls
    /// it, the nearest real space, the decay; every memory's room.
    pub(super) fn publish_space(&self, bank: &Bank, live: [f32; 2], near: usize, pull_at: [f32; 2], pull: f32, t60: f32) {
        let Some(bus) = &self.bus else { return };
        let set = |k: usize, v: f64| self.taps[k].set(bus, v);
        let f = |v: f32| f64::from(v);
        set(TAP_SPACE, pack(&[f(live[0]), f(live[1]), f(pull_at[0]), f(pull_at[1])], 13));
        // T60 on a log scale, 0.1 … 30 s.
        let t = f64::from((t60.max(0.1) / 0.1).ln() / (300f32).ln());
        set(TAP_SPACE_INFO, pack(&[near as f64 / (SPACES - 1).max(1) as f64, f(pull), t], 13));
        for g in 0..(2 * SLOTS).div_ceil(6) {
            let v: [f64; 6] = core::array::from_fn(|j| {
                let k = 6 * g + j;
                if k < 2 * SLOTS { f(bank.heard_at(k / 2)[k % 2]) } else { 0.0 }
            });
            set(TAP_HEARD_AT + g, pack(&v, 8));
        }
    }
}
