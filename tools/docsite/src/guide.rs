//! The guide: the `stave-guide` crate's module pages, as rustdoc made
//! them (links resolved, examples tested), each laid out as a site page.
//!
//! The table of contents is the module tree of `stave-guide/src/lib.rs`
//! in source order; every module rustdoc wrote must be in it, and every
//! module in it must have a page.

use std::fs;
use std::path::Path;

use crate::html::{Group, Link, Page, Section};
use crate::rustdoc;
use crate::Result;

/// One article: its module path and where it lives.
#[derive(Clone, Debug)]
pub struct Article {
    /// `["concepts", "params"]`; empty for the crate root.
    pub module: Vec<String>,
    /// The Markdown source, from the repository root.
    pub source: String,
    /// Site path: `concepts/params/index.html`.
    pub path: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub section: Section,
}

/// The module tree of the guide crate's `lib.rs`: each module's path and
/// the Markdown file it includes, in source order.
fn modules(lib: &str) -> Result<Vec<(Vec<String>, String)>> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending_doc: Option<String> = None;
    for line in lib.lines() {
        let t = line.trim();
        if let Some(file) = include_of(t) {
            if t.starts_with("#![doc") {
                // The inner doc of the module just opened (or the crate).
                out.push((stack.clone(), file));
            } else {
                pending_doc = Some(file);
            }
        } else if let Some(rest) = t.strip_prefix("pub mod ") {
            let name = rest.trim_end_matches(['{', '}', ' ', ';']).trim().to_string();
            let leaf = rest.ends_with("{}");
            let mut path = stack.clone();
            path.push(name.clone());
            if let Some(file) = pending_doc.take() {
                out.push((path, file));
            }
            if !leaf {
                stack.push(name);
            }
        } else if t == "}" {
            stack.pop();
        }
    }
    if out.is_empty() {
        return Err("stave-guide/src/lib.rs declares no articles".into());
    }
    Ok(out)
}

fn include_of(line: &str) -> Option<String> {
    let i = line.find("include_str!(\"")?;
    let rest = &line[i + 14..];
    Some(rest[..rest.find('"')?].to_string())
}

/// The site path of the article at `module`.
pub fn site_path(module: &[String]) -> String {
    if module.is_empty() {
        return "guide/index.html".into();
    }
    let segs: Vec<String> = module.iter().map(|m| m.replace('_', "-")).collect();
    format!("{}/index.html", segs.join("/"))
}

fn section_of(module: &[String]) -> Section {
    match module.first().map(String::as_str) {
        Some("getting_started") => Section::Start,
        Some("tutorials") => Section::Tutorials,
        Some("concepts") => Section::Concepts,
        Some("howto") => Section::Howto,
        Some("glossary") => Section::Reference,
        _ => Section::Start,
    }
}

/// The site path of the guide page a rustdoc path (`stave_guide/x/y/index.html`) shows.
pub fn guide_url(doc_path: &str) -> Option<String> {
    let rest = doc_path.strip_prefix("stave_guide/")?;
    let rest = rest.strip_suffix("index.html")?;
    let module: Vec<String> = rest.split('/').filter(|s| !s.is_empty()).map(String::from).collect();
    Some(site_path(&module))
}

/// Every article, in reading order.
pub fn articles(repo: &Path, doc: &Path) -> Result<Vec<Article>> {
    let lib = fs::read_to_string(repo.join("crates/stave-guide/src/lib.rs"))?;
    let tree = modules(&lib)?;
    // Every module page rustdoc wrote is in the tree (and so on the site).
    let mut written = Vec::new();
    collect_modules(&doc.join("stave_guide"), "stave_guide", &mut written)?;
    for page in &written {
        let module: Vec<String> = page.trim_start_matches("stave_guide/").trim_end_matches("index.html").split('/').filter(|s| !s.is_empty()).map(String::from).collect();
        if !tree.iter().any(|(m, _)| *m == module) {
            return Err(format!("{page}: a guide module without an article in lib.rs's tree").into());
        }
    }
    let mut out = Vec::new();
    for (module, file) in tree {
        let doc_page = if module.is_empty() { "stave_guide/index.html".to_string() } else { format!("stave_guide/{}/index.html", module.join("/")) };
        let html = rustdoc::read(doc, &doc_page)?;
        let block = rustdoc::top_doc(&html).ok_or_else(|| format!("{doc_page}: no docblock"))?;
        let path = site_path(&module);
        let (title, body) = split_title(&block).ok_or_else(|| format!("{file}: an article starts with `# Title`"))?;
        let body = rustdoc::relink(doc, &body, &doc_page, &path, &guide_url)?;
        let body = demote(&body);
        let description = rustdoc::summary(&body);
        if description.is_empty() {
            return Err(format!("{file}: the paragraph after the title is the page's summary").into());
        }
        out.push(Article {
            section: section_of(&module),
            module,
            source: format!("crates/stave-guide/src/{file}"),
            path,
            title,
            description,
            body,
        });
    }
    Ok(out)
}

fn collect_modules(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    if dir.join("index.html").exists() {
        out.push(format!("{rel}/index.html"));
    }
    for e in fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            let name = e.file_name().to_string_lossy().to_string();
            collect_modules(&e.path(), &format!("{rel}/{name}"), out)?;
        }
    }
    Ok(())
}

/// The article's `# Title` (rustdoc's first `h2`) and the rest.
fn split_title(block: &str) -> Option<(String, String)> {
    let t = block.trim_start();
    if !t.starts_with("<h2") {
        return None;
    }
    let end = t.find("</h2>")?;
    let open_end = t.find('>')?;
    let title = crate::html::text_of(&t[open_end + 1..end]).trim_start_matches('§').trim().to_string();
    Some((title, t[end + 5..].to_string()))
}

/// Rustdoc shifts Markdown headings down one level (its page title is
/// the `h1`); the site's title is the article's own, so shift them back.
fn demote(body: &str) -> String {
    let mut s = body.to_string();
    for (from, to) in [("h3", "h2"), ("h4", "h3"), ("h5", "h4"), ("h6", "h5")] {
        s = s.replace(&format!("<{from} "), &format!("<{to} ")).replace(&format!("<{from}>"), &format!("<{to}>")).replace(&format!("</{from}>"), &format!("</{to}>"));
    }
    s
}

/// The sidebar of a guide section: each top-level part of the guide,
/// with its articles.
pub fn sidebar(articles: &[Article], section: Section) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for a in articles.iter().filter(|a| a.section == section && a.section != Section::Reference) {
        if a.module.len() == 1 {
            groups.push(Group { title: a.title.clone(), path: Some(a.path.clone()), links: Vec::new() });
        } else if let Some(g) = groups.last_mut() {
            g.links.push(Link { title: a.title.clone(), path: a.path.clone() });
        }
    }
    groups
}

/// Article `i` as a page, with its neighbours in reading order.
pub fn page(articles: &[Article], i: usize, sidebar: Vec<Group>) -> Page {
    let a = &articles[i];
    let reading: Vec<&Article> = articles.iter().filter(|a| !a.module.is_empty()).collect();
    let at = reading.iter().position(|r| r.path == a.path);
    let link = |r: &&Article| Link { title: r.title.clone(), path: r.path.clone() };
    let kicker = match a.section {
        Section::Tutorials if a.module.len() > 1 => "Tutorial",
        Section::Concepts if a.module.len() > 1 => "Concept",
        Section::Howto if a.module.len() > 1 => "How-to",
        Section::Reference => "Reference",
        _ => "",
    };
    Page {
        path: a.path.clone(),
        title: a.title.clone(),
        description: a.description.clone(),
        section: a.section,
        body: a.body.clone(),
        sidebar,
        source: Some(a.source.clone()),
        prev: at.and_then(|k| k.checked_sub(1)).map(|k| link(&reading[k])),
        next: at.and_then(|k| reading.get(k + 1)).map(link),
        kicker: kicker.into(),
        wide: false,
    }
}
