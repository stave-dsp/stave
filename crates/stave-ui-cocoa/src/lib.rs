//! The macOS platform for a stave face — ABI-agnostic: every plugin
//! format wrapper (VST3, CLAP) and the storybook show a face through
//! [`FaceView`]; no view code lives in wrapper crates, and no parameter
//! logic lives here (that is the face's `Controller`).
//!
//! Layout: `view.rs` = [`FaceView`], the face's NSView (paint, pointer,
//! keys, repaint timer); `app.rs` = a stand-alone window around one;
//! `offscreen.rs` + `text.rs` = the raster blit and CoreText; `objc.rs` =
//! the typed Objective-C core every message goes through; `cf.rs` =
//! CoreFoundation references and CoreGraphics geometry.

pub mod offscreen;
pub mod text;

mod app;
mod cf;
mod objc;
mod view;

#[doc(hidden)]
pub mod synthetic;

pub use app::run_app;
pub use view::{FaceView, OnKey};
