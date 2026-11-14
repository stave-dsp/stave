//! stave-bench — deprecated: `stave` (tools/stave-cli) took its modes as
//! commands. This shim keeps scripts working for one release: it maps
//! the old flags onto the new commands, says so once on stderr, and runs
//! them through the same registry.
//!
//! ```text
//! stave-bench X [opts]               stave bench X [opts]
//! stave-bench X --describe           stave describe X --json
//! stave-bench X --list               stave describe X
//! stave-bench X --validate           stave validate X --json
//! stave-bench X --response [--response-levels L]
//!                                    stave response X [--levels L]
//! ```

use std::process::ExitCode;

/// The new command line for an old one.
fn translate(args: &[String]) -> Vec<String> {
    let mut command = "bench";
    let mut json = false;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--describe" => (command, json) = ("describe", true),
            "--list" => command = "describe",
            "--validate" => (command, json) = ("validate", true),
            "--response" => command = "response",
            "--json" => json = true,
            "--response-levels" => {
                rest.push("--levels".to_string());
                rest.extend(it.next().cloned());
            }
            _ => rest.push(a.clone()),
        }
    }
    let mut out = vec![command.to_string()];
    out.extend(rest);
    if json {
        out.push("--json".into());
    }
    out
}

/// Drop the options the chosen command does not take (the old binary
/// ignored them), saying which.
fn keep_known(registry: &stave_tools::Registry, args: Vec<String>) -> Vec<String> {
    let Some(info) = registry.get(&args[0]).map(|c| *c.info()) else { return args };
    let mut out = vec![args[0].clone()];
    let mut it = args.into_iter().skip(1);
    while let Some(a) = it.next() {
        let takes = |a: &str| a == "--json" || a == "--help" || a == "-h" || info.inputs.iter().any(|f| f.flag_name() == a);
        if a.starts_with("--") && !takes(&a) {
            let value = it.next();
            eprintln!("stave-bench: {a} {} ignored (stave {} does not take it)", value.unwrap_or_default(), info.id);
            continue;
        }
        out.push(a);
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let registry = stave_tools::registry();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "stave-bench is deprecated and forwards to `stave` (tools/stave-cli):\n  \
             stave-bench X [opts]      -> stave bench X [opts]\n  \
             stave-bench X --describe  -> stave describe X --json\n  \
             stave-bench X --list      -> stave describe X\n  \
             stave-bench X --validate  -> stave validate X --json\n  \
             stave-bench X --response  -> stave response X (--response-levels -> --levels)\n\n{}",
            stave_tools::cli::command_help(registry.get("bench").expect("built in").info())
        );
        return ExitCode::SUCCESS;
    }
    let new = keep_known(&registry, translate(&args));
    eprintln!("stave-bench is deprecated: running `stave {}`", new.join(" "));
    let Some(mut out) = stave_tools::stdout::reserve() else {
        eprintln!("stave-bench: could not reserve stdout");
        return ExitCode::FAILURE;
    };
    ExitCode::from(stave_tools::cli::run(&registry, &new, &mut out, &mut std::io::stderr()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> String {
        let args: Vec<String> = s.split_whitespace().map(String::from).collect();
        keep_known(&stave_tools::registry(), translate(&args)).join(" ")
    }

    #[test]
    fn old_modes_become_commands() {
        assert_eq!(t("X.vst3 --seconds 1 --json"), "bench X.vst3 --seconds 1 --json");
        assert_eq!(t("X.vst3 --describe"), "describe X.vst3 --json");
        assert_eq!(t("X.vst3 --list"), "describe X.vst3");
        assert_eq!(t("X.vst3 --validate --rate 44100"), "validate X.vst3 --rate 44100 --json");
        assert_eq!(t("X --response --response-levels -6,0"), "response X --levels -6,0");
        assert_eq!(t("X --validate --seconds 3"), "validate X --json", "an option the mode ignored is dropped");
    }
}
