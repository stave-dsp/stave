//! What the Windows and Linux platform shells (`stave-ui-win32`,
//! `stave-ui-x11`) share, so each is only a window, a pointer, a blit
//! and its event loop: a [`Shell`] — the face's controller and its
//! [`Session`] behind a reentrancy guard and a panic boundary — and the
//! click counting those window systems leave to the application.
//!
//! A session paints with the embedded fonts ([`stave_font::FontText`]),
//! records every event to the file `STAVE_FACE_TRACE` names (a session
//! that replays as a test: `stave_face::Trace`), and writes its own
//! picture to the PNG `STAVE_FACE_SNAPSHOT` names once it has run a
//! second — the same contract as the macOS view.

#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use stave_core::Controller;
use stave_face::{Act, Cursor, Face, Key, Mods, Runtime};
use stave_font::FontText;
use stave_raster::Canvas;
use stave_ui::scene::Pt;

/// What a key press does in a stand-alone window: return a face to
/// show instead, or `None` to keep the current one. The code is the
/// platform's (a Windows virtual key, an X11 keysym).
pub type OnKey = Box<dyn FnMut(u16) -> Option<Face>>;

/// Seconds a face runs before `STAVE_FACE_SNAPSHOT` takes its picture
/// (meters and animations under way).
const SNAPSHOT_AFTER: f64 = 1.0;

/// One face on screen: its runtime, the canvas it paints into, its
/// clock, and the trace and snapshot files.
pub struct Session {
    runtime: Runtime,
    trace: Option<std::fs::File>,
    snapshot_taken: bool,
    canvas: Canvas,
    opened: Instant,
    on_key: Option<OnKey>,
}

impl Session {
    pub fn new(face: Face) -> Self {
        let runtime = Runtime::new(face);
        let trace = trace_file(runtime.size());
        Session { runtime, trace, snapshot_taken: false, canvas: Canvas::new(0, 0), opened: Instant::now(), on_key: None }
    }

    /// Seconds since the face was shown (the clock animations read).
    pub fn time(&self) -> f64 {
        self.opened.elapsed().as_secs_f64()
    }

    /// Restart the clock (the face was just shown).
    pub fn restart(&mut self) {
        self.opened = Instant::now();
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.runtime.size()
    }

    /// The size in whole device pixels at `scale` (what Windows and X11
    /// hosts size an editor in).
    pub fn pixels(&self, scale: f64) -> (u32, u32) {
        let (w, h) = self.size();
        ((w * scale).round() as u32, (h * scale).round() as u32)
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn cursor(&self) -> Cursor {
        self.runtime.cursor()
    }

    /// Send `act` to the face (the one path every event takes), and
    /// record it when tracing. True when the picture changed.
    pub fn act(&mut self, controller: &dyn Controller, act: Act) -> bool {
        if let Some(file) = self.trace.as_mut() {
            use std::io::Write;
            // A trace is a convenience: a full disk never stops the face.
            let _ = writeln!(file, "{act}");
        }
        self.runtime.apply(controller, &act)
    }

    /// A key: the focused control takes the keys it uses; a stand-alone
    /// window's handler, the rest. `None`: the key belongs to the host.
    /// `Some(true)`: the face was replaced (its size may differ).
    pub fn key(&mut self, controller: &dyn Controller, key: Option<Key>, code: u16, mods: Mods) -> Option<bool> {
        if let Some(key) = key {
            if self.act(controller, Act::Key { key, mods }) {
                return Some(false);
            }
        }
        let face = (self.on_key.as_mut()?)(code);
        let replaced = face.is_some();
        if let Some(face) = face {
            self.runtime = Runtime::new(face);
            self.opened = Instant::now();
        }
        Some(replaced)
    }

    pub fn on_key(&mut self, on_key: OnKey) {
        self.on_key = Some(on_key);
    }

    /// True when something the face shows changed since the last paint.
    pub fn needs_paint(&self, controller: &dyn Controller) -> bool {
        self.runtime.needs_paint(controller, self.time())
    }

    /// Paint the face at `scale` px/pt; the canvas it painted.
    pub fn paint(&mut self, controller: &dyn Controller, scale: f64) -> &Canvas {
        let t = self.time();
        self.runtime.render(controller, scale, t, &mut self.canvas, &mut FontText);
        &self.canvas
    }

    /// The canvas as last painted.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The face as it draws now, offscreen at `scale`, as PNG bytes.
    pub fn snapshot(&mut self, controller: &dyn Controller, scale: f64) -> Vec<u8> {
        let mut canvas = Canvas::new(0, 0);
        let t = self.time();
        self.runtime.render(controller, scale, t, &mut canvas, &mut FontText);
        stave_raster::png::encode(&canvas)
    }

    /// Once the face has run [`SNAPSHOT_AFTER`], write its picture to the
    /// PNG `STAVE_FACE_SNAPSHOT` names (once per face).
    pub fn snapshot_when_settled(&mut self, controller: &dyn Controller, scale: f64) {
        if self.snapshot_taken || self.time() < SNAPSHOT_AFTER {
            return;
        }
        self.snapshot_taken = true;
        if let Some(path) = std::env::var_os("STAVE_FACE_SNAPSHOT") {
            // Best effort: a tool waiting on the file reports its absence.
            let png = self.snapshot(controller, scale);
            let _ = std::fs::write(path, png);
        }
    }
}

/// The trace file `STAVE_FACE_TRACE` names, appended to (a header line
/// marks each face opened).
fn trace_file(size: (f64, f64)) -> Option<std::fs::File> {
    use std::io::Write;
    let path = std::env::var_os("STAVE_FACE_TRACE")?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
    let _ = writeln!(file, "# face {}×{} pt", size.0, size.1);
    Some(file)
}

/// A face's controller and session, as a window's callbacks reach them:
/// a callback that arrives while another runs (a nested event loop) is
/// dropped, and a face that panics goes still — the host lives.
pub struct Shell {
    controller: Box<dyn Controller>,
    session: RefCell<Session>,
    broken: Cell<bool>,
    /// The face's size in points, as of the last callback: readable
    /// while a callback holds the session (a host asking an editor its
    /// size from inside one of the editor's own events).
    size: Cell<(f64, f64)>,
}

impl Shell {
    pub fn new(face: Face, controller: Box<dyn Controller>) -> Self {
        let session = Session::new(face);
        let size = Cell::new(session.size());
        Shell { controller, session: RefCell::new(session), broken: Cell::new(false), size }
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.size.get()
    }

    /// The size in whole device pixels at `scale`.
    pub fn pixels(&self, scale: f64) -> (u32, u32) {
        let (w, h) = self.size();
        ((w * scale).round() as u32, (h * scale).round() as u32)
    }

    pub fn controller(&self) -> &dyn Controller {
        &*self.controller
    }

    /// Run `f` on the face: `None` while another callback runs, or once
    /// face code has panicked (which this catches).
    pub fn with<R>(&self, f: impl FnOnce(&dyn Controller, &mut Session) -> R) -> Option<R> {
        if self.broken.get() {
            return None;
        }
        let mut session = self.session.try_borrow_mut().ok()?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&*self.controller, &mut session)));
        self.broken.set(result.is_err());
        self.size.set(session.size());
        result.ok()
    }

    /// True once face code panicked: the view stays still.
    pub fn broken(&self) -> bool {
        self.broken.get()
    }

    /// The session, outside any callback (setup, queries).
    ///
    /// # Panics
    /// Inside a callback that holds it.
    pub fn session(&self) -> std::cell::RefMut<'_, Session> {
        let s = self.session.borrow_mut();
        self.size.set(s.size());
        s
    }
}

/// Click counting for window systems that leave it to the application
/// (X11; Windows has double-click messages, but no counts past two): a
/// press within [`Clicks::INTERVAL`] and [`Clicks::SLOP`] px of the last
/// counts one more.
#[derive(Default)]
pub struct Clicks {
    last: Option<(Instant, Pt, u32)>,
}

impl Clicks {
    pub const INTERVAL: Duration = Duration::from_millis(400);
    pub const SLOP: f64 = 4.0;

    /// The click count of a press at `at`, now.
    pub fn press(&mut self, at: Pt) -> u32 {
        self.press_at(Instant::now(), at)
    }

    /// The click count of a press at `at` at time `now`.
    pub fn press_at(&mut self, now: Instant, at: Pt) -> u32 {
        let n = match self.last {
            Some((t, p, n)) if now.duration_since(t) <= Self::INTERVAL && (p.x - at.x).abs() <= Self::SLOP && (p.y - at.y).abs() <= Self::SLOP => n + 1,
            _ => 1,
        };
        self.last = Some((now, at, n));
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_count_up_close_and_quick_only() {
        let mut c = Clicks::default();
        let t = Instant::now();
        let p = Pt { x: 10.0, y: 10.0 };
        assert_eq!(c.press_at(t, p), 1);
        assert_eq!(c.press_at(t + Duration::from_millis(200), p), 2);
        assert_eq!(c.press_at(t + Duration::from_millis(300), Pt { x: 12.0, y: 9.0 }), 3);
        assert_eq!(c.press_at(t + Duration::from_millis(400), Pt { x: 30.0, y: 9.0 }), 1, "moved away");
        assert_eq!(c.press_at(t + Duration::from_millis(1000), Pt { x: 30.0, y: 9.0 }), 1, "too slow");
    }
}
