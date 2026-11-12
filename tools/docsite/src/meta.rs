//! What the workspace says about itself: `cargo metadata` (crates, their
//! dependencies, the platforms a dependency is gated to) and the CI
//! workflows (which jobs run on which runners).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::Result;

/// One workspace package.
#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub description: String,
    /// The library's crate name (`stave_core`), if it has a library.
    pub lib: Option<String>,
    /// `lib`, `cdylib`, `proc-macro`, `bin` …, from its targets.
    pub kinds: BTreeSet<String>,
    /// Binary names.
    pub bins: Vec<String>,
    /// Directory relative to the workspace root.
    pub dir: String,
    /// Normal dependencies on other workspace packages: `(name, cfg)`,
    /// `cfg` set when the dependency is platform-gated.
    pub deps: Vec<(String, Option<String>)>,
    pub publish: bool,
}

pub struct Workspace {
    pub packages: Vec<Package>,
    pub version: String,
}

pub fn workspace(repo: &Path) -> Result<Workspace> {
    let out = Command::new(env!("CARGO")).args(["metadata", "--format-version", "1", "--no-deps", "--locked"]).current_dir(repo).output()?;
    if !out.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&out.stderr)).into());
    }
    let m: Value = serde_json::from_slice(&out.stdout)?;
    let root = m["workspace_root"].as_str().ok_or("no workspace_root")?.to_string();
    let members: BTreeSet<String> = m["packages"].as_array().ok_or("no packages")?.iter().filter_map(|p| p["name"].as_str().map(String::from)).collect();
    let mut packages = Vec::new();
    let mut version = String::new();
    for p in m["packages"].as_array().ok_or("no packages")? {
        let name = p["name"].as_str().unwrap_or_default().to_string();
        if name == "stave" {
            version = p["version"].as_str().unwrap_or_default().to_string();
        }
        let manifest = p["manifest_path"].as_str().unwrap_or_default();
        let dir = Path::new(manifest).parent().and_then(|d| d.strip_prefix(&root).ok()).map(|d| d.display().to_string()).unwrap_or_default();
        let mut kinds = BTreeSet::new();
        let mut bins = Vec::new();
        let mut lib = None;
        for t in p["targets"].as_array().into_iter().flatten() {
            for k in t["kind"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                match k {
                    "bin" => {
                        bins.push(t["name"].as_str().unwrap_or_default().to_string());
                        kinds.insert("bin".to_string());
                    }
                    "lib" | "rlib" | "cdylib" | "proc-macro" => {
                        lib = Some(t["name"].as_str().unwrap_or_default().replace('-', "_"));
                        kinds.insert(k.to_string());
                    }
                    _ => {}
                }
            }
        }
        let mut deps = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dn = d["name"].as_str().unwrap_or_default();
            if !members.contains(dn) || !d["kind"].is_null() {
                continue;
            }
            let cfg = d["target"].as_str().map(String::from);
            if !deps.iter().any(|(n, c): &(String, Option<String>)| n == dn && *c == cfg) {
                deps.push((dn.to_string(), cfg));
            }
        }
        let publish = !matches!(&p["publish"], Value::Array(a) if a.is_empty());
        packages.push(Package { name, description: p["description"].as_str().unwrap_or_default().to_string(), lib, kinds, bins, dir, deps, publish });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Workspace { packages, version })
}

/// A cfg fact: `unix`, or `target_os = "macos"`.
pub type Fact = (&'static str, Option<&'static str>);

/// The three platforms, as the cfg facts a target sets.
pub const PLATFORMS: &[(&str, &[Fact])] = &[
    ("macOS", &[("unix", None), ("target_os", Some("macos")), ("target_family", Some("unix")), ("target_vendor", Some("apple"))]),
    ("Windows", &[("windows", None), ("target_os", Some("windows")), ("target_family", Some("windows"))]),
    ("Linux", &[("unix", None), ("target_os", Some("linux")), ("target_family", Some("unix"))]),
];

/// Whether `cfg(...)` holds on a platform with `facts`.
pub fn cfg_holds(cfg: &str, facts: &[(&str, Option<&str>)]) -> Result<bool> {
    let inner = cfg.trim().strip_prefix("cfg(").and_then(|s| s.strip_suffix(')')).ok_or_else(|| format!("not a cfg: {cfg}"))?;
    let (v, rest) = expr(inner.trim(), facts)?;
    if !rest.trim().is_empty() {
        return Err(format!("trailing input in {cfg}").into());
    }
    Ok(v)
}

fn expr<'a>(s: &'a str, facts: &[(&str, Option<&str>)]) -> Result<(bool, &'a str)> {
    let s = s.trim_start();
    let end = s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len());
    let (ident, rest) = s.split_at(end);
    let rest = rest.trim_start();
    if let Some(args) = rest.strip_prefix('(') {
        let mut vals = Vec::new();
        let mut r = args;
        loop {
            r = r.trim_start();
            if let Some(after) = r.strip_prefix(')') {
                r = after;
                break;
            }
            let (v, after) = expr(r, facts)?;
            vals.push(v);
            r = after.trim_start().strip_prefix(',').unwrap_or(after);
        }
        let v = match ident {
            "all" => vals.iter().all(|v| *v),
            "any" => vals.iter().any(|v| *v),
            "not" if vals.len() == 1 => !vals[0],
            _ => return Err(format!("unknown cfg predicate {ident}").into()),
        };
        return Ok((v, r));
    }
    if let Some(after) = rest.strip_prefix('=') {
        let after = after.trim_start().strip_prefix('"').ok_or("cfg value without quotes")?;
        let q = after.find('"').ok_or("unterminated cfg value")?;
        let value = &after[..q];
        return Ok((facts.iter().any(|(k, v)| *k == ident && *v == Some(value)), &after[q + 1..]));
    }
    Ok((facts.iter().any(|(k, v)| *k == ident && v.is_none()), rest))
}

/// One CI job: its workflow, display name and the runners it uses.
#[derive(Clone, Debug)]
pub struct Job {
    pub workflow: String,
    pub id: String,
    pub name: String,
    pub runners: Vec<String>,
    pub trigger: String,
}

/// Every job of every workflow in `.github/workflows`, read line by line
/// (the subset of YAML those files use: two-space indents, `runs-on`
/// either a runner or `${{ matrix.os }}` over an `os: [...]` list).
pub fn jobs(repo: &Path) -> Result<Vec<Job>> {
    let dir = repo.join(".github/workflows");
    let mut files: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "yml")).collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f)?;
        let workflow = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let trigger = triggers(&text);
        let mut in_jobs = false;
        let mut cur: Option<Job> = None;
        let mut matrix_os: Vec<String> = Vec::new();
        let mut runs_on = String::new();
        let finish = |cur: &mut Option<Job>, runs_on: &str, matrix_os: &[String], out: &mut Vec<Job>| -> Result<()> {
            if let Some(mut j) = cur.take() {
                j.runners = if runs_on.contains("matrix.os") {
                    if matrix_os.is_empty() {
                        return Err(format!("{}: job {} runs on matrix.os without an os list", j.workflow, j.id).into());
                    }
                    matrix_os.to_vec()
                } else {
                    vec![runs_on.to_string()]
                };
                if j.name.is_empty() {
                    j.name = j.id.clone();
                }
                out.push(j);
            }
            Ok(())
        };
        for line in text.lines() {
            if line.starts_with("jobs:") {
                in_jobs = true;
                continue;
            }
            if !in_jobs {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            let t = line.trim();
            if t.starts_with('#') || t.is_empty() {
                continue;
            }
            if indent == 0 {
                in_jobs = false;
                continue;
            }
            if indent == 2 && t.ends_with(':') {
                finish(&mut cur, &runs_on, &matrix_os, &mut out)?;
                matrix_os.clear();
                runs_on.clear();
                cur = Some(Job { workflow: workflow.clone(), id: t.trim_end_matches(':').to_string(), name: String::new(), runners: Vec::new(), trigger: trigger.clone() });
            } else if indent == 4 && t.starts_with("name:") {
                if let Some(j) = cur.as_mut() {
                    j.name = t["name:".len()..].trim().trim_matches('"').to_string();
                }
            } else if indent == 4 && t.starts_with("runs-on:") {
                runs_on = t["runs-on:".len()..].trim().to_string();
            } else if let Some(list) = t.strip_prefix("os:") {
                matrix_os = list.trim().trim_start_matches('[').trim_end_matches(']').split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
        }
        finish(&mut cur, &runs_on, &matrix_os, &mut out)?;
    }
    if out.is_empty() {
        return Err("no CI jobs found".into());
    }
    Ok(out)
}

fn triggers(text: &str) -> String {
    let mut on = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("on:") {
            on = true;
        } else if on && !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        } else if on && line.starts_with("  ") && !line.starts_with("   ") {
            if let Some((key, _)) = line.trim().split_once(':') {
                out.push(key.replace('_', " "));
            }
        }
    }
    out.join(", ")
}

/// The platform a runner label is.
pub fn runner_platform(runner: &str) -> &'static str {
    if runner.starts_with("macos") {
        "macOS"
    } else if runner.starts_with("windows") {
        "Windows"
    } else if runner.starts_with("ubuntu") {
        "Linux"
    } else {
        "other"
    }
}

/// `(job, platforms)` grouped by workflow, for the platform page.
pub fn jobs_by_workflow(jobs: &[Job]) -> BTreeMap<String, Vec<&Job>> {
    let mut m: BTreeMap<String, Vec<&Job>> = BTreeMap::new();
    for j in jobs {
        m.entry(j.workflow.clone()).or_default().push(j);
    }
    m
}
