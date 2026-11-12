//! The host lifecycle (VST3's terms; CLAP's activate/start_processing
//! map onto it): which calls a host may make in which state, what the
//! stave wrapper answers to each — legal or not — and what a block must
//! sound like. The simulated host (stave-host `plugin::sim`) drives a
//! real plugin with these calls and checks every answer against
//! [`Lifecycle::expect`]; [`crate::check`] explores the model itself
//! exhaustively.

/// One host → plugin call, as the simulated host makes it.
#[derive(Clone, Debug, PartialEq)]
pub enum HostCall {
    /// `setupProcessing`: legal only while inactive.
    Setup { sample_rate: f64, max_block: usize },
    /// `setActive`: legal as a toggle (on while off, off while on).
    SetActive(bool),
    /// `setProcessing`: legal only while active.
    SetProcessing(bool),
    /// `process`: legal while processing, `frames <= max_block`, with
    /// separate or in-place buffers.
    Process {
        frames: usize,
        shape: BufferShape,
        /// Parameter points in the block (any order, some outside it).
        changes: usize,
        change_seed: u64,
    },
    /// `setParamNormalized` on the controller (any time).
    SetParam { index: usize, normalized: f64 },
    /// `getState` (any time).
    SaveState,
    /// `setState` with a state the plugin saved earlier.
    LoadSaved(usize),
    /// `setState` with random bytes.
    LoadGarbage { seed: u64, len: usize },
    /// `setState` with a saved state cut short.
    LoadTruncated { slot: usize, len: usize },
    /// `getState` on the main thread during a `process` on the audio
    /// thread (a host saving a project during playback).
    SaveDuringProcess { frames: usize },
}

/// How a block's channel lists are laid out.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BufferShape {
    /// Inputs and outputs in separate memory.
    Separate,
    /// Inputs point at the outputs (Live, among others).
    InPlace,
    /// One input channel pointer is null (outside the contract).
    NullInput,
    /// One output channel pointer is null (outside the contract).
    NullOutput,
}

impl BufferShape {
    pub const ALL: [Self; 4] = [Self::Separate, Self::InPlace, Self::NullInput, Self::NullOutput];

    /// Within the format's contract.
    pub fn legal(self) -> bool {
        matches!(self, Self::Separate | Self::InPlace)
    }
}

/// What a block must do.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockExpect {
    /// Not a process call.
    NotABlock,
    /// Zero frames: success, nothing written.
    Empty,
    /// More frames than the declared maximum: refused
    /// (`kInvalidArgument`), the plugin untouched.
    Refused,
    /// Not active: success, and the outputs are silence (the plugin
    /// never runs unprepared).
    Silent,
    /// The plugin runs: success, finite output.
    Ran,
}

/// The wrapper's answer the model predicts for a call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Expect {
    /// The call returns `kResultOk` (calls with no fixed answer — state
    /// loads — say `false` and are checked by their own laws).
    pub result_ok: bool,
    pub block: BlockExpect,
}

/// The lifecycle state a host has driven a plugin into.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Lifecycle {
    /// The last setup's values: `max_block` bounds blocks at once (the
    /// wrapper refuses longer ones immediately), the sample rate takes
    /// effect at the next activation.
    pub sample_rate: f64,
    pub max_block: usize,
    pub active: bool,
    pub processing: bool,
    /// Something that changes saved state happened since the last save.
    pub dirty: bool,
}

impl Lifecycle {
    /// What `LoadedPlugin` leaves behind: set up, active, processing.
    pub fn active(sample_rate: f64, max_block: usize) -> Self {
        Self {
            sample_rate,
            max_block,
            active: true,
            processing: true,
            dirty: true,
        }
    }

    /// A freshly created instance.
    pub fn created() -> Self {
        Self {
            sample_rate: 44_100.0,
            max_block: 0,
            active: false,
            processing: false,
            dirty: true,
        }
    }

    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// Whether `call` is within the format's contract now.
    pub fn allows(&self, call: &HostCall) -> bool {
        match *call {
            HostCall::Setup { .. } => !self.active,
            HostCall::SetActive(on) => on != self.active,
            HostCall::SetProcessing(on) => self.active && on != self.processing,
            HostCall::Process { frames, shape, .. } => {
                self.processing && frames <= self.max_block && shape.legal()
            }
            HostCall::SaveDuringProcess { frames } => self.processing && frames <= self.max_block,
            HostCall::SetParam { .. }
            | HostCall::SaveState
            | HostCall::LoadSaved(_)
            | HostCall::LoadGarbage { .. }
            | HostCall::LoadTruncated { .. } => true,
        }
    }

    /// What the wrapper answers, legal call or not.
    pub fn expect(&self, call: &HostCall) -> Expect {
        let block = |frames: usize| {
            if frames == 0 {
                BlockExpect::Empty
            } else if self.max_block > 0 && frames > self.max_block {
                BlockExpect::Refused
            } else if !self.active {
                BlockExpect::Silent
            } else {
                BlockExpect::Ran
            }
        };
        match *call {
            HostCall::Setup { .. } | HostCall::SetActive(_) | HostCall::SetProcessing(_) => Expect {
                result_ok: true,
                block: BlockExpect::NotABlock,
            },
            HostCall::Process { frames, .. } | HostCall::SaveDuringProcess { frames } => Expect {
                result_ok: block(frames) != BlockExpect::Refused,
                block: block(frames),
            },
            HostCall::SetParam { .. }
            | HostCall::SaveState
            | HostCall::LoadSaved(_)
            | HostCall::LoadGarbage { .. }
            | HostCall::LoadTruncated { .. } => Expect {
                result_ok: false,
                block: BlockExpect::NotABlock,
            },
        }
    }

    /// The state after `call`.
    pub fn apply(&mut self, call: &HostCall) {
        match *call {
            HostCall::Setup {
                sample_rate,
                max_block,
            } => {
                self.sample_rate = sample_rate;
                self.max_block = max_block;
            }
            HostCall::SetActive(on) => {
                self.active = on;
                if !on {
                    self.processing = false;
                }
            }
            HostCall::SetProcessing(on) => self.processing = on && self.active,
            HostCall::Process { changes, .. } => self.dirty |= changes > 0,
            HostCall::SetParam { .. }
            | HostCall::LoadSaved(_)
            | HostCall::LoadGarbage { .. }
            | HostCall::LoadTruncated { .. }
            | HostCall::SaveDuringProcess { .. } => self.dirty = true,
            HostCall::SaveState => self.dirty = false,
        }
    }

    /// Two saves now and at the last save must be byte-identical.
    pub fn state_unchanged_since_save(&self) -> bool {
        !self.dirty
    }
}
