//! The command line, parsed and run against the built-in registry.

use serde_json::{json, Value};

use super::*;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn run_line(line: &str) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&crate::registry(), &args(line), &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

#[test]
fn words_resolve_to_the_longest_id() {
    let r = crate::registry();
    let (_, req) = parse(&r, &args("fit stimulus s.wav --rate 44100")).unwrap();
    assert_eq!(req, Request::Run { id: "fit.stimulus", input: json!({ "out": "s.wav", "rate": "44100" }) });
    let (_, req) = parse(&r, &args("fit --out m.json --circuit ts808")).unwrap();
    assert!(matches!(req, Request::Run { id: "fit", .. }));
    let (g, req) = parse(&r, &args("help corpus check --json")).unwrap();
    assert_eq!((g.json, req), (true, Request::Help(Some("corpus.check"))));
    assert_eq!(parse(&r, &args("bench X --help")).unwrap().1, Request::Help(Some("bench")));
    assert!(matches!(parse(&r, &args("frobnicate")), Err(CommandError::Usage(m)) if m.contains("frobnicate")));
}

#[test]
fn flags_follow_their_fields() {
    let r = crate::registry();
    let (g, req) = parse(
        &r,
        &args("bench stage:coupler --set a=1 --set=b=2 --seconds 0.5 --json -q --analysis-window 4096"),
    )
    .unwrap();
    assert_eq!((g.json, g.quiet), (true, true));
    let Request::Run { input, .. } = req else { panic!() };
    assert_eq!(
        input,
        json!({ "target": "stage:coupler", "set": ["a=1", "b=2"], "seconds": "0.5", "analysis_window": "4096" })
    );
    let (_, req) = parse(&r, &args("characterise stage:a stage:b --tier t0")).unwrap();
    let Request::Run { input, .. } = req else { panic!() };
    assert_eq!(input["targets"], json!(["stage:a", "stage:b"]), "a list positional takes the rest");
    for (line, says) in [
        ("bench X --frob 1", "unknown option --frob"),
        ("bench X --seconds", "--seconds needs a value"),
        ("describe A B", "unexpected argument 'B'"),
    ] {
        assert!(matches!(parse(&r, &args(line)), Err(CommandError::Usage(m)) if m.contains(says)), "{line}");
    }
}

#[test]
fn json_prints_the_data_and_text_prints_the_text() {
    let (code, out, _) = run_line("describe stage:coupler --json");
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["layout"]["outputs"], 2);
    let (code, out, _) = run_line("params stage:coupler");
    assert_eq!(code, 0);
    assert!(!out.trim_start().starts_with('{'), "{out}");
}

#[test]
fn exit_codes_tell_usage_from_failure() {
    let (code, out, err) = run_line("describe stage:nope --json");
    assert_eq!(code, 3, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["kind"], "failed");
    let (code, _, err) = run_line("bench stage:coupler --seconds -1");
    assert_eq!(code, 2, "{err}");
    let (code, _, _) = run_line("nonsense");
    assert_eq!(code, 2);
}

#[test]
fn every_command_has_help() {
    let r = crate::registry();
    let (code, out, _) = run_line("help");
    assert_eq!(code, 0);
    for c in r.commands() {
        let words = c.info().id.replace('.', " ");
        assert!(out.contains(&format!("  {words}")), "top-level help lacks {words}");
        let h = command_help(c.info());
        assert!(h.starts_with(&format!("stave {words} — ")), "{h}");
        for f in c.info().inputs.iter().filter(|f| !f.positional) {
            assert!(h.contains(&f.flag_name()), "{words}: help lacks {}", f.flag_name());
        }
        assert!(h.lines().all(|l| l.chars().count() <= 100), "{words}: a long line in\n{h}");
    }
    let (_, out, _) = run_line("commands --json");
    let list: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(list.as_array().unwrap().len(), r.commands().count());
}
