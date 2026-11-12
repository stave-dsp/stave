use crate::buffer::AudioBuffer;
use crate::event::Event;
use crate::params::ParamId;

/// One host parameter change on the current process timeline.
///
/// Values are always in the parameter's declared plain units. Wrappers
/// normalize their native representation before constructing this value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParamChange {
    pub id: ParamId,
    pub sample_offset: u32,
    pub plain_value: f64,
    pub kind: ParamChangeKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParamChangeKind {
    Value,
    Modulation,
}

/// The parameter timeline for one process call.
///
/// Changes are ordered by sample offset. The latest target is also committed
/// to the shared parameter store by the wrapper, but processors use this view
/// when timing inside the block matters.
#[derive(Copy, Clone, Debug)]
pub struct ParamBlock<'a> {
    changes: &'a [ParamChange],
    dropped: u32,
}

impl<'a> ParamBlock<'a> {
    pub const fn new(changes: &'a [ParamChange]) -> Self {
        Self {
            changes,
            dropped: 0,
        }
    }

    /// Construct a block whose fixed wrapper buffer overflowed.
    pub const fn with_dropped(changes: &'a [ParamChange], dropped: u32) -> Self {
        Self { changes, dropped }
    }

    pub const fn empty() -> Self {
        Self {
            changes: &[],
            dropped: 0,
        }
    }

    pub fn changes(self) -> &'a [ParamChange] {
        self.changes
    }

    /// Number of host points omitted because the wrapper's bounded storage
    /// was full. Consumers can publish or assert this; overflow is explicit.
    pub const fn dropped(self) -> u32 {
        self.dropped
    }

    pub fn for_param(self, id: ParamId) -> impl Iterator<Item = &'a ParamChange> + 'a {
        self.changes.iter().filter(move |change| change.id == id)
    }
}

/// Everything a plugin needs to render one block of audio.
pub struct ProcessCtx<'a> {
    pub sample_rate: f32,
    pub frames: usize,
    pub transport: Option<Transport>,
    pub input: AudioBuffer<'a>,
    pub output: AudioBuffer<'a>,
    /// Aux inputs (sidechains), in the DECLARED order of the served
    /// `PortConfig`'s `aux_in`. Wrappers guarantee one entry per
    /// declared aux; an unconnected/silent port arrives as a
    /// zero-channel buffer — the `AuxIn` contract is that the plugin
    /// is fully functional with every aux absent.
    pub aux: &'a [AudioBuffer<'a>],
    /// Ordered host parameter changes for this block.
    pub params: ParamBlock<'a>,
    /// Musical events only. Parameter values have their own timeline above.
    pub events: &'a [Event],
}

/// Host timeline state. Fields are `Option` because hosts declare
/// validity per field (VST3 state bits, CLAP flags) — a fabricated
/// default tempo is worse than an honest `None`.
#[derive(Copy, Clone, Debug, Default)]
pub struct Transport {
    pub playing: bool,
    pub tempo_bpm: Option<f64>,
    /// Musical position in quarter notes.
    pub ppq_position: Option<f64>,
    pub song_pos_seconds: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_block_filters_and_reports_overflow() {
        let changes = [
            ParamChange {
                id: ParamId(2),
                sample_offset: 7,
                plain_value: 0.5,
                kind: ParamChangeKind::Value,
            },
            ParamChange {
                id: ParamId(3),
                sample_offset: 9,
                plain_value: 1.0,
                kind: ParamChangeKind::Modulation,
            },
        ];
        let block = ParamBlock::with_dropped(&changes, 4);
        assert_eq!(block.for_param(ParamId(2)).count(), 1);
        assert_eq!(block.for_param(ParamId(9)).count(), 0);
        assert_eq!(block.dropped(), 4);
    }
}
