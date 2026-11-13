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
    /// `setActive`: legal as a toggle (on while off, off while on), on
    /// only after a setup.
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
    /// A block racing a state save on another thread: the plugin runs,
    /// or — the save holding it when the block arrives — the block is
    /// dropped: silence and a `ConcurrentProcess` fault. Success either
    /// way (the host did nothing wrong); which one is thread timing.
    Contended,
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
            // Activation needs a setup first (VST3: setupProcessing).
            HostCall::SetActive(on) => on != self.active && (!on || self.max_block > 0),
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
            HostCall::Process { frames, .. } => Expect {
                result_ok: block(frames) != BlockExpect::Refused,
                block: block(frames),
            },
            // The save holds the plugin while it serializes; a block that
            // arrives then cannot run. Inactive, it was silent anyway.
            HostCall::SaveDuringProcess { frames } => {
                let block = match block(frames) {
                    BlockExpect::Ran => BlockExpect::Contended,
                    other => other,
                };
                Expect {
                    result_ok: block != BlockExpect::Refused,
                    block,
                }
            }
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

/// The lifecycle as a [`Model`](crate::check::Model) over a compliant
/// host: a small call alphabet (two setups, toggles, blocks of 0, 1 and
/// the maximum frames, a save), so [`check`](crate::check::check)
/// explores every reachable state and [`validate`](crate::check::validate)
/// checks a recorded run's calls (the simulated host's) against it.
pub struct Compliant;

/// `Lifecycle` without its float, for hashing: the model's state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Abstract {
    pub max_block: usize,
    pub active: bool,
    pub processing: bool,
}

impl From<&Lifecycle> for Abstract {
    fn from(l: &Lifecycle) -> Self {
        Self {
            max_block: l.max_block,
            active: l.active,
            processing: l.processing,
        }
    }
}

fn concrete(s: &Abstract) -> Lifecycle {
    Lifecycle {
        sample_rate: 48_000.0,
        max_block: s.max_block,
        active: s.active,
        processing: s.processing,
        dirty: true,
    }
}

impl crate::check::Model for Compliant {
    type State = Abstract;
    type Action = HostCall;

    fn init(&self) -> Vec<Abstract> {
        vec![Abstract::from(&Lifecycle::created())]
    }

    fn actions(&self, s: &Abstract, out: &mut Vec<HostCall>) {
        for max_block in [64, 512] {
            out.push(HostCall::Setup {
                sample_rate: 48_000.0,
                max_block,
            });
        }
        out.extend([
            HostCall::SetActive(!s.active),
            HostCall::SetProcessing(!s.processing),
            HostCall::SaveState,
            HostCall::SaveDuringProcess { frames: s.max_block },
        ]);
        for frames in [0, 1, s.max_block] {
            out.push(HostCall::Process {
                frames,
                shape: BufferShape::InPlace,
                changes: 0,
                change_seed: 0,
            });
        }
    }

    /// Only the calls the format allows here; any of them is one step.
    fn next(&self, s: &Abstract, call: &HostCall) -> Option<Abstract> {
        let mut l = concrete(s);
        if !l.allows(call) {
            return None;
        }
        l.apply(call);
        Some(Abstract::from(&l))
    }

    fn invariant(&self, s: &Abstract) -> Result<(), String> {
        if s.processing && !s.active {
            return Err("processing while inactive".into());
        }
        let l = concrete(s);
        // A compliant block always runs (or is empty) once processing.
        if s.processing {
            for frames in [1, s.max_block] {
                let call = HostCall::Process {
                    frames,
                    shape: BufferShape::Separate,
                    changes: 0,
                    change_seed: 0,
                };
                if l.allows(&call) && l.expect(&call).block != BlockExpect::Ran {
                    return Err(format!("a legal {frames}-frame block would not run"));
                }
                // A save during playback is legal: the block it races
                // succeeds, run or dropped, and is never refused.
                let racing = HostCall::SaveDuringProcess { frames };
                if l.allows(&racing) {
                    let e = l.expect(&racing);
                    if !e.result_ok || e.block != BlockExpect::Contended {
                        return Err(format!("a legal {frames}-frame block racing a save answers {e:?}"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The host can always get the plugin back to inactive.
    fn quiescent(&self, s: &Abstract) -> Option<bool> {
        Some(!s.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{check, validate};

    #[test]
    fn every_compliant_state_keeps_the_laws_and_can_return_to_inactive() {
        let stats = check(&Compliant, 10_000).expect("the lifecycle");
        assert!(stats.states >= 6, "{stats:?}");
    }

    #[test]
    fn a_trace_out_of_order_is_caught() {
        let setup = HostCall::Setup { sample_rate: 48_000.0, max_block: 64 };
        let block = HostCall::Process { frames: 64, shape: BufferShape::Separate, changes: 0, change_seed: 0 };
        let ok = [setup.clone(), HostCall::SetActive(true), HostCall::SetProcessing(true), block.clone()];
        assert!(validate(&Compliant, Abstract::from(&Lifecycle::created()), &ok).is_ok());
        let early = [setup, block];
        let d = validate(&Compliant, Abstract::from(&Lifecycle::created()), &early).unwrap_err();
        assert_eq!(d.step, 1);
    }
}
