//! Headless `--screenshot PATH [--note N] [--editor-only]`: let the
//! editor settle ~90 frames (holding a note from frame 20 so meters and
//! tails are alive), then capture OUR window with `screencapture`, or —
//! `--editor-only` — the editor alone: a stave face draws itself
//! offscreen into PATH (`STAVE_FACE_SNAPSHOT`, set at launch), and only
//! another vendor's editor falls back to capturing its screen rectangle.
//! The agent's eyes on the editor without a human at the keyboard.

use std::path::PathBuf;

use eframe::egui;
use stave_host::note::{Note, NoteEvent};

use super::HostApp;

const SETTLE_FRAMES: u32 = 90;
const NOTE_AT_FRAMES_LEFT: u32 = 70;

/// What a screenshot captures.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum CaptureRegion {
    /// The whole stave-play window.
    #[default]
    Window,
    /// Only the plugin editor, at the size it is shown.
    Editor,
}

pub(super) struct Screenshot {
    path: PathBuf,
    note: Option<i16>,
    region: CaptureRegion,
    frames_left: u32,
}

#[derive(Debug, PartialEq)]
enum Step {
    Wait,
    Play(i16),
    Capture(PathBuf),
}

impl Screenshot {
    pub fn new(path: PathBuf, note: Option<i16>, region: CaptureRegion) -> Self {
        Self {
            path,
            note,
            region,
            frames_left: SETTLE_FRAMES,
        }
    }

    fn tick(&mut self) -> Step {
        self.frames_left = self.frames_left.saturating_sub(1);
        match self.frames_left {
            0 => Step::Capture(self.path.clone()),
            NOTE_AT_FRAMES_LEFT => self.note.take().map_or(Step::Wait, Step::Play),
            _ => Step::Wait,
        }
    }
}

impl HostApp {
    pub(super) fn tick_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.screenshot.as_mut() else {
            return;
        };
        match shot.tick() {
            Step::Wait => ctx.request_repaint(),
            Step::Play(key) => self.push_note(NoteEvent::on(key, Note::DEFAULT_VELOCITY)),
            Step::Capture(path) => {
                let region = shot.region;
                self.screenshot = None;
                if region == CaptureRegion::Editor && path.exists() {
                    println!("screenshot: {} (the face, drawn offscreen)", path.display());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                let target = match region {
                    CaptureRegion::Window => window_number(self.ns_view).map(Target::Window),
                    CaptureRegion::Editor => self
                        .session
                        .as_ref()
                        .and_then(|s| s.editor())
                        .and_then(|e| e.screen_rect())
                        .map(Target::Rect),
                };
                match capture(target, &path) {
                    Ok(()) => println!("screenshot: {}", path.display()),
                    Err(e) => {
                        eprintln!("screenshot FAILED: {}", crate::errors::with_causes(&e))
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum CaptureError {
    #[error("nothing to capture: no window, or no plugin editor shown")]
    NoTarget,
    #[error("could not run screencapture")]
    Spawn(#[source] std::io::Error),
    #[error("screencapture exited {0}")]
    Exited(std::process::ExitStatus),
}

enum Target {
    Window(i64),
    /// `[x, y, w, h]`, global points from the primary display's top-left.
    Rect([f64; 4]),
}

fn capture(target: Option<Target>, path: &std::path::Path) -> Result<(), CaptureError> {
    let args = match target.ok_or(CaptureError::NoTarget)? {
        Target::Window(id) => vec!["-o".to_string(), "-l".to_string(), id.to_string()],
        Target::Rect([x, y, w, h]) => vec![format!("-R{x},{y},{w},{h}")],
    };
    let status = std::process::Command::new("screencapture")
        .arg("-x")
        .args(args)
        .arg(path)
        .status()
        .map_err(CaptureError::Spawn)?;
    status
        .success()
        .then_some(())
        .ok_or(CaptureError::Exited(status))
}

/// The window's root NSView (macOS) — the parent for plugin editors.
pub(super) fn root_ns_view(cc: &eframe::CreationContext<'_>) -> *mut std::ffi::c_void {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::AppKit(h)) => h.ns_view.as_ptr(),
        _ => std::ptr::null_mut(),
    }
}

/// The NSWindow number of the view's window (what `screencapture -l`
/// takes).
fn window_number(ns_view: *mut std::ffi::c_void) -> Option<i64> {
    type Id = *mut std::ffi::c_void;
    type Sel = *const std::ffi::c_void;
    extern "C" {
        fn objc_msgSend();
        fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
    }
    if ns_view.is_null() {
        return None;
    }
    // SAFETY: objc_msgSend cast to each message's exact signature;
    // `ns_view` is the live root view eframe handed us.
    unsafe {
        let send_id: unsafe extern "C" fn(Id, Sel) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let send_i64: unsafe extern "C" fn(Id, Sel) -> i64 =
            std::mem::transmute(objc_msgSend as *const ());
        let win = send_id(ns_view, sel_registerName(c"window".as_ptr()));
        (!win.is_null()).then(|| send_i64(win, sel_registerName(c"windowNumber".as_ptr())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_the_note_once_then_captures_after_settling() {
        let mut s = Screenshot::new("x.png".into(), Some(60), CaptureRegion::Window);
        let steps: Vec<Step> = (0..SETTLE_FRAMES).map(|_| s.tick()).collect();
        assert_eq!(steps.iter().filter(|s| **s == Step::Play(60)).count(), 1);
        assert_eq!(
            steps[(SETTLE_FRAMES - NOTE_AT_FRAMES_LEFT - 1) as usize],
            Step::Play(60)
        );
        let capture = Step::Capture("x.png".into());
        assert_eq!(*steps.last().unwrap(), capture);
        assert_eq!(steps.iter().filter(|s| **s == capture).count(), 1);
    }

    #[test]
    fn no_note_no_play() {
        let mut s = Screenshot::new("x.png".into(), None, CaptureRegion::Window);
        assert!((0..SETTLE_FRAMES).all(|_| !matches!(s.tick(), Step::Play(_))));
    }
}
