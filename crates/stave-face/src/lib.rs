//! # stave-face — the design system
//!
//! A plugin's editor is a **face**: a physically lit instrument panel
//! composed from **parts** (knobs, toggles, footswitches, lamps,
//! displays, plates, print) and laid out with `ui!` rows and columns. A
//! [`Theme`] says what everything looks like; parts never mint a colour,
//! a font or a coordinate.
//!
//! ```text
//! theme.rs     the look as data: finish, palette, knob family, type, spacing
//! material.rs  lit surfaces and hardware (one studio light), parallel bakes
//! part.rs      the Part contract: bake (static) / paint (live) / controls
//! parts/       the catalogue
//! runtime.rs   layout, the cached static layer, live paint, and every
//!              gesture (drag, fine-drag, double-click reset, wheel,
//!              stepping, latching, pads)
//! ```
//!
//! The runtime is portable: a platform host feeds it pointer events and
//! blits its canvas (`stave-ui-cocoa::face` on macOS).

pub mod material;
pub mod part;
pub mod parts;
pub mod runtime;
pub mod theme;

pub use part::{leaf, step_of, step_value, Bind, Control, FaceNode, Focus, Frame, Gesture, Hit, Host, Part};
pub use parts::*;
pub use runtime::{Cursor, Face, Mods, Runtime};
pub use theme::{CapStyle, Finish, Theme, Typography};

/// Everything a face definition needs in one import.
pub mod prelude {
    pub use crate::parts::*;
    pub use crate::{Bind, Face, FaceNode, Theme};
    pub use stave_ui::{column, row, spacer, stack, ui, Align, Dim, Justify};
}
