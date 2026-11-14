//! The Windows platform for a stave face — a child window (HWND) of the
//! host's (CLAP `win32`, VST3 `HWND`), painted by the portable software
//! renderer and blitted with GDI, at the parent's DPI. The window has
//! its own repaint timer; the host's message loop runs everything.
//!
//! Layout: `view.rs` = `FaceView` (window class and procedure,
//! pointer, keys, blit); `app.rs` = a stand-alone window around a view;
//! `sys.rs` = the Win32 declarations, each call's invariant at its site.
//!
//! The crate is empty off Windows.

#![cfg(windows)]

mod app;
#[doc(hidden)]
pub mod sys;
mod view;

pub use app::run_app;
pub use stave_ui_shell::OnKey;
pub use view::FaceView;

/// The virtual keys a stand-alone window's `on_key` sees for the keys
/// the storybook uses.
pub mod keys {
    pub const ESCAPE: u16 = crate::sys::VK_ESCAPE;
    pub const LEFT: u16 = crate::sys::VK_LEFT;
    pub const RIGHT: u16 = crate::sys::VK_RIGHT;
    pub const TAB: u16 = 0x09;
}
