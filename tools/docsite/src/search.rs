//! The search index: every guide article and its sections, every
//! reference page, every API item (with methods, fields and variants),
//! written as one script the site's search reads.

use std::fmt::Write as _;
use std::fs;

use crate::html::{outline, text_of, Page, Section};
use crate::reference::Ctx;
use crate::rustdoc;
use crate::Result;

/// One entry: `[title, path, kind, context, text]`.
fn entry(out: &mut String, title: &str, path: &str, kind: &str, context: &str, text: &str) {
    let text: String = text.chars().take(160).collect();
    let _ = writeln!(out, "[{},{},{},{},{}],", json(title), json(path), json(kind), json(context), json(&text));
}

fn json(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

pub fn write(cx: &Ctx<'_>, pages: &[Page]) -> Result<()> {
    let mut js = String::from("window.STAVE_INDEX=[\n");
    let mut n = 0;
    for p in pages {
        if p.section == Section::Home {
            continue;
        }
        let kind = match p.section {
            Section::Reference if p.source.is_none() => "reference",
            Section::Tutorials => "tutorial",
            Section::Concepts => "concept",
            Section::Howto => "how-to",
            _ => "guide",
        };
        entry(&mut js, &p.title, &p.path, kind, "", &p.description);
        n += 1;
        // Each section of the page, with the words under its heading.
        for (_, id, heading) in outline(&p.body) {
            let at = p.body.find(&format!("id=\"{id}\"")).unwrap_or(0);
            let after = &p.body[at..];
            let start = after.find('>').map(|i| i + 1).unwrap_or(0);
            let end = after[start..].find("<h2").map(|i| i + start).unwrap_or(after.len().min(start + 2000));
            let words = text_of(&after[start..end.min(after.len())]);
            entry(&mut js, &heading, &format!("{}#{id}", p.path), kind, &p.title, &words);
            n += 1;
        }
    }
    for pkg in &cx.ws.packages {
        let Some(lib) = &pkg.lib else { continue };
        if pkg.name == "stave-guide" || !cx.doc.join(lib).join("index.html").exists() {
            continue;
        }
        for item in rustdoc::items(cx.doc, lib)? {
            let context = if item.kind == "crate" { String::new() } else { lib.clone() };
            entry(&mut js, &item.path, &format!("api/{}", item.href), &item.kind, &context, &item.summary);
            n += 1;
        }
    }
    js.push_str("];\n");
    if n < 100 {
        return Err(format!("the search index has only {n} entries").into());
    }
    fs::write(cx.out.join("search-index.js"), js)?;
    Ok(())
}
