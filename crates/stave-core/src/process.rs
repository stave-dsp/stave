//! [`Block`]: one `process` call's audio, events and parameter changes.

use crate::buffer::{InputBuffer, OutputBuffer};
use crate::event::Event;
use crate::params::ParamId;

/// One host parameter change on the block's timeline.
///
/// `value` is in the parameter's declared (plain) units; the wrappers
/// convert their wire representation before building it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParamChange {
    pub id: ParamId,
    /// Frame offset from the start of the block.
    pub frame: usize,
    pub value: f64,
    pub kind: ParamChangeKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParamChangeKind {
    Value,
    Modulation,
}

/// The parameter timeline for one process call, ordered by frame.
///
/// The latest target is also in the shared parameter table; a
/// [`Smoother`](crate::params::Smoother) reads this when timing inside
/// the block matters.
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

    /// A block whose fixed wrapper buffer overflowed by `dropped` points.
    pub const fn with_dropped(changes: &'a [ParamChange], dropped: u32) -> Self {
        Self { changes, dropped }
    }

    pub const fn empty() -> Self {
        Self::new(&[])
    }

    pub fn changes(self) -> &'a [ParamChange] {
        self.changes
    }

    /// Host points left out because the wrapper's bounded storage was
    /// full (also counted as a fault).
    pub const fn dropped(self) -> u32 {
        self.dropped
    }

    pub fn for_param(self, id: ParamId) -> impl Iterator<Item = &'a ParamChange> + 'a {
        self.changes.iter().filter(move |change| change.id == id)
    }
}

/// Everything a plugin needs to process one block of audio. Built by the
/// format wrappers (and test harnesses); read through its methods.
pub struct Block<'a> {
    sample_rate: f32,
    frames: usize,
    transport: Option<Transport>,
    input: InputBuffer<'a>,
    output: OutputBuffer<'a>,
    aux: &'a [InputBuffer<'a>],
    params: ParamBlock<'a>,
    events: &'a [Event],
}

impl<'a> Block<'a> {
    /// A block of `frames` frames at `sample_rate` Hz: no aux inputs, no
    /// transport, no events, no parameter changes until the `with_`
    /// methods add them.
    pub fn new(
        sample_rate: f32,
        frames: usize,
        input: InputBuffer<'a>,
        output: OutputBuffer<'a>,
    ) -> Self {
        Self {
            sample_rate,
            frames,
            transport: None,
            input,
            output,
            aux: &[],
            params: ParamBlock::empty(),
            events: &[],
        }
    }

    /// Aux (sidechain) inputs, one per aux bus of the served layout, in
    /// order; an unconnected bus is [`InputBuffer::empty`].
    pub fn with_aux(mut self, aux: &'a [InputBuffer<'a>]) -> Self {
        self.aux = aux;
        self
    }

    pub fn with_transport(mut self, transport: Option<Transport>) -> Self {
        self.transport = transport;
        self
    }

    /// Musical events, ordered by frame.
    pub fn with_events(mut self, events: &'a [Event]) -> Self {
        self.events = events;
        self
    }

    /// Parameter changes, ordered by frame.
    pub fn with_params(mut self, params: ParamBlock<'a>) -> Self {
        self.params = params;
        self
    }

    /// Frames in this block.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Hz.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The host's timeline, when it gives one.
    pub fn transport(&self) -> Option<&Transport> {
        self.transport.as_ref()
    }

    /// Musical events (notes), ordered by frame. Parameter changes have
    /// their own timeline ([`params`](Self::params)).
    pub fn events(&self) -> &'a [Event] {
        self.events
    }

    /// The parameter changes inside this block.
    pub fn params(&self) -> ParamBlock<'a> {
        self.params
    }

    /// The main input (no channels for an instrument).
    pub fn input(&self) -> &InputBuffer<'a> {
        &self.input
    }

    /// The main output.
    pub fn output(&mut self) -> &mut OutputBuffer<'a> {
        &mut self.output
    }

    /// The main input and output at once: read one while writing the
    /// other.
    pub fn io(&mut self) -> (&InputBuffer<'a>, &mut OutputBuffer<'a>) {
        (&self.input, &mut self.output)
    }

    /// Aux input `i` of the served layout; `None` past the declared aux
    /// buses (an unconnected one has no channels).
    pub fn aux(&self, i: usize) -> Option<&InputBuffer<'a>> {
        self.aux.get(i)
    }
}

/// The host's timeline. Every field the host can leave out is an
/// `Option` — a made-up tempo is worse than none.
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
                frame: 7,
                value: 0.5,
                kind: ParamChangeKind::Value,
            },
            ParamChange {
                id: ParamId(3),
                frame: 9,
                value: 1.0,
                kind: ParamChangeKind::Modulation,
            },
        ];
        let block = ParamBlock::with_dropped(&changes, 4);
        assert_eq!(block.for_param(ParamId(2)).count(), 1);
        assert_eq!(block.for_param(ParamId(9)).count(), 0);
        assert_eq!(block.dropped(), 4);
    }
}
