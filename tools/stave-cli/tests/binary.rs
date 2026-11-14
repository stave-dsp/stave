//! The `stave` binary end to end: a scripted MCP client over its real
//! stdin and stdout, and the CLI's exit codes and JSON.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const STAVE: &str = env!("CARGO_BIN_EXE_stave");

#[test]
fn an_mcp_client_lists_and_calls_tools_over_stdio() {
    let mut child = Command::new(STAVE)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |id: u64, method: &str, params: Value| -> Value {
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        loop {
            let reply: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            if reply["id"] == id {
                return reply;
            }
            assert_eq!(reply["method"], "notifications/progress", "only progress comes between: {reply}");
        }
    };
    let init = ask(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }));
    assert_eq!(init["result"]["serverInfo"]["name"], "stave");
    let tools = ask(2, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for want in ["describe", "bench", "null", "verify", "materials", "screenshot", "fit"] {
        assert!(names.contains(&want), "{want} not in {names:?}");
    }
    let d = ask(3, "tools/call", json!({ "name": "describe", "arguments": { "target": "stage:clipper/diode" } }));
    assert_eq!(d["result"]["structuredContent"]["layout"]["outputs"], 2, "{d}");
    let b = ask(
        4,
        "tools/call",
        json!({ "name": "bench", "arguments": { "target": "stage:clipper/diode", "seconds": 0.2 }, "_meta": { "progressToken": 1 } }),
    );
    assert_eq!(b["result"]["isError"], false, "{b}");
    assert!(b["result"]["structuredContent"]["cpu"].is_object());
    drop(stdin);
    assert!(child.wait().unwrap().success(), "stave mcp ends cleanly at end of input");
}

#[test]
fn the_cli_prints_json_and_says_why_by_exit_code() {
    let run = |args: &[&str]| Command::new(STAVE).args(args).output().unwrap();
    let ok = run(&["describe", "stage:coupler", "--json"]);
    assert_eq!(ok.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(v["layout"]["inputs"], 2);
    assert_eq!(run(&["describe", "stage:nope"]).status.code(), Some(3));
    assert_eq!(run(&["bench"]).status.code(), Some(2), "a missing target is a usage error");
    let help = run(&["--help"]);
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("mcp") && text.contains("play") && text.contains("characterise"), "{text}");
}
