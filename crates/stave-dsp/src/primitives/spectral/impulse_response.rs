//! **ImpulseResponse** — what a [`Convolver`](super::Convolver)
//! convolves with: one path (mono), two (stereo, each lane its own
//! response) or four (true stereo, every input to every output).
//!
//! The samples are taken as given, at the rate the convolver will be
//! prepared at. Loading a file, resampling to the prepared rate and
//! normalising arrive with DSP.md F4.

/// How an [`ImpulseResponse`]'s paths connect inputs to outputs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IrLayout {
    /// One input, one output, one path.
    Mono,
    /// Two lanes, each convolved with its own response: left → left,
    /// right → right.
    Stereo,
    /// Two inputs, two outputs, four paths: each output is the sum of
    /// both inputs, each through its own response (a stereo room
    /// captured from two source positions).
    TrueStereo,
}

impl IrLayout {
    /// The input (and output) lanes: 1 for mono, 2 otherwise.
    pub const fn lanes(self) -> usize {
        match self {
            IrLayout::Mono => 1,
            IrLayout::Stereo | IrLayout::TrueStereo => 2,
        }
    }
}

/// An impulse response: one, two or four paths of samples, laid out by
/// an [`IrLayout`]. Input data for a [`Convolver`](super::Convolver),
/// copied in at construction (which allocates); it computes nothing
/// itself. Its accessors are checked by the doctest below and the
/// convolver's tests (`tests/convolver_properties.rs`).
///
/// ```
/// use stave_dsp::ImpulseResponse;
/// use stave_dsp::primitives::spectral::IrLayout;
///
/// let room = ImpulseResponse::stereo(&[1.0, 0.5, 0.25], &[1.0, -0.5]);
/// assert_eq!(room.layout(), IrLayout::Stereo);
/// assert_eq!(room.len(), 3); // the longest path
/// assert_eq!(room.path(1, 1), Some(&[1.0, -0.5][..]));
/// assert_eq!(room.path(0, 1), None); // stereo has no cross paths
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ImpulseResponse {
    layout: IrLayout,
    /// `paths[input * lanes + output]`, `None` where the layout has no
    /// path.
    paths: Vec<Option<Vec<f32>>>,
}

impl ImpulseResponse {
    /// One path: the response of a mono system.
    pub fn mono(samples: &[f32]) -> Self {
        Self {
            layout: IrLayout::Mono,
            paths: vec![Some(samples.to_vec())],
        }
    }

    /// Two independent lanes: `left` convolves the left input into the
    /// left output, `right` the right into the right.
    pub fn stereo(left: &[f32], right: &[f32]) -> Self {
        Self {
            layout: IrLayout::Stereo,
            paths: vec![Some(left.to_vec()), None, None, Some(right.to_vec())],
        }
    }

    /// Four paths, named source → destination: the left output is
    /// `left_to_left ∗ L + right_to_left ∗ R`, the right output
    /// `left_to_right ∗ L + right_to_right ∗ R`.
    pub fn true_stereo(
        left_to_left: &[f32],
        left_to_right: &[f32],
        right_to_left: &[f32],
        right_to_right: &[f32],
    ) -> Self {
        Self {
            layout: IrLayout::TrueStereo,
            paths: vec![
                Some(left_to_left.to_vec()),
                Some(left_to_right.to_vec()),
                Some(right_to_left.to_vec()),
                Some(right_to_right.to_vec()),
            ],
        }
    }

    /// How the paths connect inputs to outputs.
    pub const fn layout(&self) -> IrLayout {
        self.layout
    }

    /// The longest path, in samples.
    pub fn len(&self) -> usize {
        self.paths.iter().flatten().map(Vec::len).max().unwrap_or(0)
    }

    /// `true` when every path is empty (the response of silence).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The samples from input lane `input` to output lane `output`, or
    /// `None` when the layout has no such path.
    pub fn path(&self, input: usize, output: usize) -> Option<&[f32]> {
        let lanes = self.layout.lanes();
        if input >= lanes || output >= lanes {
            return None;
        }
        self.paths[input * lanes + output].as_deref()
    }
}
