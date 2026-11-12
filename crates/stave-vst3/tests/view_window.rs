//! `IPlugView` in a real parent window, the way a Linux or Windows host
//! drives it: on Linux (a real X server — Xvfb in CI) `attached` with an
//! `X11EmbedWindowID` makes the face's child window and joins the
//! frame's `IRunLoop` with a descriptor handler and a timer, the host's
//! timer pumps it, and `removed` leaves the loop and the window; on
//! Windows `attached` with an `HWND` makes a child window and `removed`
//! destroys it. (macOS: the Cocoa real-window tests.)
//!
//! Without `DISPLAY` the Linux test passes vacuously, unless
//! `STAVE_REQUIRE_DISPLAY` is set (CI sets it).

#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use stave_core::params::{FloatParam, Params, Range};
use stave_core::runtime::Erased;
use stave_core::{Block, Info, ParamId, Telemetry};
use stave_vst3::stave_face;
use stave_vst3::sys::*;
use stave_vst3::wrapper::FactoryObj;

#[derive(stave_core::Params)]
struct OneKnob {
    drive: FloatParam,
}

struct Fixture {
    params: Arc<OneKnob>,
}

impl Erased for Fixture {
    const INFO: Info = Info {
        id: "com.stave.test.view-window",
        name: "View Window",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "IPlugView fixture",
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

impl stave_vst3::wrapper::Vst3Export for Fixture {
    fn face(&self) -> Option<stave_face::Face> {
        use stave_face::prelude::*;
        Some(Face::new(Theme::graphite(), Knob::new(self.params.drive.bind()).node()))
    }
}

stave_vst3::export_vst3!(Fixture);

#[repr(C)]
struct RawObj<V> {
    vtbl: *const V,
}

/// A component, its controller, and a fresh view of it.
#[cfg_attr(target_os = "macos", allow(dead_code))]
struct Rig {
    component: *mut FUnknown,
    controller: *mut FUnknown,
    view: *mut FUnknown,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
impl Rig {
    unsafe fn new() -> Rig {
        unsafe {
            let factory = GetPluginFactory() as *mut FactoryObj;
            let fvtbl = &*(*factory).vtbl;
            let cid = <Fixture as stave_vst3::wrapper::Vst3Export>::COMPONENT_CID;
            let mut obj: *mut c_void = ptr::null_mut();
            let res = (fvtbl.create_instance)(
                factory as *mut FUnknown,
                cid.as_ptr() as FidString,
                IID_ICOMPONENT.as_ptr() as FidString,
                &mut obj,
            );
            assert_eq!(res, K_RESULT_OK);
            let component = obj as *mut FUnknown;
            let qi = (*(*(component as *mut RawObj<IComponentVtbl>)).vtbl).query_interface;
            let mut ctl: *mut c_void = ptr::null_mut();
            assert_eq!(qi(component, &IID_IEDIT_CONTROLLER, &mut ctl), K_RESULT_OK);
            let controller = ctl as *mut FUnknown;
            let ec = &*(*(controller as *mut RawObj<IEditControllerVtbl>)).vtbl;
            let view = (ec.create_view)(controller, c"editor".as_ptr());
            assert!(!view.is_null(), "createView");
            Rig { component, controller, view }
        }
    }

    unsafe fn vt(&self) -> &'static IPlugViewVtbl {
        unsafe { &*(*(self.view as *mut RawObj<IPlugViewVtbl>)).vtbl }
    }

    unsafe fn size(&self) -> (i32, i32) {
        let mut r = ViewRect { left: 0, top: 0, right: 0, bottom: 0 };
        assert_eq!(unsafe { (self.vt().get_size)(self.view, &mut r) }, K_RESULT_OK);
        (r.right - r.left, r.bottom - r.top)
    }

    unsafe fn release(self) {
        unsafe {
            assert_eq!((self.vt().release)(self.view), 0, "the host held the last view reference");
            let release = (*(*(self.component as *mut RawObj<IComponentVtbl>)).vtbl).release;
            release(self.controller);
            assert_eq!(release(self.component), 0);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    use super::*;
    use std::sync::Mutex;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::wrapper::ConnectionExt as _;

    /// What the run loop holds: handlers with their descriptor, timers
    /// with their period (as addresses: the handlers are the plugin's).
    static HANDLERS: Mutex<Vec<(usize, i32)>> = Mutex::new(Vec::new());
    static TIMERS: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());

    unsafe fn add_ref(obj: *mut FUnknown) {
        unsafe { ((*(*(obj as *mut RawObj<IEventHandlerVtbl>)).vtbl).add_ref)(obj) };
    }
    unsafe fn release(obj: *mut FUnknown) {
        unsafe { ((*(*(obj as *mut RawObj<IEventHandlerVtbl>)).vtbl).release)(obj) };
    }

    unsafe extern "C" fn rl_qi(this: *mut FUnknown, iid: *const Tuid, obj: *mut *mut c_void) -> Tresult {
        unsafe {
            if *iid == IID_IRUN_LOOP || *iid == IID_FUNKNOWN {
                *obj = this.cast();
                K_RESULT_OK
            } else {
                *obj = ptr::null_mut();
                K_NO_INTERFACE
            }
        }
    }
    unsafe extern "C" fn rl_ref(_: *mut FUnknown) -> u32 {
        1
    }
    // The host keeps a reference on what it registers (the SDK's run
    // loops hold an IPtr).
    unsafe extern "C" fn rl_register_event_handler(_: *mut FUnknown, handler: *mut FUnknown, fd: i32) -> Tresult {
        unsafe { add_ref(handler) };
        HANDLERS.lock().unwrap().push((handler as usize, fd));
        K_RESULT_OK
    }
    unsafe extern "C" fn rl_unregister_event_handler(_: *mut FUnknown, handler: *mut FUnknown) -> Tresult {
        HANDLERS.lock().unwrap().retain(|(h, _)| *h != handler as usize);
        unsafe { release(handler) };
        K_RESULT_OK
    }
    unsafe extern "C" fn rl_register_timer(_: *mut FUnknown, handler: *mut FUnknown, ms: u64) -> Tresult {
        unsafe { add_ref(handler) };
        TIMERS.lock().unwrap().push((handler as usize, ms));
        K_RESULT_OK
    }
    unsafe extern "C" fn rl_unregister_timer(_: *mut FUnknown, handler: *mut FUnknown) -> Tresult {
        TIMERS.lock().unwrap().retain(|(h, _)| *h != handler as usize);
        unsafe { release(handler) };
        K_RESULT_OK
    }

    static RUN_LOOP_VTBL: IRunLoopVtbl = IRunLoopVtbl {
        query_interface: rl_qi,
        add_ref: rl_ref,
        release: rl_ref,
        register_event_handler: rl_register_event_handler,
        unregister_event_handler: rl_unregister_event_handler,
        register_timer: rl_register_timer,
        unregister_timer: rl_unregister_timer,
    };

    /// `IPlugFrame` (its one method, `resizeView`, is unused: the face
    /// never resizes).
    #[repr(C)]
    struct FrameVtbl {
        query_interface: unsafe extern "C" fn(*mut FUnknown, *const Tuid, *mut *mut c_void) -> Tresult,
        add_ref: unsafe extern "C" fn(*mut FUnknown) -> u32,
        release: unsafe extern "C" fn(*mut FUnknown) -> u32,
        resize_view: unsafe extern "C" fn(*mut FUnknown, *mut FUnknown, *mut ViewRect) -> Tresult,
    }
    unsafe extern "C" fn frame_qi(_: *mut FUnknown, iid: *const Tuid, obj: *mut *mut c_void) -> Tresult {
        unsafe {
            if *iid == IID_IRUN_LOOP {
                *obj = ptr::addr_of!(RUN_LOOP).cast_mut().cast();
                K_RESULT_OK
            } else {
                *obj = ptr::null_mut();
                K_NO_INTERFACE
            }
        }
    }
    unsafe extern "C" fn frame_resize(_: *mut FUnknown, _: *mut FUnknown, _: *mut ViewRect) -> Tresult {
        K_RESULT_FALSE
    }
    static FRAME_VTBL: FrameVtbl =
        FrameVtbl { query_interface: frame_qi, add_ref: rl_ref, release: rl_ref, resize_view: frame_resize };

    struct Obj<V: 'static>(#[allow(dead_code)] RawObj<V>);
    // SAFETY (test): read-only objects pointing at statics.
    unsafe impl<V> Sync for Obj<V> {}
    static RUN_LOOP: Obj<IRunLoopVtbl> = Obj(RawObj { vtbl: &RUN_LOOP_VTBL });
    static FRAME: Obj<FrameVtbl> = Obj(RawObj { vtbl: &FRAME_VTBL });

    #[test]
    fn an_x11_view_joins_the_frames_run_loop_and_leaves_it() {
        if std::env::var_os("DISPLAY").is_none() {
            assert!(std::env::var_os("STAVE_REQUIRE_DISPLAY").is_none(), "STAVE_REQUIRE_DISPLAY is set but DISPLAY is not");
            eprintln!("no DISPLAY: the X11 view test passes vacuously");
            return;
        }
        let (conn, screen) = x11rb::connect(None).expect("the X display");
        let root = conn.setup().roots[screen].root;
        let parent = conn.generate_id().unwrap();
        conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, parent, root, 0, 0, 400, 400, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        conn.map_window(parent).unwrap();
        conn.sync().unwrap();
        let children = |conn: &x11rb::rust_connection::RustConnection| conn.query_tree(parent).unwrap().reply().unwrap().children;
        unsafe {
            let rig = Rig::new();
            let vt = rig.vt();
            let frame = ptr::addr_of!(FRAME).cast_mut().cast::<FUnknown>();
            assert_eq!((vt.set_frame)(rig.view, frame), K_RESULT_OK);
            assert_eq!(
                (vt.attached)(rig.view, parent as usize as *mut c_void, c"X11EmbedWindowID".as_ptr()),
                K_RESULT_OK,
                "attached to a real X11 parent"
            );
            let timers = TIMERS.lock().unwrap().clone();
            let handlers = HANDLERS.lock().unwrap().clone();
            assert_eq!(timers.len(), 1, "one repaint timer");
            assert!(timers[0].1 > 0 && timers[0].1 <= 20, "about 60 Hz: {} ms", timers[0].1);
            assert_eq!(handlers.len(), 1, "the display connection's descriptor");
            assert!(handlers[0].1 >= 0);
            // The host's loop calls back: the view pumps (maps, paints).
            let timer = timers[0].0 as *mut FUnknown;
            let handler = handlers[0].0 as *mut FUnknown;
            let tv = &*(*(timer as *mut RawObj<ITimerHandlerVtbl>)).vtbl;
            let hv = &*(*(handler as *mut RawObj<IEventHandlerVtbl>)).vtbl;
            let mut same: *mut c_void = ptr::null_mut();
            assert_eq!((tv.query_interface)(timer, &IID_ITIMER_HANDLER, &mut same), K_RESULT_OK);
            assert_eq!(same, timer.cast());
            (tv.release)(timer);
            assert_eq!((hv.query_interface)(handler, &IID_ITIMER_HANDLER, &mut same), K_NO_INTERFACE);
            for _ in 0..5 {
                (tv.on_timer)(timer);
                (hv.on_fd_is_set)(handler, handlers[0].1);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let kids = children(&conn);
            assert_eq!(kids.len(), 1, "the face's window is the parent's child");
            let g = conn.get_geometry(kids[0]).unwrap().reply().unwrap();
            assert_eq!((i32::from(g.width), i32::from(g.height)), rig.size(), "the window is the size the host was told");
            assert_eq!((vt.removed)(rig.view), K_RESULT_OK);
            assert!(TIMERS.lock().unwrap().is_empty(), "the timer is unregistered");
            assert!(HANDLERS.lock().unwrap().is_empty(), "the handler is unregistered");
            conn.sync().unwrap();
            assert!(children(&conn).is_empty(), "the face's window is gone");
            rig.release();
        }
        let _ = conn.destroy_window(parent);
        let _ = conn.sync();
    }

    #[test]
    fn without_a_run_loop_an_x11_view_refuses_the_parent() {
        if std::env::var_os("DISPLAY").is_none() {
            return;
        }
        let (conn, screen) = x11rb::connect(None).expect("the X display");
        let root = conn.setup().roots[screen].root;
        unsafe {
            let rig = Rig::new();
            let vt = rig.vt();
            // No setFrame: nothing to join.
            assert_ne!((vt.attached)(rig.view, root as usize as *mut c_void, c"X11EmbedWindowID".as_ptr()), K_RESULT_OK);
            assert_eq!((vt.removed)(rig.view), K_RESULT_OK);
            rig.release();
        }
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
    fn an_hwnd_view_is_a_child_of_the_hosts_window() {
        unsafe {
            let class = wide("STATIC");
            let parent = CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                800,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!parent.is_null());
            let rig = Rig::new();
            let vt = rig.vt();
            assert_eq!((vt.attached)(rig.view, parent, c"HWND".as_ptr()), K_RESULT_OK);
            let child = GetWindow(parent, GW_CHILD);
            assert!(!child.is_null(), "the face's window is the parent's child");
            let mut r = RECT::default();
            GetClientRect(child, &mut r);
            assert_eq!((r.right, r.bottom), rig.size(), "the window is the size the host was told");
            assert_eq!((vt.removed)(rig.view), K_RESULT_OK);
            assert!(GetWindow(parent, GW_CHILD).is_null(), "the face's window is gone");
            rig.release();
            DestroyWindow(parent);
        }
    }
}
