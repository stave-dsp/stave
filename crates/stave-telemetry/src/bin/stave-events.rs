//! Check event traces against the protocol's laws:
//!
//! ```text
//! STAVE_EVENTS=1 <run a DAW session>      # writes <log dir>/<id>-<pid>-<n>.events
//! cargo run -p stave-telemetry --bin stave-events -- FILE...
//! ```
//!
//! Prints each file's violations (and whether a gesture was left open)
//! and exits 1 if any file breaks a law.

use std::process::ExitCode;

use stave_telemetry::events::{check, quiescent, Trace};

const USAGE: &str = "\
usage: stave-events FILE...

Check event traces (written with STAVE_EVENTS=1 as
<log dir>/<id>-<pid>-<n>.events) against the protocol's laws: a
gesture begun once and ended once, a face's edit inside its gesture, no
gap in the indices, release after a prepare, no fault; and whether a
gesture was left open at the end.

Prints each file's violations; exits 1 if any file breaks a law, 2 on
bad usage.

  -h, --help    this text";

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if files.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut clean = true;
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{file}: {e}");
                clean = false;
                continue;
            }
        };
        let trace: Trace = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{file}: {e}");
                clean = false;
                continue;
            }
        };
        let mut violations = check(&trace);
        violations.extend(quiescent(&trace).err());
        if violations.is_empty() {
            println!("{file}: {} events, every law kept", trace.0.len());
        } else {
            clean = false;
            for v in violations {
                println!("{file}: {v}");
            }
        }
    }
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
