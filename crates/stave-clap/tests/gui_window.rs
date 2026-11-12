//! `clap.gui` in a real parent window, the way a Linux or Windows host
//! drives it: on X11 (a real server — Xvfb in CI) `set_parent` makes the
//! face's child window and joins the host's run loop through
//! `clap.timer-support` and `clap.posix-fd-support`, the host's timer
//! pumps it, and `destroy` leaves the loop and the window; on Windows it
//! makes a child HWND and `destroy` removes it. (macOS: ci.yml's real
//! window tests in stave-ui-cocoa.)
//!
//! Without `DISPLAY` the X11 test passes vacuously, unless
//! `STAVE_REQUIRE_DISPLAY` is set (CI sets it).

#![allow(clippy::undocumented_unsafe_blocks)]

use std::sync::Arc;

use stave_clap::sys::*;
use stave_core::params::{FloatParam, Params, Range};
use stave_core::runtime::Erased;
use stave_core::{Block, Info, ParamId, Telemetry};

#[derive(stave_core::Params)]
struct OneKnob {
    drive: FloatParam,
}

struct Fixture {
    params: Arc<OneKnob>,
}

impl Erased for Fixture {
    const INFO: Info = Info {
        id: "com.stave.test.gui-window",
        name: "GUI Window",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "clap.gui fixture",
        features: &[],
    };
    fn new(_telemetry: &Telemetry) -> Self {
        Fixture { params: Arc::new(OneKnob { drive: FloatParam::new(ParamId(1), "drive", Range::linear(0.0, 1.0)) }) }
    }
    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }
    fn process(&mut self, _block: &mut Block<'_>) {}
}

impl stave_clap::wrapper::ClapExport for Fixture {
    fn face(&self) -> Option<stave_clap::stave_face::Face> {
        use stave_clap::stave_face::prelude::*;
        Some(Face::new(Theme::graphite(), Knob::new(self.params.drive.bind()).node()))
    }
}

stave_clap::export_clap!(Fixture);

/// A plugin created by `host` (null: no host extensions at all).
#[cfg_attr(target_os = "macos", allow(dead_code))]
unsafe fn create(host: *const ClapHost) -> *const ClapPlugin {
    let get_factory = clap_entry.get_factory.expect("get_factory");
    let factory = unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const ClapPluginFactory;
    let create = unsafe { (*factory).create_plugin.expect("create_plugin") };
    let plugin = unsafe { create(factory, host, c"com.stave.test.gui-window".as_ptr()) };
    assert!(!plugin.is_null());
    assert!(unsafe { ((*plugin).init.expect("init"))(plugin) });
    plugin
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
unsafe fn gui(plugin: *const ClapPlugin) -> &'static ClapPluginGui {
    let get_ext = unsafe { (*plugin).get_extension.expect("get_extension") };
    unsafe { &*get_ext(plugin, CLAP_EXT_GUI.as_ptr()).cast::<ClapPluginGui>() }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod x11 {
    use super::*;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::Mutex;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::wrapper::ConnectionExt as _;

    /// What the host's run loop holds: (timer id, period), descriptors.
    static TIMERS: Mutex<Vec<(ClapId, u32)>> = Mutex::new(Vec::new());
    static FDS: Mutex<Vec<c_int>> = Mutex::new(Vec::new());

    unsafe extern "C" fn register_timer(_: *const ClapHost, period: u32, id: *mut ClapId) -> bool {
        let mut timers = TIMERS.lock().unwrap();
        let next = 7 + timers.len() as ClapId;
        timers.push((next, period));
        unsafe { *id = next };
        true
    }
    unsafe extern "C" fn unregister_timer(_: *const ClapHost, id: ClapId) -> bool {
        TIMERS.lock().unwrap().retain(|(t, _)| *t != id);
        true
    }
    unsafe extern "C" fn register_fd(_: *const ClapHost, fd: c_int, flags: u32) -> bool {
        assert_eq!(flags, CLAP_POSIX_FD_READ);
        FDS.lock().unwrap().push(fd);
        true
    }
    unsafe extern "C" fn modify_fd(_: *const ClapHost, _: c_int, _: u32) -> bool {
        true
    }
    unsafe extern "C" fn unregister_fd(_: *const ClapHost, fd: c_int) -> bool {
        FDS.lock().unwrap().retain(|f| *f != fd);
        true
    }

    static HOST_TIMER: ClapHostTimerSupport =
        ClapHostTimerSupport { register_timer: Some(register_timer), unregister_timer: Some(unregister_timer) };
    static HOST_FD: ClapHostPosixFdSupport = ClapHostPosixFdSupport {
        register_fd: Some(register_fd),
        modify_fd: Some(modify_fd),
        unregister_fd: Some(unregister_fd),
    };

    unsafe extern "C" fn get_extension(_: *const ClapHost, id: *const c_char) -> *const c_void {
        let id = unsafe { CStr::from_ptr(id) };
        if id == CLAP_EXT_TIMER_SUPPORT {
            (&HOST_TIMER as *const ClapHostTimerSupport).cast()
        } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
            (&HOST_FD as *const ClapHostPosixFdSupport).cast()
        } else {
            std::ptr::null()
        }
    }

    struct Host(ClapHost);
    // SAFETY (test): the host is read only; its strings are 'static.
    unsafe impl Sync for Host {}
    static HOST: Host = Host(ClapHost {
        clap_version: ClapVersion { major: CLAP_VERSION_MAJOR, minor: CLAP_VERSION_MINOR, revision: CLAP_VERSION_REVISION },
        host_data: std::ptr::null_mut(),
        name: c"stave test host".as_ptr(),
        vendor: c"stave".as_ptr(),
        url: c"".as_ptr(),
        version: c"0".as_ptr(),
        get_extension: Some(get_extension),
        request_restart: None,
        request_process: None,
        request_callback: None,
    });

    fn display() -> Option<(x11rb::rust_connection::RustConnection, u32)> {
        if std::env::var_os("DISPLAY").is_none() {
            assert!(std::env::var_os("STAVE_REQUIRE_DISPLAY").is_none(), "STAVE_REQUIRE_DISPLAY is set but DISPLAY is not");
            eprintln!("no DISPLAY: the X11 gui test passes vacuously");
            return None;
        }
        let (conn, screen) = x11rb::connect(None).expect("the X display");
        let root = conn.setup().roots[screen].root;
        let parent = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            parent,
            root,
            0,
            0,
            400,
            400,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap();
        conn.map_window(parent).unwrap();
        conn.sync().unwrap();
        Some((conn, parent))
    }

    fn children(conn: &x11rb::rust_connection::RustConnection, parent: u32) -> usize {
        conn.query_tree(parent).unwrap().reply().unwrap().children.len()
    }

    #[test]
    fn an_x11_gui_joins_the_hosts_run_loop_and_leaves_it() {
        let Some((conn, parent)) = display() else { return };
        unsafe {
            let plugin = create(&HOST.0);
            let gui = gui(plugin);
            let get_ext = (*plugin).get_extension.unwrap();
            let on_timer = &*get_ext(plugin, CLAP_EXT_TIMER_SUPPORT.as_ptr()).cast::<ClapPluginTimerSupport>();
            let on_fd = &*get_ext(plugin, CLAP_EXT_POSIX_FD_SUPPORT.as_ptr()).cast::<ClapPluginPosixFdSupport>();
            assert!((gui.create.unwrap())(plugin, CLAP_WINDOW_API_X11.as_ptr(), false));
            let window = ClapWindow { api: CLAP_WINDOW_API_X11.as_ptr(), handle: ClapWindowHandle { x11: parent.into() } };
            assert!((gui.set_parent.unwrap())(plugin, &window), "attached to a real X11 parent");
            let timers = TIMERS.lock().unwrap().clone();
            let fds = FDS.lock().unwrap().clone();
            assert_eq!(timers.len(), 1, "one repaint timer");
            assert!(timers[0].1 > 0 && timers[0].1 <= 20, "about 60 Hz: {} ms", timers[0].1);
            assert_eq!(fds.len(), 1, "the display connection's descriptor");
            assert_eq!(children(&conn, parent), 1, "the face's window is the parent's child");
            // The host's loop calls back: the view pumps (maps, paints).
            for _ in 0..5 {
                (on_timer.on_timer.unwrap())(plugin, timers[0].0);
                (on_fd.on_fd.unwrap())(plugin, fds[0], CLAP_POSIX_FD_READ);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let child = conn.query_tree(parent).unwrap().reply().unwrap().children[0];
            let (mut w, mut h) = (0u32, 0u32);
            assert!((gui.get_size.unwrap())(plugin, &mut w, &mut h));
            let g = conn.get_geometry(child).unwrap().reply().unwrap();
            assert_eq!((u32::from(g.width), u32::from(g.height)), (w, h), "the window is the size the host was told");
            (gui.destroy.unwrap())(plugin);
            assert!(TIMERS.lock().unwrap().is_empty(), "the timer is unregistered");
            assert!(FDS.lock().unwrap().is_empty(), "the descriptor is unregistered");
            conn.sync().unwrap();
            assert_eq!(children(&conn, parent), 0, "the face's window is gone");
            ((*plugin).destroy.unwrap())(plugin);
        }
        let _ = conn.destroy_window(parent);
        let _ = conn.sync();
    }

    #[test]
    fn without_a_host_timer_an_x11_gui_refuses_the_parent() {
        let Some((conn, parent)) = display() else { return };
        unsafe {
            let plugin = create(std::ptr::null());
            let gui = gui(plugin);
            assert!((gui.create.unwrap())(plugin, CLAP_WINDOW_API_X11.as_ptr(), false));
            let window = ClapWindow { api: CLAP_WINDOW_API_X11.as_ptr(), handle: ClapWindowHandle { x11: parent.into() } };
            assert!(!(gui.set_parent.unwrap())(plugin, &window), "no run loop to join");
            (gui.destroy.unwrap())(plugin);
            ((*plugin).destroy.unwrap())(plugin);
        }
        let _ = conn.destroy_window(parent);
        let _ = conn.sync();
    }
}

#[cfg(windows)]
mod win32 {
    use super::*;
    use stave_ui_win32::sys::*;

    #[link(name = "user32")]
    extern "system" {
        fn GetWindow(hwnd: HWND, cmd: u32) -> HWND;
    }
    const GW_CHILD: u32 = 5;

    #[test]
    fn a_win32_gui_is_a_child_of_the_hosts_window() {
        unsafe {
            let class = wide("STATIC");
            let parent = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                800,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(!parent.is_null());
            let plugin = create(std::ptr::null());
            let gui = gui(plugin);
            assert!((gui.create.unwrap())(plugin, CLAP_WINDOW_API_WIN32.as_ptr(), false));
            let window = ClapWindow { api: CLAP_WINDOW_API_WIN32.as_ptr(), handle: ClapWindowHandle { win32: parent } };
            assert!((gui.set_parent.unwrap())(plugin, &window));
            let child = GetWindow(parent, GW_CHILD);
            assert!(!child.is_null(), "the face's window is the parent's child");
            let (mut w, mut h) = (0u32, 0u32);
            assert!((gui.get_size.unwrap())(plugin, &mut w, &mut h));
            let mut r = RECT::default();
            GetClientRect(child, &mut r);
            assert_eq!((r.right as u32, r.bottom as u32), (w, h), "the window is the size the host was told");
            (gui.destroy.unwrap())(plugin);
            assert!(GetWindow(parent, GW_CHILD).is_null(), "the face's window is gone");
            ((*plugin).destroy.unwrap())(plugin);
            DestroyWindow(parent);
        }
    }
}
