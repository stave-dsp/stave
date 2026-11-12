//! # stave-face — the design system
//!
//! A plugin's editor is a **face**: a physically lit instrument panel
//! composed from **parts** (knobs, toggles, footswitches, lamps,
//! displays, plates, print) and laid out in rows, columns and stacks. A
//! [`Theme`] says what everything looks like; parts never mint a colour,
//! a font or a coordinate.
//!
//! ```text
//! theme.rs     the look as data: finish, palette, knob family, type, spacing
//! material.rs  lit surfaces and hardware (one studio light), parallel bakes
//! photo.rs     the studio: vectors, light, materials, grain, distances, bakers
//! part.rs      the Part contract: bake (static) / paint (live) / controls
//! parts/       the catalogue
//! screens.rs   pictures for displays: graticule, scope, spectrum, curve, envelope
//! runtime.rs   layout, the cached static layer, live paint, and every
//!              gesture (drag, fine-drag, double-click reset, wheel,
//!              stepping, latching, pads)
//! ```
//!
//! Parts read and write the plugin through its [`Controller`] (from
//! `stave-core`): parameters by `ParamId`, taps by `TapId`. The runtime
//! is portable: a platform view feeds it pointer events and blits its
//! canvas (`stave-ui-cocoa`'s `FaceView` on macOS).

#![forbid(unsafe_code)]

pub mod material;
pub mod part;
pub mod photo;
pub mod parts;
pub mod runtime;
pub mod screens;
pub mod theme;

pub use part::{leaf, on_page, pages, step_of, step_value, Bind, BindParam, Control, FaceNode, Focus, Frame, Gesture, Hit, Part};
pub use stave_core::telemetry::TapId;
pub use stave_core::Controller;
pub use parts::*;
pub use runtime::{Cursor, Face, Mods, Runtime};
pub use theme::{CapStyle, Finish, Theme, Typography};

/// Everything a face definition needs in one import.
pub mod prelude {
    pub use crate::parts::*;
    pub use crate::{on_page, pages, screens, Bind, BindParam, Face, FaceNode, Frame, TapId, Theme};
    pub use stave_ui::layout::Rect;
    pub use stave_ui::scene::{pt, Path, Scene, Stroke};
    pub use stave_ui::style::Color;
    pub use stave_ui::{column, row, spacer, stack, Align, Dim, Justify};
}
