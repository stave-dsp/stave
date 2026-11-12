//! The Linux platform for a stave face — an X11 child window over the
//! host's (CLAP `x11`, VST3 `X11EmbedWindowID`), painted by the
//! portable software renderer and blitted with `PutImage`. Pure Rust
//! (x11rb's own protocol implementation), no unsafe: the parent is a
//! window id, not memory.
//!
//! X11 has no main loop a plugin can join, so the view does nothing on
//! its own: the host's run loop calls [`FaceView::pump`] when the
//! connection's file descriptor ([`FaceView::fd`]) is readable and on a
//! repaint timer (CLAP `posix-fd-support` + `timer-support`, VST3
//! `IRunLoop`). [`run_app`] is that loop for a stand-alone window.
//!
//! Layout: `view.rs` = [`FaceView`] (window, pointer, keys, blit);
//! `pixels.rs` = the visual's pixel format; `app.rs` = a stand-alone
//! window around a view.

#![forbid(unsafe_code)]

mod app;
/// The pixel format (public for the tests that read a window back).
#[doc(hidden)]
pub mod pixels;
mod view;

pub use app::run_app;
pub use stave_ui_shell::OnKey;
pub use view::{AttachError, FaceView};

/// The keysyms a stand-alone window's `on_key` sees for the keys the
/// storybook uses (the low 16 bits of the X11 keysym).
pub mod keys {
    pub const ESCAPE: u16 = 0xff1b;
    pub const LEFT: u16 = 0xff51;
    pub const RIGHT: u16 = 0xff53;
}
