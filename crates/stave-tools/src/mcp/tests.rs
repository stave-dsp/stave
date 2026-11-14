//! A scripted MCP client: initialize, list the tools, call describe and
//! bench on a catalog stage, a session across calls, progress, errors and
//! resources.

use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::*;

/// A writer the test reads back after the server ends.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Serve `requests` (one JSON message per line) to the end; every
/// message the server sent.
fn session(requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let sink = Sink::default();
    serve(crate::registry(), Cursor::new(input), Box::new(sink.clone())).unwrap();
    let bytes = sink.0.lock().unwrap().clone();
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn response(messages: &[Value], id: u64) -> &Value {
    messages
        .iter()
        .find(|m| m["id"] == id && m.get("method").is_none())
        .unwrap_or_else(|| panic!("no response {id} in {messages:#?}"))
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    request(id, "tools/call", json!({ "name": tool, "arguments": arguments }))
}

#[test]
fn initializes_lists_and_calls() {
    let wav = std::env::temp_dir().join(format!("stave-mcp-{}.wav", std::process::id()));
    std::fs::write(&wav, b"").unwrap(); // so its URI is the same before and after
    let uri = file_uri(&wav);
    let m = session(&[
        request(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } })),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        request(2, "tools/list", json!({})),
        call(3, "describe", json!({ "target": "stage:clipper/diode" })),
        request(
            4,
            "tools/call",
            json!({ "name": "bench", "arguments": { "target": "stage:clipper/diode", "seconds": 0.2, "out": wav.display().to_string() }, "_meta": { "progressToken": "b" } }),
        ),
        request(5, "resources/list", json!({})),
        request(6, "ping", json!({})),
        request(7, "resources/read", json!({ "uri": uri })),
    ]);
    let init = &response(&m, 1)["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "stave");
    assert!(init["capabilities"]["tools"].is_object());

    let tools = response(&m, 2)["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in ["describe", "bench", "validate", "fit_stimulus", "corpus_check", "characterise"] {
        assert!(names.contains(&want), "{want} not in {names:?}");
    }
    let bench = tools.iter().find(|t| t["name"] == "bench").unwrap();
    assert_eq!(bench["inputSchema"]["required"], json!(["target"]));
    assert_eq!(bench["outputSchema"]["type"], "object");

    let d = &response(&m, 3)["result"];
    assert_eq!(d["isError"], false);
    assert_eq!(d["structuredContent"]["layout"]["outputs"], 2);
    assert_eq!(d["content"][0]["type"], "text");

    let b = &response(&m, 4)["result"];
    assert_eq!(b["isError"], false, "{b}");
    assert_eq!(b["structuredContent"]["sample_rate"], 48000.0);
    let link = b["content"].as_array().unwrap().iter().find(|c| c["type"] == "resource_link").unwrap();
    assert_eq!(link["mimeType"], "audio/wav");
    let progress: Vec<&Value> = m.iter().filter(|x| x["method"] == "notifications/progress").collect();
    assert!(!progress.is_empty(), "a long command reports progress");
    assert!(progress.iter().all(|p| p["params"]["progressToken"] == "b"));
    let values: Vec<f64> = progress.iter().map(|p| p["params"]["progress"].as_f64().unwrap()).collect();
    assert!(values.windows(2).all(|w| w[1] > w[0]), "progress rises: {values:?}");

    let resources = response(&m, 5)["result"]["resources"].as_array().unwrap();
    assert_eq!(resources[0]["uri"], link["uri"]);
    assert_eq!(response(&m, 6)["result"], json!({}));
    let read = &response(&m, 7)["result"]["contents"][0];
    assert_eq!((read["uri"].as_str(), read["mimeType"].as_str()), (Some(uri.as_str()), Some("audio/wav")));
    assert!(read["blob"].as_str().unwrap().starts_with("UklGR"), "a RIFF file, base64");
}

#[test]
fn a_session_lives_across_calls() {
    let m = session(&[
        request(1, "initialize", json!({ "protocolVersion": "2025-06-18" })),
        call(2, "open", json!({ "target": "stage:clipper/diode" })),
        call(3, "set", json!({ "target": "@1", "set": ["threshold=1.5"] })),
        call(4, "params", json!({ "target": "@1" })),
    ]);
    assert_eq!(response(&m, 2)["result"]["structuredContent"]["session"], "@1");
    let params = response(&m, 4)["result"]["structuredContent"]["params"].as_array().unwrap().clone();
    let t = params.iter().find(|p| p["name"] == "threshold").unwrap();
    assert_eq!(t["value"], 1.5);
}

#[test]
fn errors_are_reported_where_mcp_puts_them() {
    let m = session(&[
        request(1, "initialize", json!({ "protocolVersion": "1999-01-01" })),
        call(2, "nope", json!({})),
        call(3, "describe", json!({ "target": "stage:nope" })),
        call(4, "bench", json!({ "target": "stage:coupler", "seconds": "soon" })),
        request(5, "frobnicate", json!({})),
        request(6, "resources/read", json!({ "uri": "file:///etc/passwd" })),
    ]);
    assert_eq!(response(&m, 1)["result"]["protocolVersion"], VERSIONS[0], "an unknown version gets ours");
    assert_eq!(response(&m, 2)["error"]["code"], -32602);
    let d = &response(&m, 3)["result"];
    assert_eq!(d["isError"], true, "a tool that fails says so in its result");
    assert!(d["content"][0]["text"].as_str().unwrap().contains("filter/peak"));
    assert_eq!(response(&m, 4)["result"]["isError"], true);
    assert_eq!(response(&m, 5)["error"]["code"], -32601);
    assert_eq!(response(&m, 6)["error"]["code"], -32002, "only files a command wrote are readable");
}

#[test]
fn older_protocols_get_plain_content() {
    let m = session(&[
        request(1, "initialize", json!({ "protocolVersion": "2024-11-05" })),
        request(2, "tools/list", json!({})),
        call(3, "stages", json!({})),
    ]);
    let tools = response(&m, 2)["result"]["tools"].as_array().unwrap().clone();
    assert!(tools.iter().all(|t| t.get("outputSchema").is_none()));
    let r = &response(&m, 3)["result"];
    assert!(r.get("structuredContent").is_none());
    assert!(r["content"].as_array().unwrap().iter().any(|c| c["text"].as_str().unwrap_or("").contains("stage:coupler")));
}

#[test]
fn a_cancelled_call_stops() {
    let m = session(&[
        request(1, "initialize", json!({ "protocolVersion": "2025-06-18" })),
        call(2, "validate", json!({ "target": "stage:coupler" })),
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 2, "reason": "test" } }),
    ]);
    let r = &response(&m, 2)["result"];
    assert_eq!(r["isError"], true, "{r}");
    assert_eq!(r["content"][0]["text"], "cancelled");
}

#[test]
fn base64_matches_the_rfc_vectors() {
    for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
        assert_eq!(base64(raw.as_bytes()), enc);
    }
    assert!(file_uri(std::path::Path::new("/a b/c.wav")).ends_with("/a%20b/c.wav"));
}
