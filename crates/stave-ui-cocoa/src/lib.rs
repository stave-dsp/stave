//! The macOS host for stave-ui — OS-specific, ABI-AGNOSTIC. Every
//! plugin wrapper (VST3 today, CLAP/AU next) adapts THIS editor to
//! its window-embedding ABI; no view code lives in wrapper crates.
//!
//! Layout: `mount.rs` realises solved stave-ui trees (metrics +
//! paint + widget dispatch); `widgets/` = one file per platform
//! widget (the escape-hatch leaves — north star: decomposition into
//! Shape/Glyph/Text); `view.rs` = the editor instance + 30 Hz tick;
//! `objc.rs` = runtime FFI; `theme.rs` = font roles + legacy color
//! tuples (deriving from stave-ui tokens).

// Unsafe here is Objective-C runtime FFI, documented with the UI overhaul
// (PLAN S6), not block by block before it.
#![allow(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc)]

pub mod theme;
pub mod offscreen;
pub mod text;

pub(crate) mod binding;
pub(crate) mod cg;
pub mod face;
pub(crate) mod mapping;
pub(crate) mod mount;
pub(crate) mod objc;
pub(crate) mod tick;
pub(crate) mod view;
pub(crate) mod widgets;

pub use binding::EditorHost;
pub use view::PluginEditorView;

// Flat re-exports for the widget modules' `use crate::*`.
pub(crate) use mapping::*;
pub(crate) use objc::*;
pub(crate) use widgets::engine::*;
pub(crate) use widgets::knob::*;
pub(crate) use widgets::meter::*;
pub(crate) use widgets::pad::*;
pub(crate) use widgets::targets::*;
