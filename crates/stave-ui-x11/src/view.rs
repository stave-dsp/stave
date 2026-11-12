//! [`FaceView`]: a `stave_face::Face` in one X11 window the size of the
//! face, a child of the host's. It paints the face runtime's canvas
//! (blitted with `PutImage`), forwards the pointer (press, drag, release,
//! wheel, hover, leave) and keys with their modifiers and a counted click
//! — each as a `stave_face::Act` — sets the cursor the runtime asks for,
//! and repaints when the runtime says something changed. Everything the
//! face reads and writes goes through its controller.
//!
//! The view has its own connection to the display (the parent is named
//! by id, which every connection shares). It never blocks and starts no
//! thread: the host's run loop calls [`FaceView::pump`].

use std::cell::RefCell;
use std::os::fd::{AsRawFd, RawFd};

use stave_core::Controller;
use stave_face::{Act, Cursor, Face, Key, Mods};
use stave_ui::scene::{pt, Pt};
use stave_ui_shell::{Clicks, Shell};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::{ConnectError, ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::xproto::{
    AtomEnum, ButtonPressEvent, ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt as _, CreateGCAux,
    CreateWindowAux, EventMask, ImageFormat, InputFocus, KeyButMask, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::pixels::Format;

/// Why a view could not attach.
#[derive(Debug)]
pub enum AttachError {
    /// No display to connect to (`DISPLAY` unset or unreachable).
    Connect(ConnectError),
    /// The connection failed or the server refused a request.
    Protocol(String),
    /// The screen's visual is not 24/32-bit TrueColor.
    Visual,
}

impl core::fmt::Display for AttachError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AttachError::Connect(e) => write!(f, "no X display: {e}"),
            AttachError::Protocol(e) => write!(f, "X11: {e}"),
            AttachError::Visual => f.write_str("the X screen is not 24- or 32-bit TrueColor"),
        }
    }
}

impl std::error::Error for AttachError {}

impl From<ConnectionError> for AttachError {
    fn from(e: ConnectionError) -> Self {
        AttachError::Protocol(e.to_string())
    }
}
impl From<ReplyError> for AttachError {
    fn from(e: ReplyError) -> Self {
        AttachError::Protocol(e.to_string())
    }
}
impl From<ReplyOrIdError> for AttachError {
    fn from(e: ReplyOrIdError) -> Self {
        AttachError::Protocol(e.to_string())
    }
}

/// A face shown in an X11 window, over its controller. Create it, read
/// its [`host_size`](Self::host_size), [`attach`](Self::attach) it to a
/// parent window, and [`pump`](Self::pump) it from the host's run loop;
/// dropping it (or [`detach`](Self::detach)) destroys the window.
pub struct FaceView {
    shell: Shell,
    /// Device pixels per point: the host's (CLAP `set_scale`), else the
    /// display's `Xft.dpi` / 96 at attach, else 1.
    scale: Option<f64>,
    /// Borrowed by `pump` (which a host may call while it is asking the
    /// view something else: everything else only reads).
    mount: RefCell<Option<Mount>>,
}

/// The window on the display, and the pointer's state in it.
struct Mount {
    conn: RustConnection,
    window: Window,
    gc: u32,
    format: Format,
    scale: f64,
    cursors: [u32; 4],
    shown_cursor: Option<Cursor>,
    clicks: Clicks,
    keymap: Keymap,
    pressed: bool,
    /// Exposed or changed: paint at the end of this pump.
    dirty: bool,
    /// Reused `PutImage` bytes.
    bytes: Vec<u8>,
}

impl FaceView {
    /// `face` over `controller`. Touches no display until `attach`.
    pub fn new(face: Face, controller: Box<dyn Controller>) -> Self {
        FaceView { shell: Shell::new(face, controller), scale: None, mount: RefCell::new(None) }
    }

    /// Hand every key press to `on_key` (a stand-alone window's keys; a
    /// plugin window's belong to the host). The code is the keysym's low
    /// 16 bits ([`keys`](crate::keys)).
    pub fn on_key(&mut self, on_key: impl FnMut(u16) -> Option<Face> + 'static) {
        self.shell.session().on_key(Box::new(on_key));
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.shell.size()
    }

    /// The device pixels per point the view paints at.
    pub fn scale(&self) -> f64 {
        self.scale.or_else(|| self.mount.try_borrow().ok()?.as_ref().map(|m| m.scale)).unwrap_or(1.0)
    }

    /// Paint at `scale` px/pt (the host's choice; CLAP `set_scale`).
    pub fn set_scale(&mut self, scale: f64) {
        if !(scale.is_finite() && scale > 0.0) {
            return;
        }
        self.scale = Some(scale);
        if let Some(m) = self.mount.get_mut().as_mut() {
            m.scale = scale;
            let (w, h) = self.shell.pixels(scale);
            let _ = m.conn.configure_window(m.window, &ConfigureWindowAux::new().width(w).height(h));
            m.dirty = true;
            let _ = m.conn.flush();
        }
    }

    /// The size in device pixels (the unit X11 hosts size editors in).
    pub fn host_size(&self) -> (u32, u32) {
        self.shell.pixels(self.scale())
    }

    /// The controller the face reads and writes.
    pub fn controller(&self) -> &dyn Controller {
        self.shell.controller()
    }

    /// The face as it draws now, offscreen at `scale` px/pt, as PNG bytes.
    pub fn snapshot(&self, scale: f64) -> Vec<u8> {
        self.shell.session().snapshot(self.shell.controller(), scale)
    }

    /// The canvas as last painted: (width, height, premultiplied RGBA8).
    /// For tests that read the window back.
    #[doc(hidden)]
    pub fn painted(&self) -> (usize, usize, Vec<u8>) {
        let s = self.shell.session();
        (s.canvas().w(), s.canvas().h(), s.canvas().pixels().to_vec())
    }

    /// The view's window, while attached.
    pub fn window(&self) -> Option<u32> {
        self.mount.try_borrow().ok()?.as_ref().map(|m| m.window)
    }

    /// The display connection's descriptor, while attached: the host's
    /// run loop calls [`pump`](Self::pump) when it is readable.
    pub fn fd(&self) -> Option<RawFd> {
        self.mount.try_borrow().ok()?.as_ref().map(|m| m.conn.stream().as_raw_fd())
    }

    /// Show the face in `parent` (an X11 window id), replacing any
    /// earlier attachment. Connects to `DISPLAY`.
    pub fn attach(&mut self, parent: u32) -> Result<(), AttachError> {
        self.detach();
        let (conn, screen) = x11rb::connect(None).map_err(AttachError::Connect)?;
        let screen = &conn.setup().roots[screen];
        let format = Format::of(conn.setup(), screen).ok_or(AttachError::Visual)?;
        let scale = self.scale.or_else(|| display_scale(&conn, screen.root)).unwrap_or(1.0);
        let (w, h) = self.shell.pixels(scale);
        let window = conn.generate_id()?;
        let events = EventMask::EXPOSURE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::LEAVE_WINDOW
            | EventMask::KEY_PRESS
            | EventMask::STRUCTURE_NOTIFY;
        // Depth, visual and colormap are the screen's own, stated (a
        // parent of another depth would refuse CopyFromParent).
        let aux = CreateWindowAux::new()
            .event_mask(events)
            .background_pixel(0)
            .border_pixel(0)
            .colormap(screen.default_colormap);
        conn.create_window(
            format.depth,
            window,
            parent,
            0,
            0,
            w.max(1) as u16,
            h.max(1) as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &aux,
        )?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, window, &CreateGCAux::new().graphics_exposures(0))?;
        let cursors = cursors(&conn)?;
        let keymap = Keymap::read(&conn)?;
        conn.map_window(window)?;
        conn.flush()?;
        self.shell.session().restart();
        *self.mount.get_mut() = Some(Mount {
            conn,
            window,
            gc,
            format,
            scale,
            cursors,
            shown_cursor: None,
            clicks: Clicks::default(),
            keymap,
            pressed: false,
            dirty: true,
            bytes: Vec::new(),
        });
        Ok(())
    }

    /// Destroy the window and close the connection.
    pub fn detach(&mut self) {
        if let Some(m) = self.mount.get_mut().take() {
            let _ = m.conn.destroy_window(m.window);
            let _ = m.conn.flush();
        }
    }

    /// Handle every event that has arrived, and repaint if the face
    /// changed (call it when [`fd`](Self::fd) is readable and on a
    /// ~60 Hz timer). Never blocks. False when the connection is gone
    /// (the view then stays still until attached again).
    pub fn pump(&self) -> bool {
        // A pump from inside a pump (the host, called by the face, calls
        // back) waits for the next one.
        let Ok(mut mount) = self.mount.try_borrow_mut() else {
            return true;
        };
        let Some(m) = mount.as_mut() else {
            return false;
        };
        loop {
            match m.conn.poll_for_event() {
                Ok(Some(event)) => m.handle(&self.shell, event),
                Ok(None) => break,
                Err(_) => return false,
            }
        }
        let scale = m.scale;
        let repaint = self
            .shell
            .with(|controller, session| {
                session.snapshot_when_settled(controller, scale);
                session.needs_paint(controller)
            })
            .unwrap_or(false);
        if repaint || m.dirty {
            m.dirty = false;
            m.paint(&self.shell);
        }
        m.conn.flush().is_ok()
    }
}

impl Drop for FaceView {
    fn drop(&mut self) {
        self.detach();
    }
}

impl Mount {
    /// Where an event happened, in face points.
    fn point(&self, x: i16, y: i16) -> Pt {
        pt(f64::from(x) / self.scale, f64::from(y) / self.scale)
    }

    fn handle(&mut self, shell: &Shell, event: Event) {
        match event {
            Event::Expose(e) if e.window == self.window => self.dirty = true,
            Event::ButtonPress(e) if e.event == self.window => self.press(shell, &e),
            Event::ButtonRelease(e) if e.event == self.window && e.detail == 1 => {
                self.pressed = false;
                let p = self.point(e.event_x, e.event_y);
                shell.with(|c, s| {
                    s.act(c, Act::Release);
                    s.act(c, Act::Hover(p));
                });
                self.dirty = true;
            }
            Event::MotionNotify(e) if e.event == self.window => {
                let (p, mods) = (self.point(e.event_x, e.event_y), mods(e.state));
                let act = if self.pressed { Act::Move { at: p, mods } } else { Act::Hover(p) };
                if shell.with(|c, s| s.act(c, act)) == Some(true) || self.pressed {
                    self.dirty = true;
                }
            }
            Event::LeaveNotify(e) if e.event == self.window && !self.pressed => {
                shell.with(|c, s| s.act(c, Act::Exit));
                self.dirty = true;
            }
            Event::KeyPress(e) if e.event == self.window => {
                let keysym = self.keymap.keysym(e.detail);
                let resized = shell.with(|c, s| s.key(c, face_key(keysym), keysym as u16, mods(e.state))).flatten();
                if resized == Some(true) {
                    let (w, h) = shell.pixels(self.scale);
                    let _ = self.conn.configure_window(self.window, &ConfigureWindowAux::new().width(w).height(h));
                }
                self.dirty = true;
            }
            _ => {}
        }
        let cursor = shell.with(|_, s| s.cursor());
        if let Some(cursor) = cursor.filter(|c| Some(*c) != self.shown_cursor) {
            self.shown_cursor = Some(cursor);
            let id = self.cursors[cursor_index(cursor)];
            let _ = self.conn.change_window_attributes(self.window, &ChangeWindowAttributesAux::new().cursor(id));
        }
    }

    fn press(&mut self, shell: &Shell, e: &ButtonPressEvent) {
        let (p, mods) = (self.point(e.event_x, e.event_y), mods(e.state));
        match e.detail {
            1 => {
                self.pressed = true;
                let clicks = self.clicks.press(p);
                let _ = self.conn.set_input_focus(InputFocus::PARENT, self.window, x11rb::CURRENT_TIME);
                shell.with(|c, s| s.act(c, Act::Press { at: p, mods, clicks }));
            }
            // Buttons 4 and 5 are the wheel's notches, up and down.
            4 | 5 => {
                let lines = if e.detail == 4 { 1.0 } else { -1.0 };
                shell.with(|c, s| s.act(c, Act::Wheel { at: p, lines, mods }));
            }
            _ => return,
        }
        self.dirty = true;
    }

    /// Paint the face and put it in the window, in requests the server
    /// accepts (a tall face is several `PutImage`s).
    fn paint(&mut self, shell: &Shell) {
        let scale = self.scale;
        let (format, conn, window, gc, bytes) = (self.format, &self.conn, self.window, self.gc, &mut self.bytes);
        shell.with(|controller, session| {
            let canvas = session.paint(controller, scale);
            let (w, h) = (canvas.w(), canvas.h());
            if w == 0 || h == 0 {
                return;
            }
            let room = conn.maximum_request_bytes().saturating_sub(64);
            let rows_per = (room / (w * 4)).clamp(1, h);
            let mut y = 0;
            while y < h {
                let rows = y..(y + rows_per).min(h);
                format.convert(canvas.pixels(), w, rows.clone(), bytes);
                let _ = conn.put_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    gc,
                    w as u16,
                    rows.len() as u16,
                    0,
                    y as i16,
                    0,
                    format.depth,
                    bytes,
                );
                y = rows.end;
            }
        });
    }
}

/// The face's modifiers from an X11 state: Shift or Control is fine,
/// Alt (Mod1) resets (macOS's ⇧/⌘ and ⌥).
fn mods(state: KeyButMask) -> Mods {
    Mods {
        fine: state.contains(KeyButMask::SHIFT) || state.contains(KeyButMask::CONTROL),
        reset: state.contains(KeyButMask::MOD1),
    }
}

/// The face's keys, from X11 keysyms.
fn face_key(keysym: u32) -> Option<Key> {
    Some(match keysym {
        0xff51 => Key::Left,
        0xff52 => Key::Up,
        0xff53 => Key::Right,
        0xff54 => Key::Down,
        0xff50 => Key::Home,
        0xff57 => Key::End,
        0xff1b => Key::Escape,
        _ => return None,
    })
}

fn cursor_index(c: Cursor) -> usize {
    match c {
        Cursor::Arrow => 0,
        Cursor::Point => 1,
        Cursor::Grab => 2,
        Cursor::Grabbing => 3,
    }
}

/// The four cursors, from the core cursor font: left_ptr, hand2, hand1,
/// fleur.
fn cursors(conn: &RustConnection) -> Result<[u32; 4], ReplyOrIdError> {
    let font = conn.generate_id()?;
    conn.open_font(font, b"cursor")?;
    let mut out = [0; 4];
    for (slot, glyph) in out.iter_mut().zip([68u16, 60, 58, 52]) {
        *slot = conn.generate_id()?;
        conn.create_glyph_cursor(*slot, font, font, glyph, glyph + 1, 0, 0, 0, 0xffff, 0xffff, 0xffff)?;
    }
    conn.close_font(font)?;
    Ok(out)
}

/// The display's scale: `Xft.dpi` from the root window's resources over
/// 96, when set.
fn display_scale(conn: &RustConnection, root: Window) -> Option<f64> {
    let reply = conn
        .get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 16)
        .ok()?
        .reply()
        .ok()?;
    xft_dpi(&reply.value).map(|dpi| dpi / 96.0)
}

/// `Xft.dpi` in an X resource database.
pub(crate) fn xft_dpi(resources: &[u8]) -> Option<f64> {
    let text = core::str::from_utf8(resources).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d >= 48.0 && *d <= 960.0)
}

/// Keycode → keysym (the first, unshifted, column).
struct Keymap {
    min: u8,
    per: usize,
    syms: Vec<u32>,
}

impl Keymap {
    fn read(conn: &RustConnection) -> Result<Self, ReplyError> {
        let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let reply = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
        Ok(Keymap { min, per: usize::from(reply.keysyms_per_keycode), syms: reply.keysyms })
    }

    fn keysym(&self, code: u8) -> u32 {
        let i = usize::from(code.saturating_sub(self.min)) * self.per;
        self.syms.get(i).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xft_dpi_is_read_from_the_resources() {
        assert_eq!(xft_dpi(b"Xft.antialias:\t1\nXft.dpi:\t192\n"), Some(192.0));
        assert_eq!(xft_dpi(b"Xft.antialias:\t1\n"), None);
        assert_eq!(xft_dpi(b"Xft.dpi: lots\n"), None);
    }

    #[test]
    fn alt_resets_and_shift_or_control_is_fine() {
        assert_eq!(mods(KeyButMask::SHIFT), Mods { fine: true, reset: false });
        assert_eq!(mods(KeyButMask::CONTROL), Mods { fine: true, reset: false });
        assert_eq!(mods(KeyButMask::MOD1), Mods { fine: false, reset: true });
    }
}
