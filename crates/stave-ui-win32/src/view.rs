//! [`FaceView`]: a `stave_face::Face` in one child window (HWND) the
//! size of the face, at the parent's DPI. It paints the face runtime's
//! canvas (blitted with `SetDIBitsToDevice`), forwards the pointer
//! (press, drag with capture, release, wheel, hover, leave) and keys
//! with their modifiers and a counted click — each as a
//! `stave_face::Act` — sets the cursor the runtime asks for, and
//! repaints from its own timer when the runtime says something changed.
//!
//! One root reaches the face from the window procedure: the window's
//! `GWLP_USERDATA`, a counted reference to the view's [`Shared`] state.
//! Every message takes its own count for the duration of the call, so a
//! message that (through the host) closes the editor never frees what it
//! is using; a message that arrives while another is being handled (a
//! nested loop) is dropped by the shell; a face that panics goes still.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use stave_core::Controller;
use stave_face::{Act, Cursor, Face, Key, Mods};
use stave_ui::scene::{pt, Pt};
use stave_ui_shell::{Clicks, Shell};

use crate::sys::*;

/// A face shown in a child window, over its controller. Create it, read
/// its [`host_size`](Self::host_size), [`attach`](Self::attach) it to a
/// parent window; dropping it (or [`detach`](Self::detach)) destroys the
/// window.
///
/// Not `Send`: a window lives and dies on the thread that creates it
/// (the host's UI thread).
pub struct FaceView {
    shared: Rc<Shared>,
    hwnd: Option<HWND>,
}

/// What the window procedure reaches.
struct Shared {
    shell: Shell,
    /// The host's scale (CLAP `set_scale`), else the parent's DPI / 96.
    asked: Cell<Option<f64>>,
    scale: Cell<f64>,
    win: RefCell<Win>,
}

/// The pointer's state in the window, and the blit buffer.
#[derive(Default)]
struct Win {
    clicks: Clicks,
    tracking: bool,
    pressed: bool,
    bgra: Vec<u8>,
}

/// The repaint timer's id (one per window).
const TIMER: usize = 1;

impl FaceView {
    /// `face` over `controller`. Creates no window until `attach`.
    pub fn new(face: Face, controller: Box<dyn Controller>) -> Self {
        FaceView {
            shared: Rc::new(Shared {
                shell: Shell::new(face, controller),
                asked: Cell::new(None),
                scale: Cell::new(1.0),
                win: RefCell::new(Win::default()),
            }),
            hwnd: None,
        }
    }

    /// Hand every key press to `on_key` (a stand-alone window's keys; a
    /// plugin window's belong to the host). The code is the Windows
    /// virtual key ([`keys`](crate::keys)).
    pub fn on_key(&mut self, on_key: impl FnMut(u16) -> Option<Face> + 'static) {
        self.shared.shell.session().on_key(Box::new(on_key));
    }

    /// The face's size in points.
    pub fn size(&self) -> (f64, f64) {
        self.shared.shell.size()
    }

    /// The device pixels per point the view paints at.
    pub fn scale(&self) -> f64 {
        self.shared.asked.get().unwrap_or(self.shared.scale.get())
    }

    /// Paint at `scale` px/pt (the host's choice; CLAP `set_scale`).
    pub fn set_scale(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.shared.asked.set(Some(scale));
            self.shared.scale.set(scale);
            if let Some(hwnd) = self.hwnd {
                // SAFETY: our live window, on its thread (`FaceView` is
                // not `Send`).
                unsafe { resize(hwnd, &self.shared) };
            }
        }
    }

    /// The size in device pixels (the unit Windows hosts size editors in).
    pub fn host_size(&self) -> (u32, u32) {
        self.shared.shell.pixels(self.scale())
    }

    /// The controller the face reads and writes.
    pub fn controller(&self) -> &dyn Controller {
        self.shared.shell.controller()
    }

    /// The face as it draws now, offscreen at `scale` px/pt, as PNG bytes.
    pub fn snapshot(&self, scale: f64) -> Vec<u8> {
        self.shared.shell.session().snapshot(self.shared.shell.controller(), scale)
    }

    /// The canvas as last painted: (width, height, premultiplied RGBA8).
    /// For tests that read the window back.
    #[doc(hidden)]
    pub fn painted(&self) -> (usize, usize, Vec<u8>) {
        let s = self.shared.shell.session();
        (s.canvas().w(), s.canvas().h(), s.canvas().pixels().to_vec())
    }

    /// The view's window, while attached.
    pub fn hwnd(&self) -> Option<HWND> {
        self.hwnd
    }

    /// Show the face in `parent` (replacing any earlier attachment). A
    /// null parent, or a window the system refuses, shows nothing.
    ///
    /// # Safety
    /// `parent` is null or a live window of this thread.
    pub unsafe fn attach(&mut self, parent: *mut c_void) {
        self.detach();
        if parent.is_null() {
            return;
        }
        let Some(class) = class() else { return };
        if self.shared.asked.get().is_none() {
            // SAFETY: `parent` is a live window (this function's contract).
            let dpi = unsafe { GetDpiForWindow(parent) };
            self.shared.scale.set(if dpi == 0 { 1.0 } else { f64::from(dpi) / USER_DEFAULT_SCREEN_DPI });
        }
        let (w, h) = self.host_size();
        let style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
        // SAFETY: a registered class, a live parent; the window is ours
        // until `detach` destroys it.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.name.as_ptr(),
                core::ptr::null(),
                style,
                0,
                0,
                w as i32,
                h as i32,
                parent,
                core::ptr::null_mut(),
                class.instance,
                core::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            release_class();
            return;
        }
        self.shared.shell.session().restart();
        // SAFETY: our fresh window; the slot's reference is taken back in
        // `detach` before the window is destroyed.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(Rc::clone(&self.shared)) as isize);
            SetTimer(hwnd, TIMER, 16, core::ptr::null_mut());
        }
        self.hwnd = Some(hwnd);
    }

    /// Destroy the window and stop its timer.
    pub fn detach(&mut self) {
        let Some(hwnd) = self.hwnd.take() else { return };
        // SAFETY: our live window, on its thread. The slot is emptied
        // first, so no message during destruction reaches the state; the
        // reference it held is then released.
        unsafe {
            KillTimer(hwnd, TIMER);
            let slot = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const Shared;
            DestroyWindow(hwnd);
            if !slot.is_null() {
                drop(Rc::from_raw(slot));
            }
        }
        release_class();
    }
}

impl Drop for FaceView {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Size the window to the face at the current scale.
///
/// # Safety
/// `hwnd` is a live face window of this thread.
unsafe fn resize(hwnd: HWND, shared: &Shared) {
    let (w, h) = shared.shell.pixels(shared.scale.get());
    // SAFETY: this function's contract.
    unsafe {
        SetWindowPos(hwnd, core::ptr::null_mut(), 0, 0, w as i32, h as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hwnd, core::ptr::null(), 0);
    }
}

// ------------------------------------------------------------ the class

struct Class {
    name: Vec<u16>,
    instance: HINSTANCE,
}

// SAFETY: the name is immutable; the instance is a module handle, a
// process-wide value any thread may pass to the window functions.
unsafe impl Send for Class {}
// SAFETY: as above.
unsafe impl Sync for Class {}

/// Live face windows of this binary: the class is registered while any
/// exists, and unregistered with the last, so a plugin DLL that unloads
/// leaves no class pointing into its unmapped code.
static WINDOWS: AtomicUsize = AtomicUsize::new(0);

/// The class name and module, decided once per binary: two stave
/// plugins in one host each register their own (the name carries the
/// address of a static in this image).
fn class_info() -> &'static Class {
    static CLASS: OnceLock<Class> = OnceLock::new();
    CLASS.get_or_init(|| {
        static TAG: u8 = 0;
        let mut instance: HINSTANCE = core::ptr::null_mut();
        // SAFETY: FROM_ADDRESS with an address inside this image, and an
        // out-pointer; UNCHANGED_REFCOUNT takes no reference to release.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                (&TAG as *const u8).cast(),
                &mut instance,
            )
        };
        Class { name: wide(&format!("StaveFace{:x}", &TAG as *const u8 as usize)), instance }
    })
}

/// The registered class, for one more window (`None`: refused).
fn class() -> Option<&'static Class> {
    let class = class_info();
    if WINDOWS.fetch_add(1, Ordering::SeqCst) == 0 {
        let wc = WNDCLASSEXW {
            cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: class.instance,
            hIcon: core::ptr::null_mut(),
            hCursor: core::ptr::null_mut(),
            hbrBackground: core::ptr::null_mut(),
            lpszMenuName: core::ptr::null(),
            lpszClassName: class.name.as_ptr(),
            hIconSm: core::ptr::null_mut(),
        };
        // SAFETY: a filled class description whose strings outlive the call.
        if unsafe { RegisterClassExW(&wc) } == 0 {
            WINDOWS.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
    }
    Some(class)
}

/// One window fewer; the last unregisters the class.
fn release_class() {
    if WINDOWS.fetch_sub(1, Ordering::SeqCst) == 1 {
        let class = class_info();
        // SAFETY: our class, with no windows left.
        unsafe { UnregisterClassW(class.name.as_ptr(), class.instance) };
    }
}

// ------------------------------------------------------ the procedure

/// The state of a face window, counted for the caller: `None` once
/// detached (or before the slot is set).
///
/// # Safety
/// `hwnd` is a live window of the face class.
unsafe fn shared(hwnd: HWND) -> Option<Rc<Shared>> {
    // SAFETY: this function's contract.
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
    if p.is_null() {
        return None;
    }
    // SAFETY: a non-null slot holds the view's counted reference, so the
    // count is at least one while we add ours.
    unsafe {
        Rc::increment_strong_count(p);
        Some(Rc::from_raw(p))
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // A panic must not unwind into the system's dispatch (the shell
    // catches the face's own; this catches the view's).
    let handled = std::panic::catch_unwind(|| {
        // SAFETY: the system calls the procedure with a live window of
        // our class, on that window's thread.
        let shared = unsafe { shared(hwnd) }?;
        // SAFETY: as above.
        unsafe { handle(hwnd, &shared, msg, w, l) }
    });
    match handled {
        Ok(Some(result)) => result,
        // SAFETY: the default procedure, with the message as given.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// The face's modifiers: Shift or Control is fine, Alt resets (macOS's
/// ⇧/⌘ and ⌥).
fn mods(w: WPARAM) -> Mods {
    // SAFETY: reads this thread's keyboard state.
    let alt = unsafe { GetKeyState(VK_MENU) } < 0;
    Mods { fine: w & (MK_SHIFT | MK_CONTROL) != 0, reset: alt }
}

/// The modifiers of a key message (no MK_ flags there).
fn key_mods() -> Mods {
    // SAFETY: reads this thread's keyboard state.
    let down = |k| unsafe { GetKeyState(k) } < 0;
    Mods { fine: down(VK_SHIFT) || down(VK_CONTROL), reset: down(VK_MENU) }
}

fn face_key(vk: u16) -> Option<Key> {
    Some(match vk {
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_ESCAPE => Key::Escape,
        _ => return None,
    })
}

/// Handle one message; `None` sends it to the default procedure.
///
/// # Safety
/// `hwnd` is the live face window `shared` belongs to, on its thread.
unsafe fn handle(hwnd: HWND, shared: &Shared, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    let shell = &shared.shell;
    let scale = shared.scale.get();
    let at = |l: LPARAM| -> Pt {
        let (x, y) = xy_lparam(l);
        pt(f64::from(x) / scale, f64::from(y) / scale)
    };
    // SAFETY: (for every call below) the live window, on its thread.
    let redraw = || unsafe { InvalidateRect(hwnd, core::ptr::null(), 0) };
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = PAINTSTRUCT { hdc: core::ptr::null_mut(), fErase: 0, rcPaint: RECT::default(), fRestore: 0, fIncUpdate: 0, rgbReserved: [0; 32] };
            // SAFETY: the window's paint cycle; `ps` is ours.
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            if !hdc.is_null() {
                // SAFETY: the paint DC of this window, during the cycle.
                unsafe { blit(hdc, shared) };
            }
            // SAFETY: closes the cycle `BeginPaint` opened.
            unsafe { EndPaint(hwnd, &ps) };
            Some(0)
        }
        WM_PRINTCLIENT => {
            // SAFETY: the caller's DC (WM_PRINTCLIENT's contract).
            unsafe { blit(w as HDC, shared) };
            Some(0)
        }
        WM_ERASEBKGND => Some(1),
        WM_NCDESTROY => {
            // Destroyed under us (the host destroyed the parent first):
            // release the slot's reference; this call holds its own.
            // SAFETY: as above; the slot held the view's counted reference.
            unsafe {
                let slot = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const Shared;
                if !slot.is_null() {
                    drop(Rc::from_raw(slot));
                }
            }
            None
        }
        WM_TIMER if w == TIMER => {
            let dirty = shell.with(|c, s| {
                s.snapshot_when_settled(c, scale);
                s.needs_paint(c)
            });
            if dirty == Some(true) {
                redraw();
            }
            Some(0)
        }
        WM_LBUTTONDOWN => {
            let p = at(l);
            let clicks = shared.win.borrow_mut().clicks.press(p);
            shared.win.borrow_mut().pressed = true;
            // SAFETY: as above.
            unsafe {
                SetCapture(hwnd);
                SetFocus(hwnd);
            }
            shell.with(|c, s| s.act(c, Act::Press { at: p, mods: mods(w), clicks }));
            set_cursor(shared);
            redraw();
            Some(0)
        }
        WM_MOUSEMOVE => {
            let p = at(l);
            let pressed = shared.win.borrow().pressed;
            if !pressed && !shared.win.borrow().tracking {
                let mut t = TRACKMOUSEEVENT { cbSize: core::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                // SAFETY: a filled request for this window.
                shared.win.borrow_mut().tracking = unsafe { TrackMouseEvent(&mut t) } != 0;
            }
            let act = if pressed { Act::Move { at: p, mods: mods(w) } } else { Act::Hover(p) };
            if shell.with(|c, s| s.act(c, act)) == Some(true) || pressed {
                redraw();
            }
            set_cursor(shared);
            Some(0)
        }
        WM_LBUTTONUP => {
            let p = at(l);
            let was = core::mem::replace(&mut shared.win.borrow_mut().pressed, false);
            // SAFETY: as above.
            unsafe { ReleaseCapture() };
            if was {
                shell.with(|c, s| {
                    s.act(c, Act::Release);
                    s.act(c, Act::Hover(p));
                });
            }
            set_cursor(shared);
            redraw();
            Some(0)
        }
        WM_CAPTURECHANGED => {
            // Capture taken away mid-drag (a host dialog): end the drag.
            if core::mem::replace(&mut shared.win.borrow_mut().pressed, false) {
                shell.with(|c, s| s.act(c, Act::Release));
                redraw();
            }
            Some(0)
        }
        WM_MOUSELEAVE => {
            shared.win.borrow_mut().tracking = false;
            if !shared.win.borrow().pressed {
                shell.with(|c, s| s.act(c, Act::Exit));
                redraw();
            }
            Some(0)
        }
        WM_MOUSEWHEEL => {
            // Screen coordinates, and the delta in 120ths of a notch.
            let (x, y) = xy_lparam(l);
            let mut p = POINT { x, y };
            // SAFETY: as above; `p` is ours.
            unsafe { ScreenToClient(hwnd, &mut p) };
            let lines = f64::from((w >> 16) as u16 as i16) / WHEEL_DELTA;
            let at = pt(f64::from(p.x) / scale, f64::from(p.y) / scale);
            if lines != 0.0 {
                shell.with(|c, s| s.act(c, Act::Wheel { at, lines, mods: mods(w & 0xffff) }));
                redraw();
            }
            Some(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = w as u16;
            let result = shell.with(|c, s| s.key(c, face_key(vk), vk, key_mods())).flatten();
            match result {
                Some(replaced) => {
                    if replaced {
                        // SAFETY: as above.
                        unsafe { resize(hwnd, shared) };
                    }
                    redraw();
                    Some(0)
                }
                None => None,
            }
        }
        // Arrows and characters come to the face, not a dialog's focus
        // navigation.
        WM_GETDLGCODE => Some(DLGC_WANTARROWS | DLGC_WANTCHARS),
        WM_SETCURSOR if l as u16 == HTCLIENT => {
            set_cursor(shared);
            Some(1)
        }
        WM_DPICHANGED_AFTERPARENT => {
            if shared.asked.get().is_none() {
                // SAFETY: as above.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                if dpi != 0 {
                    shared.scale.set(f64::from(dpi) / USER_DEFAULT_SCREEN_DPI);
                    // SAFETY: as above.
                    unsafe { resize(hwnd, shared) };
                }
            }
            Some(0)
        }
        _ => None,
    }
}

fn set_cursor(shared: &Shared) {
    let Some(c) = shared.shell.with(|_, s| s.cursor()) else { return };
    let id = match c {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Point | Cursor::Grab => IDC_HAND,
        Cursor::Grabbing => IDC_SIZEALL,
    };
    // SAFETY: a system cursor (null instance, an IDC_ id as the name).
    unsafe { SetCursor(LoadCursorW(core::ptr::null_mut(), id as *const u16)) };
}

/// Paint the face and copy it into `hdc` at the origin.
///
/// # Safety
/// `hdc` is a live device context.
unsafe fn blit(hdc: HDC, shared: &Shared) {
    let scale = shared.scale.get();
    let Ok(mut win) = shared.win.try_borrow_mut() else { return };
    let win = &mut *win;
    shared.shell.with(|c, s| {
        let canvas = s.paint(c, scale);
        let (w, h) = (canvas.w(), canvas.h());
        if w == 0 || h == 0 {
            return;
        }
        to_bgra(canvas.pixels(), &mut win.bgra);
        let info = BITMAPINFO::top_down(w as i32, h as i32);
        // SAFETY: `bgra` holds w × h × 4 bytes, the bitmap `info` describes;
        // `hdc` is live (this function's contract).
        unsafe {
            SetDIBitsToDevice(hdc, 0, 0, w as u32, h as u32, 0, 0, 0, h as u32, win.bgra.as_ptr().cast(), &info, DIB_RGB_COLORS)
        };
    });
}

/// Premultiplied RGBA8 as GDI's BGRX (shown over black).
pub(crate) fn to_bgra(rgba: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(rgba.len());
    for p in rgba.chunks_exact(4) {
        out.extend_from_slice(&[p[2], p[1], p[0], 0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdi_pixels_are_bgr() {
        let mut out = Vec::new();
        to_bgra(&[10, 20, 30, 255, 1, 2, 3, 4], &mut out);
        assert_eq!(out, [30, 20, 10, 0, 3, 2, 1, 0]);
    }

    #[test]
    fn mouse_coordinates_are_signed_words() {
        assert_eq!(xy_lparam(lparam_xy(12, 34)), (12, 34));
        assert_eq!(xy_lparam(lparam_xy(-5, -7)), (-5, -7));
    }
}
