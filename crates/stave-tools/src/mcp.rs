//! The MCP layer: `stave mcp` serves every registry command as a Model
//! Context Protocol tool over stdio — newline-delimited JSON-RPC 2.0 —
//! with its input and output schemas, progress notifications for
//! long-running commands, cancellation, and the files commands write as
//! resources.
//!
//! The protocol is implemented here, on serde_json, rather than through
//! an SDK crate: the server side stave needs is six methods and two
//! notifications (about 400 lines), where the SDKs bring an async
//! runtime and a macro layer for the same messages — nowhere near the
//! 10–50x a dependency has to win by.
//!
//! ```text
//! initialize                 protocol version (2025-06-18, 2025-03-26, 2024-11-05), capabilities
//! ping
//! tools/list                 every command: name (id, `.` → `_`), title, description, schemas
//! tools/call                 run it; text + structuredContent (+ resource links, images)
//! resources/list, read       the files commands wrote this session (only those)
//! notifications/cancelled    cancel a running call
//! notifications/progress     sent for a call that gave a progressToken
//! ```
//!
//! Calls run one at a time on a worker thread that keeps the session
//! [`Workspace`], so `open` then `bench @1` work on one instance; the
//! reader stays free to take cancellations.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};

use crate::command::{Artefact, Cancel, CommandError, Context, Outcome, Progress, Status};
use crate::registry::{input_schema, output_schema, Registry};
use crate::target::Workspace;

/// Protocol versions spoken, newest first.
pub const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "stave hosts and measures audio plugins and stave-dsp models. A target is \
PLUGIN.vst3, au:NAME (macOS), stage:NAME (call `stages` for the catalog) or @N, a session from `open`, \
then optional ;name=value param settings, ;preset=NAME. `open` once and pass @N to keep one instance \
between calls (set, describe, bench, process). Long commands (bench, validate, characterise, fit) report \
progress when given a progressToken and can be cancelled. Files a command writes come back as resource \
links and are readable through resources/read.";

/// One `tools/call` for the worker.
struct Job {
    id: Value,
    command: &'static str,
    arguments: Value,
    progress_token: Option<Value>,
    cancel: Cancel,
}

/// What the worker does, in the order requests came: a call, or a
/// resources request (after the calls before it, so it sees their files).
enum Work {
    Call(Job),
    Resources { id: Value, method: String, params: Value },
}

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn send(out: &Out, message: &Value) {
    if let Ok(mut w) = out.lock() {
        let _ = writeln!(w, "{message}");
        let _ = w.flush();
    }
}

fn reply(out: &Out, id: &Value, result: Value) {
    send(out, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn error(out: &Out, id: &Value, code: i64, message: impl Into<String>) {
    send(out, &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } }));
}

/// What the session negotiated.
#[derive(Clone, Copy)]
struct Features {
    /// outputSchema, structuredContent, resource_link, title (2025-06-18).
    structured: bool,
}

/// Serve `registry` until `input` ends. Calls still running finish
/// first.
pub fn serve(registry: Registry, input: impl BufRead, output: Box<dyn Write + Send>) -> std::io::Result<()> {
    let registry = Arc::new(registry);
    let out: Out = Arc::new(Mutex::new(output));
    let running: Arc<Mutex<HashMap<String, Cancel>>> = Arc::default();
    let features = Arc::new(Mutex::new(Features { structured: true }));
    let (jobs, queue) = mpsc::channel::<Work>();
    let worker = {
        let (registry, out, running, features) = (registry.clone(), out.clone(), running.clone(), features.clone());
        thread::spawn(move || {
            let mut workspace = Workspace::default();
            let mut artefacts: Vec<Artefact> = Vec::new();
            for work in queue {
                let job = match work {
                    Work::Call(job) => job,
                    Work::Resources { id, method, params } => {
                        resources(&out, &id, &method, &params, &artefacts);
                        continue;
                    }
                };
                let f = features.lock().map(|f| *f).unwrap_or(Features { structured: true });
                let result = call(&registry, &mut workspace, &job, &out);
                if let Ok(o) = &result {
                    artefacts.extend(o.artefacts.iter().cloned());
                }
                if let Ok(mut r) = running.lock() {
                    r.remove(&job.id.to_string());
                }
                reply(&out, &job.id, tool_result(result, f));
            }
        })
    };
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                error(&out, &Value::Null, -32700, format!("parse error: {e}"));
                continue;
            }
        };
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            continue; // a response to us: we send no requests
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = msg.get("id").cloned() else {
            if method == "notifications/cancelled" {
                let key = params.get("requestId").map(Value::to_string).unwrap_or_default();
                if let Some(c) = running.lock().ok().and_then(|r| r.get(&key).cloned()) {
                    c.cancel();
                }
            }
            continue; // notifications/initialized and others need nothing
        };
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(VERSIONS[0]);
                let version = VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(VERSIONS[0]);
                if let Ok(mut f) = features.lock() {
                    f.structured = version == VERSIONS[0];
                }
                reply(
                    &out,
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false } },
                        "serverInfo": { "name": "stave", "title": "stave", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": INSTRUCTIONS,
                    }),
                );
            }
            "ping" => reply(&out, &id, json!({})),
            "tools/list" => {
                let structured = features.lock().map(|f| f.structured).unwrap_or(true);
                reply(&out, &id, json!({ "tools": tools(&registry, structured) }));
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let Some(command) = registry.by_tool_name(name) else {
                    error(&out, &id, -32602, format!("unknown tool '{name}'"));
                    continue;
                };
                let cancel = Cancel::default();
                if let Ok(mut r) = running.lock() {
                    r.insert(id.to_string(), cancel.clone());
                }
                let job = Job {
                    id: id.clone(),
                    command: command.info().id,
                    arguments: params.get("arguments").cloned().unwrap_or(json!({})),
                    progress_token: params.pointer("/_meta/progressToken").cloned(),
                    cancel,
                };
                if jobs.send(Work::Call(job)).is_err() {
                    error(&out, &id, -32603, "the worker stopped");
                }
            }
            "resources/list" | "resources/read" => {
                let work = Work::Resources { id: id.clone(), method: method.into(), params };
                if jobs.send(work).is_err() {
                    error(&out, &id, -32603, "the worker stopped");
                }
            }
            "resources/templates/list" => reply(&out, &id, json!({ "resourceTemplates": [] })),
            other => error(&out, &id, -32601, format!("method not found: {other}")),
        }
    }
    drop(jobs);
    let _ = worker.join();
    Ok(())
}

/// `resources/list` or `resources/read`, over the files written so far.
fn resources(out: &Out, id: &Value, method: &str, params: &Value, artefacts: &[Artefact]) {
    if method == "resources/list" {
        let list: Vec<Value> = artefacts.iter().map(resource).collect();
        return reply(out, id, json!({ "resources": list }));
    }
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
    match artefacts.iter().find(|a| file_uri(&a.path) == uri).map(read) {
        Some(Ok(contents)) => reply(out, id, json!({ "contents": [contents] })),
        Some(Err(e)) => error(out, id, -32603, format!("{uri}: {e}")),
        None => error(out, id, -32002, format!("no resource {uri} (only files this session wrote)")),
    }
}

/// Run one call, sending its progress.
fn call(registry: &Registry, workspace: &mut Workspace, job: &Job, out: &Out) -> Result<Outcome, CommandError> {
    let mut last: Option<f64> = None;
    let mut progress = |p: &Progress| {
        let Some(token) = &job.progress_token else { return };
        // MCP wants progress to rise with every notification: a known
        // extent reports `done` (skipping a repeat), an unknown one counts.
        let value = match p.total {
            Some(_) if last.is_some_and(|l| p.done <= l) => return,
            Some(_) => p.done,
            None => last.map_or(1.0, |l| l.floor() + 1.0),
        };
        last = Some(value);
        let mut params = json!({ "progressToken": token, "progress": value, "message": p.message });
        if let Some(t) = p.total {
            params["total"] = json!(t);
        }
        send(out, &json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": params }));
    };
    let mut cx = Context::new(workspace, &mut progress, job.cancel.clone());
    registry.run(job.command, job.arguments.clone(), &mut cx)
}

/// A call's result as MCP content.
fn tool_result(result: Result<Outcome, CommandError>, f: Features) -> Value {
    match result {
        Ok(o) => {
            let mut text = o.text.clone();
            if o.status == Status::Failed {
                text = format!("status: failed\n{text}");
            }
            let mut content = vec![json!({ "type": "text", "text": text })];
            for a in &o.artefacts {
                if f.structured {
                    content.push(json!({
                        "type": "resource_link", "uri": file_uri(&a.path), "name": file_name(&a.path),
                        "mimeType": a.mime, "description": a.role,
                    }));
                }
                if a.mime.starts_with("image/") {
                    if let Ok(bytes) = std::fs::read(&a.path) {
                        content.push(json!({ "type": "image", "data": base64(&bytes), "mimeType": a.mime }));
                    }
                }
            }
            if !f.structured {
                for a in &o.artefacts {
                    content.push(json!({ "type": "text", "text": format!("wrote {} ({})", file_uri(&a.path), a.role) }));
                }
                content.push(json!({ "type": "text", "text": o.data.to_string() }));
                return json!({ "content": content, "isError": false });
            }
            json!({ "content": content, "structuredContent": o.data, "isError": false })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
    }
}

/// Every command as an MCP tool.
fn tools(registry: &Registry, structured: bool) -> Vec<Value> {
    registry
        .commands()
        .map(|c| {
            let i = c.info();
            let mut t = json!({
                "name": i.tool_name(),
                "description": format!("{}\n\n{}", i.summary, i.description),
                "inputSchema": input_schema(i),
            });
            if structured {
                t["title"] = json!(i.title);
                t["outputSchema"] = output_schema(i);
            }
            t
        })
        .collect()
}

fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.into()))
}

/// `file:///abs/path`, spaces and the like percent-encoded.
pub fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in absolute(p).display().to_string().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => s.push(b as char),
            b'\\' => s.push('/'),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn resource(a: &Artefact) -> Value {
    json!({ "uri": file_uri(&a.path), "name": file_name(&a.path), "mimeType": a.mime, "description": a.role })
}

fn read(a: &Artefact) -> std::io::Result<Value> {
    let bytes = std::fs::read(&a.path)?;
    let uri = file_uri(&a.path);
    let textual = a.mime.starts_with("text/") || a.mime == "application/json";
    Ok(match String::from_utf8(bytes) {
        Ok(text) if textual => json!({ "uri": uri, "mimeType": a.mime, "text": text }),
        Ok(text) => json!({ "uri": uri, "mimeType": a.mime, "blob": base64(text.as_bytes()) }),
        Err(e) => json!({ "uri": uri, "mimeType": a.mime, "blob": base64(e.as_bytes()) }),
    })
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests;
