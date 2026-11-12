//! The few Win32 declarations the view uses (user32, gdi32, kernel32),
//! with their documented x64/arm64 types. Public for the tests that
//! drive a view with real window messages.

#![allow(non_snake_case, clippy::upper_case_acronyms, missing_docs)]

use core::ffi::c_void;

pub type HWND = *mut c_void;
pub type HDC = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: Option<WNDPROC>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: *mut c_void,
    pub hCursor: HCURSOR,
    pub hbrBackground: *mut c_void,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: *mut c_void,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: i32,
    pub rcPaint: RECT,
    pub fRestore: i32,
    pub fIncUpdate: i32,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}

/// `BITMAPINFO` with its one (unused, for 32 bpp) colour entry.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [u32; 1],
}

impl BITMAPINFO {
    /// A top-down 32-bit BGRX bitmap of `w` × `h`.
    pub fn top_down(w: i32, h: i32) -> Self {
        BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [0],
        }
    }
}

#[repr(C)]
pub struct TRACKMOUSEEVENT {
    pub cbSize: u32,
    pub dwFlags: u32,
    pub hwndTrack: HWND,
    pub dwHoverTime: u32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_GETDLGCODE: u32 = 0x0087;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_DPICHANGED_AFTERPARENT: u32 = 0x02E3;
pub const WM_PRINTCLIENT: u32 = 0x0318;

pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const CW_USEDEFAULT: i32 = 0x8000_0000_u32 as i32;
pub const SW_SHOW: i32 = 5;

pub const GWLP_USERDATA: i32 = -21;
pub const IDC_ARROW: usize = 32512;
pub const IDC_HAND: usize = 32649;
pub const IDC_SIZEALL: usize = 32646;
pub const HTCLIENT: u16 = 1;
pub const TME_LEAVE: u32 = 0x2;
pub const DLGC_WANTARROWS: LRESULT = 0x1;
pub const DLGC_WANTCHARS: LRESULT = 0x80;
pub const MK_SHIFT: usize = 0x4;
pub const MK_CONTROL: usize = 0x8;
pub const MK_LBUTTON: usize = 0x1;
pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_MENU: i32 = 0x12;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const WHEEL_DELTA: f64 = 120.0;
pub const DIB_RGB_COLORS: u32 = 0;
pub const BI_RGB: u32 = 0;
pub const PRF_CLIENT: LPARAM = 0x4;
pub const SWP_NOMOVE: u32 = 0x2;
pub const SWP_NOZORDER: u32 = 0x4;
pub const SWP_NOACTIVATE: u32 = 0x10;
pub const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
pub const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;
pub const USER_DEFAULT_SCREEN_DPI: f64 = 96.0;

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn UnregisterClassW(name: *const u16, instance: HINSTANCE) -> i32;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: *mut c_void,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(hwnd: HWND) -> i32;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn SendMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> i32;
    pub fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: i32) -> i32;
    pub fn SetTimer(hwnd: HWND, id: usize, ms: u32, f: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> i32;
    pub fn SetCapture(hwnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> i32;
    pub fn GetCapture() -> HWND;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn TrackMouseEvent(t: *mut TRACKMOUSEEVENT) -> i32;
    pub fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
    pub fn SetCursor(cursor: HCURSOR) -> HCURSOR;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn GetKeyboardState(state: *mut u8) -> i32;
    pub fn SetKeyboardState(state: *const u8) -> i32;
    pub fn ScreenToClient(hwnd: HWND, p: *mut POINT) -> i32;
    pub fn ClientToScreen(hwnd: HWND, p: *mut POINT) -> i32;
    pub fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    pub fn GetClientRect(hwnd: HWND, r: *mut RECT) -> i32;
    pub fn AdjustWindowRectEx(r: *mut RECT, style: u32, menu: i32, ex_style: u32) -> i32;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const MSG) -> i32;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
}

#[link(name = "gdi32")]
extern "system" {
    pub fn SetDIBitsToDevice(
        hdc: HDC,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        src_x: i32,
        src_y: i32,
        start_scan: u32,
        lines: u32,
        bits: *const c_void,
        info: *const BITMAPINFO,
        usage: u32,
    ) -> i32;
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn CreateDIBSection(
        hdc: HDC,
        info: *const BITMAPINFO,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> HGDIOBJ;
    pub fn SelectObject(hdc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(obj: HGDIOBJ) -> i32;
    pub fn DeleteDC(hdc: HDC) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut HINSTANCE) -> i32;
}

/// `MAKELPARAM(x, y)`: client coordinates in a mouse message.
pub fn lparam_xy(x: i32, y: i32) -> LPARAM {
    ((y as u16 as u32) << 16 | x as u16 as u32) as i32 as LPARAM
}

/// `GET_X_LPARAM` / `GET_Y_LPARAM`: signed client coordinates.
pub fn xy_lparam(l: LPARAM) -> (i32, i32) {
    (i32::from(l as u16 as i16), i32::from((l >> 16) as u16 as i16))
}

/// A Rust string as nul-terminated UTF-16.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
