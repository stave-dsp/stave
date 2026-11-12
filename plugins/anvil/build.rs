// Build stamp: every binary knows when it was compiled, and the
// debug bus reports it as the `build` probe — so a bench render
// can never silently come from a stale bundle (the poisoned-
// measurement day, 2026-07-31: hours of measurement runs read old
// binaries; the stamp makes that class of fiction impossible).
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    println!("cargo:rustc-env=STAVE_BUILD_STAMP={stamp}");
    // Re-stamp on every build invocation that actually relinks.
    println!("cargo:rerun-if-changed=/nonexistent-force-restamp");
}
