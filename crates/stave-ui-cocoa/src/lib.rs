//! The macOS host for a stave face — OS-specific, ABI-AGNOSTIC. Every
//! plugin wrapper (VST3 today) adapts THIS editor to its
//! window-embedding ABI; no view code lives in wrapper crates.
//!
//! Layout: `view.rs` = the plugin editor (attach / detach / size, the
//! face and its presets); `face.rs` = the face's NSView (paint, pointer,
//! repaint timer) and the parameter bridge; `binding.rs` = the table and
//! the host's gesture path; `offscreen.rs` + `text.rs` = the raster blit
//! and CoreText; `objc.rs` = runtime FFI.

// Unsafe here is Objective-C runtime FFI, documented with the UI overhaul
// (PLAN S6), not block by block before it.
#![allow(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc)]

pub mod face;
pub mod offscreen;
pub mod text;

pub(crate) mod binding;
pub(crate) mod objc;
pub(crate) mod view;

pub use binding::EditorHost;
pub use view::PluginEditorView;

pub(crate) use objc::*;
