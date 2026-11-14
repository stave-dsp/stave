//! The macOS platform for a stave face — ABI-agnostic: every plugin
//! format wrapper (VST3, CLAP, AU) and the storybook show a face through
//! [`FaceView`]; no view code lives in wrapper crates, and no parameter
//! logic lives here (that is the face's `Controller`).
//!
//! Layout: `view.rs` = [`FaceView`], the face's NSView (paint, pointer,
//! keys, repaint timer); `app.rs` = a stand-alone window around one;
//! `hosted.rs` = a face in a view the host asks for (an Audio Unit's
//! Cocoa view, [`view_factory`]);
//! `offscreen.rs` + `text.rs` = the raster blit and CoreText; `objc.rs` =
//! the typed Objective-C core every message goes through; `cf.rs` =
//! CoreFoundation references and CoreGraphics geometry.
//!
//! The crate is empty off macOS (`stave-ui-win32` and `stave-ui-x11` are
//! the other platforms, with the same `FaceView` shape).

// Off macOS the crate is empty, and these docs link to nothing.
#![cfg_attr(not(target_os = "macos"), allow(rustdoc::broken_intra_doc_links))]
#![cfg(target_os = "macos")]

pub mod offscreen;
pub mod text;

mod app;
mod cf;
mod hosted;
mod objc;
mod view;

#[doc(hidden)]
pub mod synthetic;

pub use app::run_app;
pub use hosted::{view_factory, MakeView, ViewFactory};
pub use view::{AttachError, FaceView, OnKey};
