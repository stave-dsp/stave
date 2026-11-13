//! The model catalogue: every public model in stave-dsp with its layer,
//! what it models, how it was derived, the test that holds it to its
//! reference, its cost and its anti-aliasing, read from the model's own
//! rustdoc (DSP.md naming rule 7).
//!
//! A type is a model when it lives in a model layer (`behavioral`,
//! `circuit`, `physical`, `learned`, `effects`) and runs as a `Stage` or
//! implements a device family's trait (`circuit::devices::*`), or when
//! its rustdoc has a *Models* section anywhere in the crate (how a
//! primitive that is a published model opts in). A function in
//! `circuit::topologies` with a *Models* section is one too: a classic
//! circuit as data, the netlist it returns. Every model has the
//! six sections; the build fails when one is missing or empty, when
//! *Verified* names no test, or a test (`path/to/file.rs::name`) that
//! is not a `#[test]` in the workspace, or a test file (`path/to/file.rs`)
//! that does not exist or holds none, or when *Cost* quotes a
//! datasheet stage (`"Slew"`) that `crates/stave-dsp/datasheet.json`
//! does not measure. The catalogue shows each quoted stage's committed
//! ns/sample, so the docs never carry a number that drifts.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

use serde_json::Value;

use crate::html::{esc, rel, text_of};
use crate::reference::{Ctx, Generated, GITHUB};
use crate::rustdoc::{self, Item};
use crate::Result;

/// The sections every model's rustdoc has, in order.
pub const SECTIONS: [&str; 6] = ["Models", "Derivation", "Verified", "Cost", "Aliasing", "Valid ranges"];

/// The layers whose stages are models, in the layer table's order (and
/// `primitives`, for the models that opt in).
const LAYERS: [(&str, &str); 6] = [
    ("primitives", "textbook DSP; a primitive is catalogued when it is a published model"),
    ("behavioral", "a shape chosen or fitted, with no hardware claim"),
    ("circuit", "derived from a schematic"),
    ("physical", "derived from mechanics or acoustics"),
    ("learned", "trained from captures"),
    ("effects", "algorithmic designs that model no hardware"),
];

/// Types in a model layer that run as stages but model nothing
/// themselves, and why. Anything else there is a model.
const NOT_MODELS: &[(&str, &str)] = &[
    ("stave_dsp::circuit::audio::AudioAdapter", "runs a quiescent circuit as an audio stage; the circuit it wraps is the model"),
    ("stave_dsp::circuit::devices::triode::Tube", "a runtime choice among the triode models, each catalogued on its own"),
    ("stave_dsp::circuit::devices::opamp::Amp", "a runtime choice among the op-amp models, each catalogued on its own"),
    ("stave_dsp::circuit::devices::diode::Junction", "a Shockley junction with its breakdown knee solved, as circuit methods hold it; the model is Shockley"),
];

/// One catalogued model.
struct Model {
    item: Item,
    layer: &'static str,
    /// Section heading → its HTML, in [`SECTIONS`] order.
    sections: Vec<String>,
    /// `(as written, file, test, line)` of each test *Verified* names.
    tests: Vec<(String, String, String, usize)>,
    /// `(stage, ns/sample)` of each datasheet stage *Cost* quotes.
    costs: Vec<(String, f64)>,
}

/// The committed datasheet (DSP.md P1): stage label → ns/sample, and
/// the machine it was measured on.
fn datasheet(cx: &Ctx<'_>) -> Result<(BTreeMap<String, f64>, String)> {
    const PATH: &str = "crates/stave-dsp/datasheet.json";
    let text = fs::read_to_string(cx.repo.join(PATH)).map_err(|e| format!("{PATH}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{PATH}: {e}"))?;
    let machine = v["machine"]["name"].as_str().ok_or_else(|| format!("{PATH}: no machine.name"))?.to_string();
    let mut stages = BTreeMap::new();
    for s in v["stages"].as_array().ok_or_else(|| format!("{PATH}: no stages"))? {
        let (Some(name), Some(ns)) = (s["stage"].as_str(), s["ns_per_sample"].as_f64()) else {
            return Err(format!("{PATH}: a stage without a name or ns_per_sample").into());
        };
        stages.insert(name.to_string(), ns);
    }
    Ok((stages, machine))
}

/// `(heading text, html up to the next heading)` of every heading in a
/// docblock.
fn sections_of(doc: &str) -> Vec<(String, String)> {
    let mut heads: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while let Some(k) = doc[i..].find("<h") {
        let at = i + k;
        let level = doc.as_bytes().get(at + 2).copied().unwrap_or(0);
        if !(b'1'..=b'6').contains(&level) {
            i = at + 2;
            continue;
        }
        let close = format!("</h{}>", level as char);
        let (Some(open_end), Some(end)) = (doc[at..].find('>'), doc[at..].find(&close)) else { break };
        let text = text_of(&doc[at + open_end + 1..at + end]).trim_start_matches('§').trim().to_string();
        heads.push((at, at + end + close.len(), text));
        i = at + end;
    }
    heads.iter().enumerate().map(|(n, (_, body, text))| (text.clone(), doc[*body..heads.get(n + 1).map_or(doc.len(), |h| h.0)].to_string())).collect()
}

/// The text of every `<code>` span in `html`.
fn code_spans(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("<code>") {
        rest = &rest[i + 6..];
        let Some(end) = rest.find("</code>") else { break };
        out.push(text_of(&rest[..end]));
        rest = &rest[end..];
    }
    out
}

/// `file` (`crates/stave-test/tests/x.rs`, or `stave-test/tests/x.rs`
/// from `crates/`) as the workspace path it names, and its text.
fn source(cx: &Ctx<'_>, file: &str) -> Option<(String, String)> {
    [file.to_string(), format!("crates/{file}")].into_iter().find_map(|f| fs::read_to_string(cx.repo.join(&f)).ok().map(|s| (f, s)))
}

/// The workspace path of `file` and the line of `#[test] fn name` in
/// it, or why not.
fn find_test(cx: &Ctx<'_>, file: &str, name: &str) -> std::result::Result<(String, usize), String> {
    let (file, src) = source(cx, file).ok_or_else(|| format!("names {file}::{name}, but {file} does not exist"))?;
    let lines: Vec<&str> = src.lines().collect();
    let decl = format!("fn {name}(");
    for (n, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with(&decl) && !line.contains(&format!(" {decl}")) {
            continue;
        }
        // The attributes above it: a `#[test]` among them.
        let above = lines[..n].iter().rev().take_while(|l| l.trim_start().starts_with("#[") || l.trim_start().starts_with("//"));
        if above.clone().any(|l| l.trim_start().starts_with("#[test")) {
            return Ok((file, n + 1));
        }
        return Err(format!("names {file}::{name}, which is a function but not a #[test]"));
    }
    Err(format!("names {file}::{name}, but {file} has no fn {name}"))
}

/// The model layer of an item path (`stave_dsp::circuit::…` → `circuit`).
fn layer_of(path: &str) -> Option<&'static str> {
    let seg = path.strip_prefix("stave_dsp::")?.split("::").next()?;
    LAYERS.iter().map(|(l, _)| *l).find(|l| *l == seg)
}

/// Every public model in stave-dsp, checked: an error lists every model
/// that lacks a section or names a test that does not exist.
fn models(cx: &Ctx<'_>, measured: &BTreeMap<String, f64>) -> Result<Vec<Model>> {
    let items = rustdoc::items(cx.doc, "stave_dsp")?;
    let family_traits: Vec<String> =
        items.iter().filter(|i| i.kind == "trait" && i.path.starts_with("stave_dsp::circuit::devices::")).map(|i| i.path.rsplit("::").next().unwrap_or_default().to_string()).collect();
    if family_traits.is_empty() {
        return Err("stave_dsp::circuit::devices has no device family traits; the model catalogue cannot find the devices".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let mut problems = Vec::new();
    // Types, and the classic circuits as data: functions in
    // `circuit::topologies` that return a netlist and say what it models.
    let catalogued = |i: &&rustdoc::Item| matches!(i.kind.as_str(), "struct" | "enum") || (i.kind == "fn" && i.path.starts_with("stave_dsp::circuit::topologies::"));
    for item in items.iter().filter(catalogued) {
        if !seen.insert(item.href.clone()) {
            continue;
        }
        let Some(layer) = layer_of(&item.path) else { continue };
        let html = rustdoc::read(cx.doc, &item.href)?;
        let doc = rustdoc::top_doc(&html).unwrap_or_default();
        let found = sections_of(&doc);
        let opted_in = found.iter().any(|(h, _)| h == "Models");
        let runs = html.contains("id=\"impl-Stage-for-") || family_traits.iter().any(|t| html.contains(&format!("id=\"impl-{t}-for-")));
        let is_model = opted_in || (layer != "primitives" && runs && !NOT_MODELS.iter().any(|(p, _)| *p == item.path));
        if !is_model {
            continue;
        }
        let mut sections = Vec::new();
        for want in SECTIONS {
            match found.iter().find(|(h, _)| h == want) {
                Some((_, body)) if !text_of(body).is_empty() => sections.push(body.clone()),
                _ => problems.push(format!("{}: no `# {want}` section", item.path)),
            }
        }
        let mut tests = Vec::new();
        if let Some(verified) = found.iter().find(|(h, _)| h == "Verified") {
            for span in code_spans(&verified.1) {
                // A whole test file: an integration-test target.
                if span.ends_with(".rs") && span.contains('/') && !span.contains(' ') {
                    match source(cx, &span) {
                        Some((file, src)) if src.contains("#[test]") => tests.push((span.clone(), file, span.rsplit('/').next().unwrap_or(&span).to_string(), 1)),
                        Some(_) => problems.push(format!("{}: Verified names {span}, which holds no #[test]", item.path)),
                        None => problems.push(format!("{}: Verified names {span}, which does not exist", item.path)),
                    }
                    continue;
                }
                let Some((file, name)) = span.split_once(".rs::") else { continue };
                let (written, file, name) = (span.clone(), format!("{file}.rs"), name.rsplit("::").next().unwrap_or(name).to_string());
                match find_test(cx, &file, &name) {
                    Ok((file, line)) => tests.push((written, file, name, line)),
                    Err(e) => problems.push(format!("{}: Verified {e}", item.path)),
                }
            }
            if tests.is_empty() && sections.len() == SECTIONS.len() {
                problems.push(format!("{}: Verified names no test (write it as `crates/…/file.rs::test_name`, or a test file `crates/…/tests/file.rs`)", item.path));
            }
        }
        let mut costs = Vec::new();
        if let Some(cost) = found.iter().find(|(h, _)| h == "Cost") {
            for span in code_spans(&cost.1) {
                let Some(stage) = span.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else { continue };
                match measured.get(stage) {
                    Some(ns) => costs.push((stage.to_string(), *ns)),
                    None => problems.push(format!("{}: Cost quotes datasheet stage \"{stage}\", which crates/stave-dsp/datasheet.json does not measure", item.path)),
                }
            }
        }
        if sections.len() == SECTIONS.len() {
            out.push(Model { item: item.clone(), layer, sections, tests, costs });
        }
    }
    if !problems.is_empty() {
        return Err(format!("the model catalogue (DSP.md naming rule 7):\n  {}", problems.join("\n  ")).into());
    }
    if out.is_empty() {
        return Err("the model catalogue found no models in stave_dsp".into());
    }
    Ok(out)
}

/// A section's text, its code spans kept.
fn prose(html: &str) -> String {
    let text = esc(&text_of(&html.replace("<code>", "\u{1}").replace("</code>", "\u{2}")));
    text.replace('\u{1}', "<code>").replace('\u{2}', "</code>")
}

pub fn catalogue(cx: &Ctx<'_>) -> Result<(Generated, usize)> {
    const PATH: &str = "reference/models/index.html";
    let (measured, machine) = datasheet(cx)?;
    let models = models(cx, &measured)?;
    let mut body = format!(
        "<p>Every model in <code>stave-dsp</code>: what it models, how it was derived, the test that holds it to its reference, what it costs and how it handles aliasing. Each row is read from the model's own rustdoc, whose sections are the same everywhere (DSP.md, naming rule 7); this page fails to build when a model lacks one, names a test that does not exist, or quotes a datasheet stage that is not measured. Costs are the committed datasheet's, measured on {}. A model is a stage or a device in a model layer, or any type whose docs open a <em>Models</em> section. Where a new model goes: <a href=\"{}\">Where does my model go?</a></p>",
        esc(&machine),
        rel(PATH, "howto/model-placement/index.html")
    );
    for (layer, claim) in LAYERS {
        let here: Vec<&Model> = models.iter().filter(|m| m.layer == layer).collect();
        if here.is_empty() {
            continue;
        }
        let _ = write!(body, "<h2 id=\"{layer}\"><code>{layer}</code></h2><p>{}.</p><table class=\"models\"><thead><tr><th>Model</th>", esc(&capitalise(claim)));
        for s in SECTIONS {
            let _ = write!(body, "<th>{s}</th>");
        }
        body.push_str("</tr></thead><tbody>");
        for m in here {
            let short = m.item.path.strip_prefix("stave_dsp::").unwrap_or(&m.item.path);
            let _ = write!(body, "<tr id=\"{}\"><td><a href=\"{}\"><code>{}</code></a></td>", short.replace("::", "-").to_lowercase(), rel(PATH, &format!("api/{}", m.item.href)), esc(short));
            for (k, html) in m.sections.iter().enumerate() {
                let mut cell = prose(html);
                if SECTIONS[k] == "Verified" {
                    // Each test as written becomes a link to its source.
                    for (written, file, name, line) in &m.tests {
                        let link = format!("<a href=\"{GITHUB}/blob/main/{file}#L{line}\"><code>{}</code></a>", esc(name));
                        cell = cell.replacen(&format!("<code>{}</code>", esc(written)), &link, 1);
                    }
                }
                if SECTIONS[k] == "Cost" {
                    // Each quoted stage gets its committed ns/sample beside it.
                    for (stage, ns) in &m.costs {
                        let quoted = format!("<code>{}</code>", esc(&format!("\"{stage}\"")));
                        let priced = format!("{quoted} (<a href=\"{}\">{ns:.1} ns/sample</a>)", rel(PATH, "reference/cost/index.html#datasheet"));
                        cell = cell.replacen(&quoted, &priced, 1);
                    }
                }
                let _ = write!(body, "<td>{cell}</td>");
            }
            body.push_str("</tr>");
        }
        body.push_str("</tbody></table>");
    }
    body.push_str("<h2 id=\"not-models\">Stages that are not models</h2><ul>");
    for (path, why) in NOT_MODELS {
        let _ = write!(body, "<li><code>{}</code>: {}.</li>", esc(path.strip_prefix("stave_dsp::").unwrap_or(path)), esc(why));
    }
    body.push_str("</ul>");
    let n = models.len();
    Ok((
        Generated {
            path: PATH,
            title: "Model catalogue",
            description: format!("All {n} models in stave-dsp by layer: what each models, its derivation, verification test, cost and aliasing, read from its rustdoc."),
            from: format!("Generated from the rustdoc of <code>stave-dsp</code> · checked by <a href=\"{GITHUB}/blob/main/tools/docsite/src/models.rs\"><code>tools/docsite/src/models.rs</code></a>"),
            body,
            wide: true,
        },
        n,
    ))
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
