//! How a plugin declares its face. `stave-core` is UI-free, so the face
//! is declared here, beside the core `Spec`: an engine implements
//! [`FaceSpec`] (a pure function of its params), and the format wrappers
//! ask any plugin for its face through [`HasFace`].

use stave_core::{Plugin, Spec, StavePlugin};

use crate::runtime::Face;

/// The face half of a plugin's declarations: its editor, a pure
/// function of its params. A plugin without one writes
/// `impl FaceSpec for MyEngine {}`.
pub trait FaceSpec: Spec {
    fn face(_params: &Self::Params) -> Option<Face> {
        None
    }
}

/// What a format wrapper asks an instance for: the face to show, if
/// any. `StavePlugin` answers from its engine's [`FaceSpec`]; a plugin
/// implementing `Plugin` directly implements this (often empty).
pub trait HasFace: Plugin {
    fn face(&self) -> Option<Face> {
        None
    }
}

impl<E: FaceSpec> HasFace for StavePlugin<E> {
    fn face(&self) -> Option<Face> {
        E::face(self.params())
    }
}
