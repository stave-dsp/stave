//! Test doubles: pure-Rust [`Instance`]s with exactly known behavior,
//! so every measurement and check has an answer to be tested against.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::control::{find, test_control, Control, ControlError};
use crate::instance::{Instance, Layout, StateError, Tail};
use crate::note::{NoteEvent, NoteKind};

pub const RATE: f64 = 48_000.0;
pub const BLOCK: usize = 64;

pub fn layout(inputs: usize, outputs: usize) -> Layout {
    Layout {
        inputs,
        outputs,
        sample_rate: RATE,
        max_block: BLOCK,
    }
}

/// What every double shares: controls, state as control values, and
/// presets behind a program-change control.
#[derive(Clone)]
struct Surface {
    controls: Vec<Control>,
    presets: Vec<Vec<(String, f64)>>,
}

impl Surface {
    fn get(&self, name: &str) -> f64 {
        find(&self.controls, name).map_or(0.0, |c| c.value)
    }
    fn set(&mut self, name: &str, v: f64) -> Result<(), ControlError> {
        let id = find(&self.controls, name)
            .ok_or_else(|| ControlError::NotFound(name.into()))?
            .id;
        let c = self.controls.iter_mut().find(|c| c.id == id).unwrap();
        if c.flags.read_only {
            return Err(ControlError::ReadOnly(name.into()));
        }
        c.value = v.clamp(c.min, c.max);
        if c.flags.program_change {
            for (name, value) in self.presets[c.value as usize].clone() {
                self.set(&name, value)?;
            }
        }
        Ok(())
    }
    fn save(&self) -> Vec<u8> {
        self.controls
            .iter()
            .flat_map(|c| c.value.to_le_bytes())
            .collect()
    }
    fn load(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        if bytes.len() != self.controls.len() * 8 {
            return Err(StateError::Unreadable { bytes: bytes.len() });
        }
        for (c, chunk) in self.controls.iter_mut().zip(bytes.chunks_exact(8)) {
            c.value = f64::from_le_bytes(chunk.try_into().unwrap());
        }
        Ok(())
    }
}

/// out[c] = in[c] × gain (bypass → in). Controls: `gain` [0, max],
/// gliding 5 ms to each new value after the first block, and `bypass`
/// (flagged). `leaky_bypass` keeps applying gain when bypassed.
#[derive(Clone)]
pub struct Gain {
    layout: Layout,
    surface: Surface,
    gain: Option<f32>,
    pub leaky_bypass: bool,
}

impl Gain {
    pub fn new(inputs: usize, outputs: usize) -> Self {
        Self::with_max(inputs, outputs, 4.0)
    }

    pub fn with_max(inputs: usize, outputs: usize, max_gain: f64) -> Self {
        let mut gain = test_control("Level · gain", 0.0, max_gain);
        gain.id = 1;
        (gain.default, gain.value) = (1.0, 1.0);
        let mut bypass = test_control("bypass", 0.0, 1.0);
        (bypass.id, bypass.steps, bypass.flags.bypass) = (2, 1, true);
        Self {
            layout: layout(inputs, outputs),
            surface: Surface {
                controls: vec![gain, bypass],
                presets: vec![],
            },
            gain: None,
            leaky_bypass: false,
        }
    }

    pub fn with_preset(mut self, name: &str, values: &[(&str, f64)]) -> Self {
        let s = &mut self.surface;
        s.presets
            .push(values.iter().map(|(n, v)| (n.to_string(), *v)).collect());
        if !s.controls.iter().any(|c| c.flags.program_change) {
            let mut program = test_control("Program", 0.0, 0.0);
            (program.id, program.flags.program_change) = (3, true);
            s.controls.push(program);
        }
        let program = s
            .controls
            .iter_mut()
            .find(|c| c.flags.program_change)
            .unwrap();
        program.step_names.push(name.into());
        program.steps = program.step_names.len() as u32 - 1;
        program.max = f64::from(program.steps);
        self
    }
}

impl Instance for Gain {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        let target = self.surface.get("gain") as f32;
        let bypassed = self.surface.get("bypass") >= 0.5 && !self.leaky_bypass;
        let k = 1.0 - (-1.0 / (0.005 * self.layout.sample_rate)).exp() as f32;
        let frames = outputs.first().map_or(0, |o| o.len());
        for n in 0..frames {
            let gain = self.gain.map_or(target, |g| g + k * (target - g));
            self.gain = Some(gain);
            let g = if bypassed { 1.0 } else { gain };
            for (c, out) in outputs.iter_mut().enumerate() {
                out[n] = inputs
                    .get(c)
                    .or(inputs.last())
                    .map_or(0.0, |inp| inp[n] * g);
            }
        }
    }
    fn controls(&self) -> Vec<Control> {
        self.surface.controls.clone()
    }
    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.surface.set(name, plain)
    }
    fn save_state(&mut self) -> Result<Vec<u8>, StateError> {
        Ok(self.surface.save())
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        self.surface.load(bytes)
    }
}

/// A one-pole feedback loop, y = x + feedback·y₋₁ per channel: stable
/// below `feedback` 1, growing without bound above it. Control:
/// `feedback` [0, 1.1].
pub struct Feedback {
    layout: Layout,
    surface: Surface,
    state: Vec<f32>,
}

impl Feedback {
    pub fn new(channels: usize) -> Self {
        let mut feedback = test_control("feedback", 0.0, 1.1);
        feedback.id = 1;
        Self {
            layout: layout(channels, channels),
            surface: Surface {
                controls: vec![feedback],
                presets: vec![],
            },
            state: vec![0.0; channels],
        }
    }
}

impl Instance for Feedback {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        let feedback = self.surface.get("feedback") as f32;
        for ((out, inp), y) in outputs.iter_mut().zip(inputs).zip(&mut self.state) {
            for (o, &x) in out.iter_mut().zip(*inp) {
                *y = x + feedback * *y;
                *o = *y;
            }
        }
    }
    fn controls(&self) -> Vec<Control> {
        self.surface.controls.clone()
    }
    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.surface.set(name, plain)
    }
}

/// A pure delay of `frames` (per channel), reporting `reported` latency.
pub struct Delay {
    layout: Layout,
    ring: Vec<Vec<f32>>,
    pos: usize,
    reported: u32,
}

impl Delay {
    pub fn new(channels: usize, frames: usize) -> Self {
        Self::misreporting(channels, frames, frames as u32)
    }
    pub fn misreporting(channels: usize, frames: usize, reported: u32) -> Self {
        Self {
            layout: layout(channels, channels),
            ring: vec![vec![0.0; frames.max(1)]; channels],
            pos: 0,
            reported,
        }
    }
}

impl Instance for Delay {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn latency(&self) -> u32 {
        self.reported
    }
    fn tail(&self) -> Tail {
        Tail::Samples(self.ring[0].len() as u32)
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        let n = outputs[0].len();
        let len = self.ring[0].len();
        for c in 0..outputs.len() {
            let mut p = self.pos;
            for i in 0..n {
                outputs[c][i] = self.ring[c][p];
                self.ring[c][p] = inputs[c][i];
                p = (p + 1) % len;
            }
        }
        self.pos = (self.pos + n) % len;
    }
}

/// Passes input through, then emits NaN from frame `after` on.
pub struct NanAfter {
    layout: Layout,
    after: u64,
    frame: u64,
}

impl NanAfter {
    pub fn new(after: u64) -> Self {
        Self {
            layout: layout(1, 1),
            after,
            frame: 0,
        }
    }
}

impl Instance for NanAfter {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        for (i, o) in outputs[0].iter_mut().enumerate() {
            *o = if self.frame + i as u64 >= self.after {
                f32::NAN
            } else {
                inputs[0][i]
            };
        }
        self.frame += outputs[0].len() as u64;
    }
}

/// Adds a per-INSTANCE offset: two fresh instances never agree
/// (a reproducibility failure), one instance is self-consistent.
pub struct Drift {
    layout: Layout,
    offset: f32,
}

impl Drift {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let k = NEXT.fetch_add(1, Ordering::Relaxed);
        Self {
            layout: layout(1, 1),
            offset: k as f32 * 1e-3,
        }
    }
}

impl Instance for Drift {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], _: &[NoteEvent]) {
        for (o, i) in outputs[0].iter_mut().zip(inputs[0]) {
            *o = i + self.offset;
        }
    }
}

/// An instrument: a unit-amplitude square-ish tone while any note is
/// held, silence otherwise. `bypass` silences it.
pub struct Synth {
    layout: Layout,
    held: Vec<i16>,
    surface: Surface,
    phase: u32,
}

impl Synth {
    pub fn new() -> Self {
        let mut bypass = test_control("bypass", 0.0, 1.0);
        (bypass.id, bypass.flags.bypass) = (1, true);
        Self {
            layout: layout(0, 2),
            held: vec![],
            surface: Surface {
                controls: vec![bypass],
                presets: vec![],
            },
            phase: 0,
        }
    }
}

impl Instance for Synth {
    fn layout(&self) -> Layout {
        self.layout
    }
    fn process(&mut self, _: &[&[f32]], outputs: &mut [&mut [f32]], notes: &[NoteEvent]) {
        let n = outputs[0].len();
        let mut next = 0;
        for i in 0..n {
            while let Some(e) = notes.get(next).filter(|e| e.offset as usize == i) {
                match e.kind {
                    NoteKind::On => self.held.push(e.pitch),
                    NoteKind::Off => self.held.retain(|&p| p != e.pitch),
                }
                next += 1;
            }
            self.phase = self.phase.wrapping_add(1);
            let on = !self.held.is_empty() && self.surface.get("bypass") < 0.5;
            let v = if !on {
                0.0
            } else if self.phase % 48 < 24 {
                0.5
            } else {
                -0.5
            };
            for out in outputs.iter_mut() {
                out[i] = v;
            }
        }
    }
    fn controls(&self) -> Vec<Control> {
        self.surface.controls.clone()
    }
    fn set_control(&mut self, name: &str, plain: f64) -> Result<(), ControlError> {
        self.surface.set(name, plain)
    }
}

/// A uniquely named scratch directory, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "stave-host-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
