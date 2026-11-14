//! stave-fit — deprecated: `stave fit` and `stave fit stimulus`
//! (tools/stave-cli) took its two commands. This shim keeps scripts
//! working for one release: `stave-fit stimulus …` runs `stave fit
//! stimulus …`, `stave-fit fit … -o M` runs `stave fit … --out M`.

use std::process::ExitCode;

/// The new command line for an old one.
fn translate(args: &[String]) -> Result<Vec<String>, String> {
    let mut out = vec!["fit".to_string()];
    let mut rest = args.iter();
    match rest.next().map(String::as_str) {
        Some("stimulus") => out.push("stimulus".into()),
        Some("fit") => {}
        Some(other) => return Err(format!("unknown command {other} (stimulus or fit)")),
        None => return Err("no command (stimulus or fit)".into()),
    }
    out.extend(rest.map(|a| if a == "-o" { "--out".to_string() } else { a.clone() }));
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let registry = stave_tools::registry();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        let help = |id: &str| stave_tools::cli::command_help(registry.get(id).expect("built in").info());
        println!(
            "stave-fit is deprecated and forwards to `stave` (tools/stave-cli):\n  \
             stave-fit stimulus OUT [opts]   -> stave fit stimulus OUT [opts]\n  \
             stave-fit fit -o M [opts]       -> stave fit --out M [opts]\n\n{}\n{}",
            help("fit.stimulus"),
            help("fit")
        );
        return ExitCode::SUCCESS;
    }
    let new = match translate(&args) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("stave-fit: {e} (see --help)");
            return ExitCode::from(stave_tools::cli::USAGE_EXIT);
        }
    };
    eprintln!("stave-fit is deprecated: running `stave {}`", new.join(" "));
    let Some(mut out) = stave_tools::stdout::reserve() else {
        eprintln!("stave-fit: could not reserve stdout");
        return ExitCode::FAILURE;
    };
    ExitCode::from(stave_tools::cli::run(&registry, &new, &mut out, &mut std::io::stderr()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Result<String, String> {
        translate(&s.split_whitespace().map(String::from).collect::<Vec<_>>()).map(|v| v.join(" "))
    }

    #[test]
    fn old_commands_become_new_ones() {
        assert_eq!(t("stimulus s.wav --rate 44100").unwrap(), "fit stimulus s.wav --rate 44100");
        assert_eq!(t("fit --circuit ts808 -o m.json").unwrap(), "fit --circuit ts808 --out m.json");
        assert!(t("frob").is_err());
    }
}
