//! Why a format entry point refused a host's call, logged before the
//! host sees the format's failure code.

use super::Severity;
use crate::state::StateError;
use core::fmt;

#[derive(Debug)]
pub enum CallError {
    NullArgument(&'static str),
    UnknownParamId(u32),
    UnknownConfigId(u32),
    UnknownUnitId(i32),
    UnknownProgramListId(i32),
    IndexOutOfRange(i64),
    /// A host stream without its read or write function.
    IncompleteStream,
    StreamWrite {
        written: usize,
        of: usize,
    },
    StreamRead,
    StateTooLarge {
        limit: usize,
    },
    State(StateError),
    /// The audio thread held the plugin longer than the call waits.
    ProcessorBusy,
    /// Saved without the plugin's extra state: the audio thread held it.
    ExtraStateSkipped,
}

impl CallError {
    pub fn severity(&self) -> Severity {
        match self {
            Self::NullArgument(_)
            | Self::UnknownParamId(_)
            | Self::UnknownConfigId(_)
            | Self::UnknownUnitId(_)
            | Self::UnknownProgramListId(_)
            | Self::IndexOutOfRange(_)
            | Self::IncompleteStream => Severity::HostMisbehaving,
            Self::StreamWrite { .. }
            | Self::StreamRead
            | Self::StateTooLarge { .. }
            | Self::State(_)
            | Self::ProcessorBusy
            | Self::ExtraStateSkipped => Severity::Error,
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullArgument(name) => write!(f, "`{name}` is null"),
            Self::UnknownParamId(id) => write!(f, "no parameter has id {id}"),
            Self::UnknownConfigId(id) => write!(f, "no port configuration has id {id}"),
            Self::UnknownUnitId(id) => write!(f, "no unit has id {id}"),
            Self::UnknownProgramListId(id) => write!(f, "no program list has id {id}"),
            Self::IndexOutOfRange(index) => write!(f, "index {index} is out of range"),
            Self::IncompleteStream => f.write_str("the stream has no read or write function"),
            Self::StreamWrite { written, of } => {
                write!(f, "the stream took {written} of {of} bytes")
            }
            Self::StreamRead => f.write_str("the stream failed to read"),
            Self::StateTooLarge { limit } => write!(f, "state is larger than {limit} bytes"),
            Self::State(error) => write!(f, "state not loaded: {error}"),
            Self::ProcessorBusy => f.write_str("the audio thread held the plugin"),
            Self::ExtraStateSkipped => {
                f.write_str("saved without the plugin's extra state: the audio thread held it")
            }
        }
    }
}

impl From<StateError> for CallError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}
