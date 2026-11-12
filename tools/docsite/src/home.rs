//! The front page. Its code is the `stave` crate's own example (a
//! doctest, so it builds), and every number on it is counted from the
//! code.

use std::fmt::Write as _;

use crate::guide::Article;
use crate::html::{esc, Page, Section};
use crate::meta;
use crate::reference::Ctx;
use crate::rustdoc;
use crate::Result;

/// What the front page counts.
pub struct Facts {
    pub parts: usize,
    pub checks: usize,
    pub themes: usize,
    pub crates: usize,
}

pub fn page(cx: &Ctx<'_>, articles: &[Article], facts: &Facts) -> Result<Page> {
    const PATH: &str = "index.html";
    // The umbrella crate's first example: a whole plugin, tested.
    let stave = rustdoc::read(cx.doc, "stave/index.html")?;
    let doc = rustdoc::top_doc(&stave).ok_or("stave has no crate docs")?;
    let pre = doc.find("<pre class=\"rust rust-example-").ok_or("stave's crate docs lost their example")?;
    let code_at = doc[pre..].find("<code>").ok_or("an example without code")? + pre + "<code>".len();
    let code_end = doc[code_at..].find("</code></pre>").ok_or("unterminated example")? + code_at;
    let code = &doc[code_at..code_end];
    // The platforms a plugin's face is shown on: those where stave-vst3
    // takes a platform crate.
    let vst3 = cx.ws.packages.iter().find(|p| p.name == "stave-vst3").ok_or("no stave-vst3")?;
    let mut platforms = Vec::new();
    for (name, facts) in meta::PLATFORMS {
        let shown = vst3.deps.iter().any(|(d, c)| d.starts_with("stave-ui-") && c.as_deref().is_some_and(|c| meta::cfg_holds(c, facts).unwrap_or(false)));
        if shown {
            platforms.push(*name);
        }
    }
    let lookup = |m: &[&str]| -> Result<&Article> { articles.iter().find(|a| a.module == m).ok_or_else(|| format!("the guide has no {}", m.join("::")).into()) };
    let start = lookup(&["getting_started"])?;
    let first = lookup(&["tutorials", "first_plugin"])?;
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<section class="hero"><div class="hero-copy"><p class="kicker">Rust audio plugin framework · VST3 + CLAP</p><h1>Build audio plugins you can prove.</h1><p class="sub">Stave is the whole Rust stack for audio plugins — the host ABIs, DSP, a design system for faces, testing and measurement. Every plugin is realtime-safe, validated and measurable from its first commit.</p><div class="ctas"><a class="btn" href="{start}">Get started</a><a class="btn ghost" href="{first}">Build your first plugin</a><a class="btn ghost" href="api/stave/index.html">API</a></div></div><div class="window" aria-label="A complete plugin"><div class="window-bar"><span></span><span></span><span></span><em>a whole plugin · the <code>stave</code> crate's example, built and tested</em></div><pre class="rust"><code>{code}</code></pre></div></section>"#,
        start = start.path.trim_end_matches("index.html"),
        first = first.path.trim_end_matches("index.html"),
    );
    let _ = write!(
        h,
        r#"<ul class="strip" aria-label="Counted from the code"><li><strong>VST3 + CLAP</strong>from one cdylib</li><li><strong>{n_plats} platforms</strong>{plats}</li><li><strong>{checks}</strong>checks per plugin, one line</li><li><strong>{parts}</strong>part types</li><li><strong>{themes}</strong>themes</li><li><strong>{crates}</strong>crates, no C/C++ SDKs</li></ul>"#,
        plats = esc(&platforms.join(" · ")),
        n_plats = platforms.len(),
        checks = facts.checks,
        parts = facts.parts,
        themes = facts.themes,
        crates = facts.crates,
    );
    h.push_str(r#"<section class="block"><div class="block-head"><h2>Find your way</h2><p>Learn by building, understand the model, look things up.</p></div><div class="routes">"#);
    for m in [&["getting_started"][..], &["tutorials"], &["concepts"], &["howto"]] {
        let a = lookup(m)?;
        let _ = write!(h, "<a href=\"{}\"><strong>{} →</strong><span>{}</span></a>", a.path.trim_end_matches("index.html"), esc(&a.title), esc(&a.description));
    }
    h.push_str(r#"<a href="reference/"><strong>Reference →</strong><span>Every crate's API, the part catalogue, themes, units, CLI tools, the crate graph — generated from the code.</span></a>"#);
    let g = lookup(&["glossary"])?;
    let _ = write!(h, "<a href=\"{}\"><strong>Glossary →</strong><span>{}</span></a>", g.path.trim_end_matches("index.html"), esc(&g.description));
    h.push_str("</div></section>");
    h.push_str(r#"<section class="block"><div class="block-head"><h2>Why every page here is true</h2><p>This site is built from the code on every push to main and never committed, so there is nothing to fall out of date.</p></div><div class="pillars"><article><h3>Articles are doctests</h3><p>The guide is a crate. Every article is the documentation of a module: rustdoc resolves each of its links against the real API and runs each of its Rust examples as a test.</p></article><article><h3>The reference is computed</h3><p>Pictures of every part, drawn by the face runtime. Units formatted by <code>Unit::format</code>. CLI help from running each binary. The crate graph from <code>cargo metadata</code>. The changelog from git tags.</p></article><article><h3>The API is rustdoc</h3><p>Every public item of every crate, with source links, built with warnings as errors and every intra-doc link checked.</p></article><article><h3>Links are checked</h3><p>The generator refuses to publish a page with a link that goes nowhere, a part without a picture, or a module without an article.</p></article></div></section>"#);
    Ok(Page {
        path: PATH.into(),
        title: "Stave".into(),
        description: "Stave is a Rust framework for VST3 and CLAP audio plugins, with measurement, validation and realtime-safety checks built into every layer.".into(),
        section: Section::Home,
        body: h,
        sidebar: Vec::new(),
        source: None,
        prev: None,
        next: None,
        kicker: String::new(),
        wide: true,
    })
}
