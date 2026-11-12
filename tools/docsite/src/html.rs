//! The site's one page layout, and the small HTML vocabulary every
//! generator writes with.

use std::fmt::Write as _;

/// `s` with `& < > "` escaped, for text and attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `html` without its tags, entities decoded, whitespace collapsed: what
/// a reader sees, for descriptions and the search index.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    let words: Vec<&str> = out.split_whitespace().collect();
    // Punctuation the tag spaces split off its word.
    words
        .join(" ")
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" :", ":")
        .replace(" ;", ";")
        .replace(" )", ")")
        .replace("( ", "(")
        .replace(" ’", "’")
}

/// The site path (`concepts/lifecycle/index.html`) of a page, relative
/// to the site root, and how a page links to another.
pub fn rel(from: &str, to: &str) -> String {
    let depth = from.matches('/').count();
    let mut s = "../".repeat(depth);
    let to = to.strip_suffix("index.html").unwrap_or(to);
    s.push_str(to);
    if s.is_empty() {
        "./".to_string()
    } else {
        s
    }
}

/// A link in a sidebar: its title and the page it opens.
#[derive(Clone, Debug)]
pub struct Link {
    pub title: String,
    pub path: String,
}

/// A titled group of links in a sidebar.
#[derive(Clone, Debug)]
pub struct Group {
    pub title: String,
    pub path: Option<String>,
    pub links: Vec<Link>,
}

/// The top-level sections, in the masthead's order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Section {
    Home,
    Start,
    Tutorials,
    Concepts,
    Howto,
    Reference,
}

impl Section {
    fn nav(self) -> &'static [(Section, &'static str, &'static str)] {
        &[
            (Section::Start, "Get started", "getting-started/index.html"),
            (Section::Tutorials, "Tutorials", "tutorials/index.html"),
            (Section::Concepts, "Concepts", "concepts/index.html"),
            (Section::Howto, "How-to", "howto/index.html"),
            (Section::Reference, "Reference", "reference/index.html"),
        ]
    }
}

/// One page of the site, before layout.
pub struct Page {
    /// Site path, e.g. `concepts/params/index.html`.
    pub path: String,
    pub title: String,
    /// One sentence for `<meta name=description>` and the search index.
    pub description: String,
    pub section: Section,
    /// The article's HTML (headings with ids; links already relative to
    /// this page).
    pub body: String,
    /// The sidebar: the section's groups.
    pub sidebar: Vec<Group>,
    /// Where the page's words come from, for "Edit this page".
    pub source: Option<String>,
    /// Previous and next pages in reading order.
    pub prev: Option<Link>,
    pub next: Option<Link>,
    /// A kicker over the title ("Concept", "Generated from …").
    pub kicker: String,
    /// Wide pages (the catalogue, the crate graph) drop the outline.
    pub wide: bool,
}

/// `(level, id, text)` of each `h2`/`h3` in `body`, for the outline.
pub fn outline(body: &str) -> Vec<(u8, String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<h") {
        rest = &rest[i + 2..];
        let level = match rest.as_bytes().first() {
            Some(b'2') => 2,
            Some(b'3') => 3,
            _ => continue,
        };
        let Some(close) = rest.find('>') else { break };
        let attrs = &rest[1..close];
        let Some(id) = attr(attrs, "id") else { continue };
        let end = rest.find(&format!("</h{level}>")).unwrap_or(rest.len());
        let inner = &rest[close + 1..end];
        let text = text_of(inner).trim_start_matches('§').trim().to_string();
        out.push((level, id, text));
    }
    out
}

/// The value of attribute `name` in a tag's attribute text.
pub fn attr(attrs: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = attrs[from..].find(&key) {
        let at = from + i;
        // Whole attribute names only (`id=` is not `data-id=`).
        if at == 0 || attrs.as_bytes()[at - 1].is_ascii_whitespace() {
            let start = at + key.len();
            let end = attrs[start..].find('"')? + start;
            return Some(attrs[start..end].to_string());
        }
        from = at + key.len();
    }
    None
}

const GITHUB: &str = "https://github.com/stave-dsp/stave";

/// The page, laid out: masthead, sidebar, article, outline, footer.
pub fn layout(p: &Page, version: &str) -> String {
    let r = rel(&p.path, "");
    let r = if r == "./" { String::new() } else { r };
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{desc}">
<link rel="icon" href="{r}assets/icon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,400;0,500;0,600;1,400&amp;family=IBM+Plex+Sans:ital,wght@0,400;0,500;0,600;1,400&amp;display=swap">
<link rel="stylesheet" href="{r}assets/site.css">
<script>try{{var t=localStorage.getItem("stave-theme");if(t)document.documentElement.dataset.theme=t}}catch(e){{}}</script>
<script defer src="{r}assets/site.js"></script>
</head>
<body data-root="{r}">
"#,
        title = if p.section == Section::Home { "Stave — the Rust audio plugin framework".to_string() } else { format!("{} · Stave", esc(&p.title)) },
        desc = esc(&p.description),
    );
    h.push_str(&masthead(&r, p.section, version));
    let wide = if p.wide { " wide" } else { "" };
    if p.section == Section::Home {
        let _ = write!(h, "<main id=\"main\" class=\"home\">{}</main>", p.body);
    } else {
        let _ = write!(h, "<div class=\"layout{wide}\">");
        // Sidebar.
        h.push_str("<nav class=\"sidebar\" aria-label=\"Section\"><button class=\"sidebar-toggle\" aria-expanded=\"false\">Contents</button><div class=\"sidebar-inner\">");
        for g in &p.sidebar {
            match &g.path {
                Some(path) => {
                    let cur = if *path == p.path { " aria-current=\"page\"" } else { "" };
                    let _ = write!(h, "<h4><a href=\"{}\"{cur}>{}</a></h4>", rel(&p.path, path), esc(&g.title));
                }
                None => {
                    let _ = write!(h, "<h4>{}</h4>", esc(&g.title));
                }
            }
            h.push_str("<ul>");
            for l in &g.links {
                let cur = if l.path == p.path { " aria-current=\"page\"" } else { "" };
                let _ = write!(h, "<li><a href=\"{}\"{cur}>{}</a></li>", rel(&p.path, &l.path), esc(&l.title));
            }
            h.push_str("</ul>");
        }
        h.push_str("</div></nav>");
        // Article.
        h.push_str("<main id=\"main\" class=\"content\"><article class=\"doc\">");
        if !p.kicker.is_empty() {
            let _ = write!(h, "<p class=\"kicker\">{}</p>", p.kicker);
        }
        let _ = write!(h, "<h1>{}</h1>", esc(&p.title));
        h.push_str(&p.body);
        h.push_str("</article>");
        if p.prev.is_some() || p.next.is_some() {
            h.push_str("<nav class=\"pager\" aria-label=\"Pages\">");
            if let Some(l) = &p.prev {
                let _ = write!(h, "<a class=\"prev\" href=\"{}\"><span>Previous</span>{}</a>", rel(&p.path, &l.path), esc(&l.title));
            }
            if let Some(l) = &p.next {
                let _ = write!(h, "<a class=\"next\" href=\"{}\"><span>Next</span>{}</a>", rel(&p.path, &l.path), esc(&l.title));
            }
            h.push_str("</nav>");
        }
        if let Some(src) = &p.source {
            let _ = write!(h, "<p class=\"edit\"><a href=\"{GITHUB}/blob/main/{src}\">Edit this page</a> · <code>{}</code></p>", esc(src));
        }
        h.push_str("</main>");
        // Outline.
        let toc = outline(&p.body);
        if !p.wide && toc.len() > 1 {
            h.push_str("<aside class=\"outline\" aria-label=\"On this page\"><h4>On this page</h4><ul>");
            for (level, id, text) in toc {
                let _ = write!(h, "<li class=\"l{level}\"><a href=\"#{id}\">{}</a></li>", esc(&text));
            }
            h.push_str("</ul></aside>");
        }
        h.push_str("</div>");
    }
    let _ = write!(
        h,
        r#"<footer class="colophon"><span>stave {version} · MIT licensed · every page here is built from the code at <a href="{GITHUB}/commit/{commit}"><code>{short}</code></a></span><span><a href="{GITHUB}">GitHub</a> · <a href="https://stave-dsp.github.io/">stave-dsp.github.io</a></span></footer>
</body>
</html>
"#,
        commit = crate::COMMIT.with(|c| c.borrow().clone()),
        short = crate::COMMIT.with(|c| c.borrow().chars().take(9).collect::<String>()),
    );
    h
}

fn masthead(r: &str, current: Section, version: &str) -> String {
    let mut h = String::new();
    let _ = write!(
        h,
        r##"<a class="skip" href="#main">Skip to content</a><header class="masthead"><a class="wordmark" href="{home}">stave</a><span class="ver">{version}</span><nav class="primary" aria-label="Site">"##,
        home = if r.is_empty() { "./" } else { r },
    );
    for &(s, title, path) in Section::Home.nav() {
        let cur = if s == current { " aria-current=\"true\"" } else { "" };
        let _ = write!(h, "<a href=\"{r}{}\"{cur}>{title}</a>", path.trim_end_matches("index.html"));
    }
    let _ = write!(
        h,
        r#"<a href="{r}api/stave/index.html">API</a></nav><div class="tools"><button class="search-open" aria-label="Search" title="Search (/)"><span>Search</span><kbd>/</kbd></button><button class="theme-toggle" aria-label="Toggle dark mode" title="Toggle dark mode"></button><a class="gh" href="{GITHUB}" aria-label="GitHub">GitHub</a></div></header>"#
    );
    h
}
