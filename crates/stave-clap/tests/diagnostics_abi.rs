//! Refused calls reach the host's `clap.log`: a fake host that offers it
//! records each message the plugin sends.

// Every `unsafe` block here plays the host: it calls the plugin's C ABI
// with pointers this file owns, under clap.h's contract. Miri runs it.
#![allow(clippy::undocumented_unsafe_blocks)]

use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use stave_clap::sys::*;
use stave_core::runtime::Erased;
use stave_core::{Block, Info, Telemetry};

struct Silent;

impl Erased for Silent {
    const INFO: Info = Info {
        id: "com.stave.test.diagnostics-clap",
        name: "Diagnostics (CLAP)",
        vendor: "stave tests",
        version: "0.0.0",
        url: "",
        description: "diagnostics fixture",
        features: &[],
        au: None,
    };

    fn new(_telemetry: &Telemetry) -> Self {
        Self
    }

    fn process(&mut self, _block: &mut Block<'_>) {}
}

impl stave_clap::wrapper::ClapExport for Silent {}

stave_clap::export_clap!(Silent);

static MESSAGES: Mutex<Vec<(ClapLogSeverity, String)>> = Mutex::new(Vec::new());
static CALLBACK_REQUESTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn host_log(
    _host: *const ClapHost,
    severity: ClapLogSeverity,
    msg: *const c_char,
) {
    let text = unsafe { CStr::from_ptr(msg) }
        .to_string_lossy()
        .into_owned();
    MESSAGES.lock().unwrap().push((severity, text));
}

static HOST_LOG: ClapHostLog = ClapHostLog {
    log: Some(host_log),
};

unsafe extern "C" fn get_extension(_host: *const ClapHost, id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_EXT_LOG {
        ptr::from_ref(&HOST_LOG).cast()
    } else {
        ptr::null()
    }
}

unsafe extern "C" fn request_callback(_host: *const ClapHost) {
    CALLBACK_REQUESTED.store(true, Ordering::Relaxed);
}

unsafe extern "C" fn request_nothing(_host: *const ClapHost) {}

struct Host(ClapHost);

// SAFETY: every pointer in the host is to 'static data.
unsafe impl Sync for Host {}

static HOST: Host = Host(ClapHost {
    clap_version: ClapVersion {
        major: 1,
        minor: 2,
        revision: 0,
    },
    host_data: ptr::null_mut(),
    name: c"diagnostics test host".as_ptr(),
    vendor: c"".as_ptr(),
    url: c"".as_ptr(),
    version: c"".as_ptr(),
    get_extension: Some(get_extension),
    request_restart: Some(request_nothing),
    request_process: Some(request_nothing),
    request_callback: Some(request_callback),
});

unsafe extern "C" fn read_garbage(
    _stream: *const ClapIstream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return 0;
    }
    let garbage = b"not a state chunk";
    let n = garbage.len().min(size as usize);
    unsafe { ptr::copy_nonoverlapping(garbage.as_ptr(), buffer.cast(), n) };
    n as i64
}

#[test]
fn refused_calls_and_process_faults_reach_the_host_log() {
    unsafe {
        let get_factory = clap_entry.get_factory.unwrap();
        let factory = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<ClapPluginFactory>();
        let create = (*factory).create_plugin.unwrap();
        let plugin = create(factory, &HOST.0, c"com.stave.test.diagnostics-clap".as_ptr());
        assert!(((*plugin).init.unwrap())(plugin));
        let get_ext = (*plugin).get_extension.unwrap();

        let state = &*get_ext(plugin, CLAP_EXT_STATE.as_ptr()).cast::<ClapPluginState>();
        let stream = ClapIstream {
            ctx: ptr::null_mut(),
            read: Some(read_garbage),
        };
        assert!(!(state.load.unwrap())(plugin, &stream));

        let params = &*get_ext(plugin, CLAP_EXT_PARAMS.as_ptr()).cast::<ClapPluginParams>();
        let mut value = 0.0;
        assert!(!(params.get_value.unwrap())(plugin, 999, &mut value));

        assert_eq!(
            ((*plugin).process.unwrap())(plugin, ptr::null()),
            CLAP_PROCESS_ERROR
        );
        assert_eq!(
            ((*plugin).process.unwrap())(plugin, ptr::null()),
            CLAP_PROCESS_ERROR
        );
        assert!(CALLBACK_REQUESTED.load(Ordering::Relaxed));
        ((*plugin).on_main_thread.unwrap())(plugin);
        ((*plugin).destroy.unwrap())(plugin);
    }
    let messages = MESSAGES.lock().unwrap().clone();
    assert_eq!(
        messages,
        [
            (
                3,
                "state.load: state not loaded: not a STVE chunk".to_string()
            ),
            (5, "params.get_value: no parameter has id 999".to_string()),
            (5, "process refused: no process data (×2)".to_string()),
        ]
    );
}
