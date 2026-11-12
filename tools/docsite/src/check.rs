//! Every `href` and `src` on every page the site writes resolves: the
//! file exists, and a `#fragment` names an id on it.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::html::{attr, Page};
use crate::rustdoc::normalise;
use crate::Result;

pub fn links(out: &Path, pages: &[Page]) -> Result<()> {
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut broken = Vec::new();
    let mut checked = 0;
    for p in pages {
        let html = fs::read_to_string(out.join(&p.path))?;
        let dir = p.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for url in urls(&html) {
            if url.contains("://") || url.starts_with("mailto:") || url.starts_with("data:") || url.starts_with('/') {
                continue;
            }
            checked += 1;
            let (path, frag) = match url.split_once('#') {
                Some((a, b)) => (a, Some(b)),
                None => (url.as_str(), None),
            };
            let mut target = if path.is_empty() { p.path.clone() } else { normalise(&format!("{dir}/{path}")) };
            if target.is_empty() || path.ends_with('/') || out.join(&target).is_dir() {
                target = if target.is_empty() { "index.html".into() } else { format!("{target}/index.html") };
            }
            let file = out.join(&target);
            if !file.is_file() {
                broken.push(format!("{}: {url} (no {target})", p.path));
                continue;
            }
            if let Some(f) = frag.filter(|f| !f.is_empty()) {
                if !target.ends_with(".html") {
                    continue;
                }
                let text = cache.entry(target.clone()).or_insert_with(|| fs::read_to_string(&file).unwrap_or_default());
                if !text.contains(&format!("id=\"{f}\"")) {
                    broken.push(format!("{}: {url} (no #{f} on {target})", p.path));
                }
            }
        }
    }
    // Rustdoc's own pages: no intra-doc link left as text.
    let mut stack = vec![out.join("api")];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir)? {
            let path = e?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "html") {
                let html = fs::read_to_string(&path)?;
                for url in urls(&html) {
                    if crate::rustdoc::unresolved(&url) {
                        broken.push(format!("{}: rustdoc left `{url}` unresolved", path.strip_prefix(out).unwrap_or(&path).display()));
                    }
                }
            }
        }
    }
    if !broken.is_empty() {
        return Err(format!("{} broken links:\n  {}", broken.len(), broken.join("\n  ")).into());
    }
    if checked < 100 {
        return Err(format!("only {checked} links checked; the checker is not reading the pages").into());
    }
    Ok(())
}

fn urls(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if tag.starts_with('!') || tag.starts_with('/') {
            continue;
        }
        // Script bodies and code are not markup.
        for a in ["href", "src"] {
            if let Some(v) = attr(tag, a) {
                out.push(v.replace("&amp;", "&"));
            }
        }
    }
    out
}
