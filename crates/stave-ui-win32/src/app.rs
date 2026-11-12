//! A stand-alone window around one [`FaceView`] (the storybook): a
//! top-level window whose client area is the face, and the message
//! loop a host would run.

use crate::sys::*;
use crate::FaceView;

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        // SAFETY: no arguments beyond the exit code; on the window's thread.
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    // SAFETY: the default procedure, with the message as given.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// The outer size of a top-level window whose client area is `(w, h)`.
fn outer(w: u32, h: u32) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
    // SAFETY: `r` is ours; plain arithmetic on the style's frame.
    unsafe { AdjustWindowRectEx(&mut r, WS_OVERLAPPEDWINDOW, 0, 0) };
    (r.right - r.left, r.bottom - r.top)
}

/// Show `view` in a window titled `title` until the window is closed.
///
/// # Panics
/// When the system refuses the window.
pub fn run_app(title: &str, mut view: FaceView) -> ! {
    let class = wide("StaveFaceApp");
    let title = wide(title);
    let wc = WNDCLASSEXW {
        cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(frame_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: core::ptr::null_mut(),
        hIcon: core::ptr::null_mut(),
        // SAFETY: a system cursor.
        hCursor: unsafe { LoadCursorW(core::ptr::null_mut(), IDC_ARROW as *const u16) },
        hbrBackground: core::ptr::null_mut(),
        lpszMenuName: core::ptr::null(),
        lpszClassName: class.as_ptr(),
        hIconSm: core::ptr::null_mut(),
    };
    let (w, h) = view.host_size();
    let (ow, oh) = outer(w, h);
    // SAFETY: a filled class description (its strings outlive the
    // calls); the window is the process's until it exits.
    let frame = unsafe {
        RegisterClassExW(&wc);
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ow,
            oh,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    };
    assert!(!frame.is_null(), "a top-level window");
    // SAFETY: the live frame window of this thread.
    unsafe { view.attach(frame) };
    // The view took the frame's DPI: size the frame to it.
    let mut size = view.host_size();
    let (ow, oh) = outer(size.0, size.1);
    let mut msg = MSG { hwnd: core::ptr::null_mut(), message: 0, wParam: 0, lParam: 0, time: 0, pt: POINT::default(), lPrivate: 0 };
    // SAFETY: the live frame; `msg` is ours for the loop.
    unsafe {
        SetWindowPos(frame, core::ptr::null_mut(), 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER);
        ShowWindow(frame, SW_SHOW);
        while GetMessageW(&mut msg, core::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            // A face that replaced itself (a key) may have changed size.
            if view.host_size() != size {
                size = view.host_size();
                let (ow, oh) = outer(size.0, size.1);
                SetWindowPos(frame, core::ptr::null_mut(), 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER);
            }
        }
    }
    drop(view);
    std::process::exit(0)
}
