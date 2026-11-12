//! Reading rustdoc's output: the docblock of a page, its links resolved
//! to the site, and the items of every crate (for search and for the
//! generated pages that point into the API).
//!
//! Rustdoc's HTML is not a stable interface, so every reader here fails
//! loudly on a shape it does not know rather than producing a page with
//! a hole in it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::html::{attr, text_of};
use crate::Result;

/// The `<div class="docblock">` that follows `from` in `html` (balanced
/// on `div`s), without the wrapper.
pub fn docblock_after(html: &str, from: usize) -> Option<String> {
    let open = "<div class=\"docblock\">";
    let start = html[from..].find(open)? + from + open.len();
    let mut depth = 1;
    let mut i = start;
    while depth > 0 {
        let next_open = html[i..].find("<div").map(|k| k + i);
        let next_close = html[i..].find("</div>").map(|k| k + i)?;
        match next_open {
            Some(o) if o < next_close => {
                depth += 1;
                i = o + 4;
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    return Some(html[start..next_close].to_string());
                }
                i = next_close + 6;
            }
        }
    }
    None
}

/// The top docblock of a rustdoc page (a module's or an item's own docs).
pub fn top_doc(html: &str) -> Option<String> {
    // An item without docs has no top-doc block (the first docblock on
    // its page would be a field's).
    let at = html.find("class=\"toggle top-doc\"")?;
    docblock_after(html, at)
}

/// The first paragraph of a docblock, as text.
pub fn summary(doc: &str) -> String {
    match (doc.find("<p>"), doc.find("</p>")) {
        (Some(a), Some(b)) if a < b => text_of(&doc[a + 3..b]),
        _ => String::new(),
    }
}

/// Normalise `a/b/../c/./d` to `a/c/d`.
pub fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Rewrite every `href` in `html`, a docblock from the rustdoc page at
/// `doc_page` (a path under the doc root), for a site page at
/// `site_page`: in-page anchors stay, external links stay, links into
/// the guide crate go to its site page (`guide_url`), everything else
/// into the API copy under `api/`.
pub fn relink(doc: &Path, html: &str, doc_page: &str, site_page: &str, guide_url: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let dir = doc_page.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        out.push_str(&rest[..i + 6]);
        rest = &rest[i + 6..];
        let end = rest.find('"').ok_or("unterminated href")?;
        let href = &rest[..end];
        rest = &rest[end..];
        if href.starts_with('#') || href.contains("://") || href.starts_with("mailto:") {
            out.push_str(href);
            continue;
        }
        if unresolved(href) {
            return Err(format!("{doc_page}: rustdoc left the link `{href}` unresolved").into());
        }
        let (path, frag) = match href.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (href, None),
        };
        let target = follow(doc, &normalise(&format!("{dir}/{path}")));
        let site_target = match guide_url(&target) {
            Some(url) => url,
            None => format!("api/{target}"),
        };
        out.push_str(&crate::html::rel(site_page, &site_target));
        if let Some(f) = frag {
            out.push('#');
            out.push_str(f);
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// An `href` that is an intra-doc link rustdoc did not resolve (it
/// writes the path through as text: `stave_test::PluginHarness`).
pub fn unresolved(href: &str) -> bool {
    href.contains("::") && !href.contains("://")
}

/// `target`, or where it redirects to: rustdoc leaves a redirect page at
/// an item's private path (`params/types/struct.FloatParam.html`) that
/// points at its public one.
pub fn follow(doc: &Path, target: &str) -> String {
    let Ok(html) = fs::read_to_string(doc.join(target)) else { return target.to_string() };
    let Some(i) = html.find("http-equiv=\"refresh\" content=\"0;URL=") else { return target.to_string() };
    let rest = &html[i + 36..];
    let to = &rest[..rest.find('"').unwrap_or(0)];
    let dir = target.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    normalise(&format!("{dir}/{to}"))
}

/// One documented item.
#[derive(Clone, Debug)]
pub struct Item {
    /// `struct`, `enum`, `trait`, `fn`, `macro`, `mod`, `method`, …
    pub kind: String,
    /// `stave_core::params::FloatParam`, `FloatParam::new`.
    pub path: String,
    /// Under the doc root: `stave_core/params/struct.FloatParam.html`.
    pub href: String,
    pub summary: String,
}

const KINDS: &[&str] = &[
    "mod", "struct", "enum", "trait", "fn", "macro", "constant", "type", "derive", "attr", "static", "union", "traitalias", "primitive",
];

/// Every item of `krate` reachable from its module tree, with its
/// summary, and the methods and associated items on each type's page.
pub fn items(doc: &Path, krate: &str) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = vec![format!("{krate}/index.html")];
    out.push(Item { kind: "crate".into(), path: krate.into(), href: format!("{krate}/index.html"), summary: page_summary(doc, &format!("{krate}/index.html"))? });
    while let Some(page) = queue.pop() {
        if !seen.insert(page.clone()) {
            continue;
        }
        let html = read(doc, &page)?;
        let dir = page.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        for (kind, href, title, summary) in item_table(&html) {
            if !KINDS.contains(&kind.as_str()) {
                continue;
            }
            let target = normalise(&format!("{dir}/{href}"));
            // Re-exports point into other crates; they are indexed there.
            if !target.starts_with(&format!("{krate}/")) {
                continue;
            }
            let path = title.split_once(' ').map(|(_, p)| p.to_string()).unwrap_or(title);
            if kind == "mod" {
                queue.push(target.clone());
            }
            if matches!(kind.as_str(), "struct" | "enum" | "trait" | "union") {
                let page = read(doc, &target)?;
                let short = path.rsplit("::").next().unwrap_or(&path).to_string();
                for (id, sum) in members(&page, kind == "trait") {
                    let (mk, name) = id.split_once('.').unwrap_or(("method", &id));
                    let mk = match mk {
                        "tymethod" => "method",
                        "associatedtype" => "type",
                        "associatedconstant" => "const",
                        "structfield" => "field",
                        k => k,
                    };
                    out.push(Item { kind: mk.to_string(), path: format!("{short}::{name}"), href: format!("{target}#{id}"), summary: sum });
                }
            }
            out.push(Item { kind, path, href: target, summary });
        }
    }
    Ok(out)
}

fn page_summary(doc: &Path, page: &str) -> Result<String> {
    Ok(top_doc(&read(doc, page)?).map(|d| summary(&d)).unwrap_or_default())
}

pub fn read(doc: &Path, page: &str) -> Result<String> {
    let p: PathBuf = doc.join(page);
    fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()).into())
}

/// `(kind, href, title, summary)` of every row of a module page's item
/// tables.
fn item_table(html: &str) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("<dt><a class=\"") {
        rest = &rest[i + 4..];
        let Some(close) = rest.find('>') else { break };
        let attrs = &rest[3..close];
        let (Some(kind), Some(href), Some(title)) = (attr(attrs, "class"), attr(attrs, "href"), attr(attrs, "title")) else { continue };
        let summary = match (rest.find("</dt>"), rest.find("<dt>")) {
            (Some(e), next) => {
                let after = &rest[e + 5..];
                if after.starts_with("<dd>") && next.is_none_or(|n| n > e) {
                    let end = after.find("</dd>").unwrap_or(after.len());
                    text_of(&after[4..end])
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        out.push((kind, href, title, summary));
    }
    out
}

/// `(id, summary)` of every method and associated item on a type page.
fn members(html: &str, is_trait: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("<section id=\"") {
        rest = &rest[i + 13..];
        let Some(end) = rest.find('"') else { break };
        let id = rest[..end].to_string();
        // A type's associated items come from the traits it implements
        // (`Error` from `TryFrom` …): noise; a trait's own are its API.
        let interesting: &[&str] = if is_trait {
            &["method.", "tymethod.", "associatedconstant.", "associatedtype."]
        } else {
            &["method.", "variant.", "structfield."]
        };
        if !interesting.iter().any(|p| id.starts_with(p)) {
            continue;
        }
        // Trait impls repeat their trait's methods (`method.fmt-1`).
        if id.rsplit_once('-').is_some_and(|(_, n)| n.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let summary = match (rest.find("<div class=\"docblock\">"), rest.find("<section id=\"")) {
            (Some(d), next) if next.is_none_or(|n| d < n) => docblock_after(rest, d).map(|b| summary(&b)).unwrap_or_default(),
            _ => String::new(),
        };
        out.push((id, summary));
    }
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// Name → doc path of every item listed on `krate/all.html`.
pub fn all_items(doc: &Path, krate: &str) -> Result<BTreeMap<String, String>> {
    let html = read(doc, &format!("{krate}/all.html"))?;
    let mut out = BTreeMap::new();
    let mut rest = html.as_str();
    while let Some(i) = rest.find("<li><a href=\"") {
        rest = &rest[i + 13..];
        let Some(end) = rest.find('"') else { break };
        let href = rest[..end].to_string();
        let Some(close) = rest.find("</a>") else { break };
        let name = text_of(&rest[end + 2..close]);
        out.insert(name, format!("{krate}/{href}"));
    }
    if out.is_empty() {
        return Err(format!("{krate}/all.html lists no items").into());
    }
    Ok(out)
}

/// The names of the public types implementing trait `trait_page` (from
/// its implementors list).
pub fn implementors(doc: &Path, trait_page: &str, trait_name: &str) -> Result<Vec<String>> {
    let html = read(doc, trait_page)?;
    let key = format!("id=\"impl-{trait_name}-for-");
    let mut out = Vec::new();
    let mut rest = html.as_str();
    while let Some(i) = rest.find(&key) {
        rest = &rest[i + key.len()..];
        let end = rest.find('"').unwrap_or(0);
        out.push(rest[..end].to_string());
    }
    if out.is_empty() {
        return Err(format!("{trait_page}: no implementors found").into());
    }
    Ok(out)
}
