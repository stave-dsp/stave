//! A stand-alone window around one [`FaceView`] (the storybook): a
//! top-level window the face's size, and the loop a host would run —
//! pump the view, about 120 times a second.

use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

use crate::FaceView;

/// Show `view` in a window titled `title` until the window is closed;
/// then drop it and exit.
///
/// # Panics
/// When there is no X display.
pub fn run_app(title: &str, mut view: FaceView) -> ! {
    let (conn, screen) = x11rb::connect(None).expect("an X display (DISPLAY)");
    let screen = &conn.setup().roots[screen];
    let (w, h) = view.host_size();
    let window = conn.generate_id().expect("an X id");
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        screen.root,
        0,
        0,
        w as u16,
        h as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY).background_pixel(screen.black_pixel),
    )
    .expect("a top-level window");
    let _ = conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes());
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS").ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let delete = conn.intern_atom(false, b"WM_DELETE_WINDOW").ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    if let (Some(p), Some(d)) = (protocols, delete) {
        let _ = conn.change_property32(PropMode::REPLACE, window, p, AtomEnum::ATOM, &[d]);
    }
    conn.map_window(window).expect("map");
    conn.flush().expect("flush");
    view.attach(window).expect("the face attaches");
    let mut size = (w, h);
    'run: loop {
        while let Ok(Some(event)) = conn.poll_for_event() {
            match event {
                Event::ClientMessage(e) if Some(e.data.as_data32()[0]) == delete => break 'run,
                Event::DestroyNotify(_) => break 'run,
                _ => {}
            }
        }
        if !view.pump() {
            std::process::exit(1);
        }
        // A face that replaced itself (a key) may have changed size.
        if view.host_size() != size {
            size = view.host_size();
            let aux = x11rb::protocol::xproto::ConfigureWindowAux::new().width(size.0).height(size.1);
            let _ = conn.configure_window(window, &aux);
            let _ = conn.flush();
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    // The controller's destructor runs (it may save) before the exit.
    drop(view);
    std::process::exit(0)
}
