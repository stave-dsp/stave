//! The panic hook is process-wide, so this check has a binary of its own.

use stave_core::diagnostics::{log_dir, log_panics};
use stave_core::PluginDescriptor;

const DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: c"com.stave.test.panic-log",
    name: c"Panic Log",
    vendor: c"stave tests",
    version: c"0.0.0",
    url: c"",
    description: c"",
    features: &[],
};

#[test]
#[cfg_attr(miri, ignore = "Miri isolates the file system")]
fn a_panic_is_logged_as_fatal_with_its_location() {
    let path = log_dir()
        .expect("cargo sets STAVE_LOG_DIR")
        .join("com.stave.test.panic-log.log");
    let before = std::fs::read_to_string(&path).unwrap_or_default().len();
    log_panics(&DESCRIPTOR);
    log_panics(&DESCRIPTOR);
    let caught = std::panic::catch_unwind(|| panic!("the plugin gave up"));
    assert!(caught.is_err());
    let text = std::fs::read_to_string(&path).unwrap();
    let logged = &text[before..];
    assert_eq!(logged.matches(" fatal Panic Log: ").count(), 1, "{logged}");
    assert!(logged.contains("tests/panic_log.rs"), "{logged}");
    assert!(logged.contains("the plugin gave up"), "{logged}");
}
