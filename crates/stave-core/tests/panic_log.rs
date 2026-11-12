//! The panic hook is process-wide, so this check has a binary of its own.

use stave_core::diagnostics::{log_dir, log_panics};
use stave_core::Info;
use std::path::PathBuf;
use std::sync::OnceLock;

/// This process's own log directory: concurrent test runs (two
/// `cargo test`s, nextest's processes) never share a log file.
fn log_file() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("stave-panic-log-{}", std::process::id()));
        // Set once, before either test installs the hook or logs.
        std::env::set_var("STAVE_LOG_DIR", &dir);
        dir
    });
    assert_eq!(log_dir().as_ref(), Some(dir));
    dir.join("com.stave.test.panic-log.log")
}

const DESCRIPTOR: Info = Info {
    id: "com.stave.test.panic-log",
    name: "Panic Log",
    vendor: "stave tests",
    ..Info::EFFECT
};

#[test]
#[cfg_attr(miri, ignore = "Miri isolates the file system")]
fn a_panic_is_logged_as_fatal_with_its_location() {
    let path = log_file();
    let before = std::fs::read_to_string(&path).unwrap_or_default().len();
    log_panics(&DESCRIPTOR);
    log_panics(&DESCRIPTOR);
    let caught = std::panic::catch_unwind(|| panic!("the plugin gave up"));
    assert!(caught.is_err());
    let text = std::fs::read_to_string(&path).unwrap();
    let logged = &text[before..];
    // The other test writes this file too: find this panic's message.
    assert_eq!(logged.matches("the plugin gave up").count(), 1, "{logged}");
    assert!(
        logged.contains(" fatal Panic Log: panicked at crates/stave-core/tests/panic_log.rs:"),
        "{logged}"
    );
}

/// On the audio thread the hook writes nothing; the next main-thread
/// entry point (any `PanicGuard::run`) writes the message.
#[test]
#[cfg_attr(miri, ignore = "Miri isolates the file system")]
fn a_realtime_panic_is_written_by_the_next_main_thread_call() {
    use stave_core::diagnostics::Diagnostics;
    use stave_core::runtime::PanicGuard;
    // Its own instance (and log file): the other test writes DESCRIPTOR's
    // concurrently, and log lines are not written atomically.
    const INSTANCE: Info = Info {
        id: "com.stave.test.panic-log-realtime",
        ..DESCRIPTOR
    };
    let path = log_file().with_file_name("com.stave.test.panic-log-realtime.log");
    log_panics(&DESCRIPTOR);
    let count = || {
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .matches("the audio thread gave up")
            .count()
    };
    let before = count();
    let guard = PanicGuard::default();
    let diagnostics = Diagnostics::new(&INSTANCE);
    assert_eq!(
        guard.run_realtime("process", || 0, || panic!("the audio thread gave up")),
        0
    );
    assert_eq!(count(), before, "nothing written on the audio thread");
    guard.run(&diagnostics, "setActive", || (), || ());
    assert_eq!(count(), before + 1);
}
