//! The macOS platform for a stave face — ABI-agnostic: every plugin
//! format wrapper (VST3, CLAP) and the storybook show a face through
//! [`FaceView`]; no view code lives in wrapper crates, and no parameter
//! logic lives here (that is the face's `Controller`).
//!
//! Layout: `view.rs` = [`FaceView`], the face's NSView (paint, pointer,
//! keys, repaint timer); `offscreen.rs` + `text.rs` = the raster blit and
//! CoreText; `objc.rs` = runtime FFI.

// Unsafe here is Objective-C runtime FFI, documented with the UI overhaul
// (PLAN S6), not block by block before it.
#![allow(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc)]

pub mod offscreen;
pub mod text;

pub(crate) mod objc;
pub(crate) mod view;

pub use view::{FaceView, OnKey};

pub(crate) use objc::*;
