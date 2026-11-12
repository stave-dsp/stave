use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    println!("cargo:rustc-env=STAVE_BUILD_STAMP={stamp}");
    println!("cargo:rerun-if-changed=/nonexistent-force-restamp");
}
