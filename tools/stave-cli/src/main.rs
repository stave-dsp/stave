//! `stave` — the stave tool. Every command lives in `stave-tools`' registry;
//! this binary picks a layer:
//!
//! ```text
//! stave <command> …    the CLI layer (stave_tools::cli)
//! stave mcp            the MCP layer on stdin/stdout (stave_tools::mcp)
//! stave play …         the GUI layer: stave-play, beside this binary or on PATH
//! ```
//!
//! The voicing modules (null tests, blessed takes, program material) still
//! lives in stave-play; `voicing` registers its headless modes as commands
//! that run it, so they are CLI commands and MCP tools too.

use std::io::Write;
use std::process::ExitCode;

mod voicing;

fn registry() -> stave_tools::Registry {
    let mut r = stave_tools::registry();
    voicing::register(&mut r);
    r
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("play") {
        return match voicing::play(&args[1..]) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("stave play: {e}");
                ExitCode::from(3)
            }
        };
    }
    // stdout is the data channel (--json, JSON-RPC); plugins print to
    // stderr from here on.
    let Some(mut out) = stave_tools::stdout::reserve() else {
        eprintln!("stave: could not reserve stdout");
        return ExitCode::from(3);
    };
    if args.first().map(String::as_str) == Some("mcp") {
        if args.len() > 1 && !matches!(args[1].as_str(), "-h" | "--help") {
            eprintln!("stave mcp: takes no arguments (an MCP client starts it and speaks JSON-RPC on stdin/stdout)");
            return ExitCode::from(stave_tools::cli::USAGE_EXIT);
        }
        if args.len() > 1 {
            let _ = writeln!(
                out,
                "stave mcp — serve every stave command as an MCP tool over stdio (JSON-RPC 2.0, one message per line).\n\n\
                 Register it with an MCP client, e.g. `claude mcp add stave -- stave mcp`.\n\
                 Tools: {}",
                registry().commands().map(|c| c.info().tool_name()).collect::<Vec<_>>().join(", ")
            );
            return ExitCode::SUCCESS;
        }
        let stdin = std::io::stdin();
        return match stave_tools::mcp::serve(registry(), stdin.lock(), Box::new(out)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("stave mcp: {e}");
                ExitCode::from(3)
            }
        };
    }
    let code = stave_tools::cli::run(&registry(), &args, &mut out, &mut std::io::stderr());
    ExitCode::from(code)
}
