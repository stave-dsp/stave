//! The built-in commands end to end on catalog stages, through the
//! registry as every layer runs them.

use serde_json::{json, Value};

use crate::command::{Cancel, CommandError, Context, Outcome, Progress, Status};
use crate::registry::{input_schema, Registry};
use crate::target::Workspace;

fn run_in(r: &Registry, ws: &mut Workspace, id: &str, input: Value) -> (Result<Outcome, CommandError>, Vec<Progress>) {
    let mut seen = Vec::new();
    let mut sink = |p: &Progress| seen.push(p.clone());
    let out = r.run(id, input, &mut Context::new(ws, &mut sink, Cancel::default()));
    (out, seen)
}

/// Run, and check the data holds every output the command declares as
/// required.
fn run(ws: &mut Workspace, id: &str, input: Value) -> Outcome {
    let r = crate::registry();
    let out = run_in(&r, ws, id, input.clone()).0.unwrap_or_else(|e| panic!("{id} {input}: {e}"));
    for f in r.get(id).unwrap().info().outputs.iter().filter(|f| f.required) {
        assert!(out.data.get(f.name).is_some(), "{id}: no {} in {}", f.name, out.data);
    }
    out
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("stave-tools-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn every_command_is_unique_and_documented() {
    let r = crate::registry();
    let mut ids: Vec<&str> = r.commands().map(|c| c.info().id).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate ids");
    for c in r.commands() {
        let i = c.info();
        assert!(!i.title.is_empty() && !i.summary.is_empty() && !i.description.is_empty(), "{}", i.id);
        assert!(i.inputs.iter().chain(i.outputs).all(|f| !f.help.is_empty()), "{}: a field has no help", i.id);
        assert_eq!(input_schema(i)["type"], "object");
        // Positionals first: the command line takes them in order.
        let first_flag = i.inputs.iter().position(|f| !f.positional).unwrap_or(i.inputs.len());
        assert!(i.inputs[first_flag..].iter().all(|f| !f.positional), "{}: positional after a flag", i.id);
    }
}

#[test]
fn describes_a_stage() {
    let out = run(&mut Workspace::default(), "describe", json!({ "target": "stage:clipper/diode" }));
    assert_eq!(out.data["layout"]["outputs"], 2);
    assert!(out.data["params"].as_array().unwrap().iter().any(|p| p["name"] == "threshold"), "{}", out.data);
    assert!(out.text.contains("params ("), "{}", out.text);
}

#[test]
fn sessions_keep_their_settings() {
    let mut ws = Workspace::default();
    let out = run(&mut ws, "open", json!({ "target": "stage:clipper/diode;threshold=0.25", "rate": 44100 }));
    assert_eq!(out.data["session"], "@1");
    let threshold = |out: &Outcome| {
        out.data["params"].as_array().unwrap().iter().find(|p| p["name"] == "threshold").unwrap()["value"].as_f64().unwrap()
    };
    assert!((threshold(&run(&mut ws, "params", json!({ "target": "@1" }))) - 0.25).abs() < 1e-9);
    let set = run(&mut ws, "set", json!({ "target": "@1", "set": ["threshold=0.75"] }));
    assert!((threshold(&set) - 0.75).abs() < 1e-9);
    let d = run(&mut ws, "describe", json!({ "target": "@1" }));
    assert_eq!(d.data["layout"]["sample_rate"], 44100.0, "the session keeps its rate");
    let r = crate::registry();
    let (e, _) = run_in(&r, &mut ws, "set", json!({ "target": "stage:coupler", "set": ["x=1"] }));
    assert!(matches!(e, Err(CommandError::Usage(_))), "set needs a session");
    run(&mut ws, "close", json!({ "session": "@1" }));
    let (e, _) = run_in(&r, &mut ws, "params", json!({ "target": "@1" }));
    assert!(matches!(e, Err(CommandError::Usage(m)) if m.contains("no session @1")));
}

#[test]
fn a_bad_setting_fails_naming_it() {
    let r = crate::registry();
    let (e, _) = run_in(&r, &mut Workspace::default(), "params", json!({ "target": "stage:coupler", "set": ["nope=1"] }));
    assert!(matches!(e, Err(CommandError::Failed(m)) if m.contains("nope")));
    let (e, _) = run_in(&r, &mut Workspace::default(), "describe", json!({ "target": "stage:nope" }));
    assert!(matches!(e, Err(CommandError::Failed(m)) if m.contains("filter/peak")), "the miss lists the catalog");
}

#[test]
fn benches_and_processes() {
    let wav = scratch("bench.wav");
    let out = run(
        &mut Workspace::default(),
        "bench",
        json!({ "target": "stage:clipper/diode", "seconds": 0.25, "out": wav.display().to_string(), "measure": "peak,rms" }),
    );
    assert_eq!(out.status, Status::Ok);
    assert_eq!(out.data["sample_rate"], 48000.0);
    assert_eq!(out.artefacts.len(), 1);
    assert!(wav.is_file());
    let wav = scratch("process.wav");
    let out = run(
        &mut Workspace::default(),
        "process",
        json!({ "target": "stage:coupler", "seconds": 0.1, "out": wav.display().to_string(), "signal": "sweep:20:2000" }),
    );
    assert_eq!((out.data["channels"].as_u64(), out.data["frames"].as_u64()), (Some(2), Some(4800)));
    assert_eq!(stave_host::wav::read(&wav).unwrap().channels[0].len(), 4800);
}

#[test]
fn validates_with_progress_per_check() {
    let r = crate::registry();
    let (out, progress) = run_in(&r, &mut Workspace::default(), "validate", json!({ "target": "stage:coupler" }));
    let out = out.unwrap();
    let checks = out.data["checks"].as_array().unwrap().len();
    assert_eq!(progress.len(), checks);
    assert_eq!(progress.last().unwrap().total, Some(checks as f64));
    assert_eq!(out.status == Status::Ok, out.data["failures"] == 0);
}

#[test]
fn cancelled_before_a_check_stops() {
    let r = crate::registry();
    let cancel = Cancel::default();
    cancel.cancel();
    let mut ws = Workspace::default();
    let mut sink = |_: &Progress| {};
    let out = r.run("validate", json!({ "target": "stage:coupler" }), &mut Context::new(&mut ws, &mut sink, cancel));
    assert_eq!(out, Err(CommandError::Cancelled));
}

#[test]
fn response_reads_each_level() {
    let out = run(&mut Workspace::default(), "response", json!({ "target": "stage:coupler", "levels": "-24,-6" }));
    assert_eq!(out.data["levels_db"], json!([-24.0, -6.0]));
}

#[test]
fn stages_lists_the_catalog() {
    let out = run(&mut Workspace::default(), "stages", json!({}));
    let names: Vec<&str> = out.data["stages"].as_array().unwrap().iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"stage:clipper/diode"), "{names:?}");
}
