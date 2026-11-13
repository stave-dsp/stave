//! stave-docsite: builds the documentation site from rustdoc's output and
//! the code itself. Nothing it writes is committed; CI builds it fresh
//! and deploys it (`.github/workflows/docs.yml`), so the site cannot
//! drift from the code it was built from.
//!
//! ```text
//! scripts/docsite.sh                  # rustdoc, then this, into target/site
//! stave-docsite --doc target/doc --out target/site
//! ```
//!
//! * the guide is the `stave-guide` crate's rustdoc pages (every link
//!   resolved, every example a doctest), laid out as the site's pages;
//! * the reference pages are computed: the crate graph and platform
//!   matrix from `cargo metadata` and the workflows, the units from
//!   `Unit::format`, the themes and the catalogue drawn from the real
//!   parts, the model catalogue from stave-dsp's rustdoc (failing on a
//!   model without its sections), the CLI pages by running each
//!   binary's `--help`, the plugin checks from `plugin_checks!`'s
//!   source, the changelog from git tags, the cost of each DSP stage
//!   from its committed measurements;
//! * the API is rustdoc's own output, restyled (`assets/rustdoc-*.html`
//!   are passed to rustdoc by the script);
//! * search covers the guide, the reference pages and every API item;
//! * every link on every page the site writes is checked before it
//!   exits.

mod check;
mod cost;
mod guide;
mod home;
mod html;
mod meta;
mod models;
mod reference;
mod rustdoc;
mod search;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use html::{layout, Group, Link, Page, Section};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

thread_local! {
    /// The commit the site is built from, for the footer.
    pub static COMMIT: RefCell<String> = const { RefCell::new(String::new()) };
}

const USAGE: &str = "\
usage: stave-docsite --doc DIR --out DIR

Builds the documentation site. DIR given to --doc is rustdoc's output
for the workspace (with the site's rustdoc assets: run scripts/docsite.sh,
which does both). The site is written to --out (emptied first) and every
link in it checked.

  --doc DIR     rustdoc's output (target/doc)
  --out DIR     where the site goes (target/site)
  -h, --help    this text";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let (Some(doc), Some(out)) = (value("--doc"), value("--out")) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match build(&doc, &out) {
        Ok(n) => {
            println!("stave-docsite: {n} pages in {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("stave-docsite: {e}");
            ExitCode::FAILURE
        }
    }
}

fn build(doc: &Path, out: &Path) -> Result<usize> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let commit = String::from_utf8(std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&repo).output()?.stdout)?;
    COMMIT.with(|c| *c.borrow_mut() = commit.trim().to_string());
    if !doc.join("stave_guide/index.html").exists() {
        return Err(format!("{}: no rustdoc output for stave-guide (run scripts/docsite.sh)", doc.display()).into());
    }
    if out.exists() {
        fs::remove_dir_all(out)?;
    }
    fs::create_dir_all(out)?;

    // The API: rustdoc's output as it is.
    copy_dir(doc, &out.join("api"))?;

    let ws = meta::workspace(&repo)?;
    let cx = reference::Ctx { repo: &repo, doc, out, ws: &ws };
    let mut pages: Vec<Page> = Vec::new();

    // The guide.
    let articles = guide::articles(&repo, doc)?;

    // The reference: generated.
    let (catalogue, parts) = reference::catalogue_page(&cx)?;
    let (checks, n_checks) = reference::plugin_checks(&cx)?;
    let (models, _) = models::catalogue(&cx)?;
    let generated = vec![
        reference::crates(&cx)?,
        reference::platforms(&cx)?,
        catalogue,
        models,
        reference::themes(&cx)?,
        reference::units(&cx)?,
        cost::page(&cx)?,
        checks,
        reference::cli(&cx)?,
        reference::changelog(&cx)?,
    ];
    let glossary = articles.iter().find(|a| a.module == ["glossary"]).ok_or("the guide has no glossary")?;
    let mut listed: Vec<(&str, &str, String)> = generated.iter().map(|g| (g.path, g.title, g.description.clone())).collect();
    listed.insert(0, (glossary.path.as_str(), "Glossary", glossary.description.clone()));
    let ref_sidebar = reference::sidebar(&cx, &listed);
    for g in &generated {
        pages.push(reference::page(g, ref_sidebar.clone()));
    }
    let index = reference::index(&cx, &listed)?;
    pages.push(reference::page(&index, ref_sidebar.clone()));

    for (i, a) in articles.iter().enumerate() {
        let sidebar = match a.section {
            Section::Reference => ref_sidebar.clone(),
            Section::Start => vec![Group {
                title: "Start here".into(),
                path: None,
                links: articles.iter().filter(|x| x.module.len() <= 1 && x.section == Section::Start).map(|x| Link { title: x.title.clone(), path: x.path.clone() }).collect(),
            }],
            s => guide::sidebar(&articles, s),
        };
        pages.push(guide::page(&articles, i, sidebar));
    }

    // Home.
    let facts = home::Facts { parts, checks: n_checks, themes: stave_face::Theme::all().len(), crates: ws.packages.len() };
    pages.push(home::page(&cx, &articles, &facts)?);

    // Write.
    for p in &pages {
        let path = out.join(&p.path);
        fs::create_dir_all(path.parent().ok_or("page without a directory")?)?;
        fs::write(&path, layout(p, &ws.version))?;
    }
    let assets = out.join("assets");
    fs::create_dir_all(&assets)?;
    fs::write(assets.join("site.css"), include_str!("../assets/site.css"))?;
    fs::write(assets.join("site.js"), include_str!("../assets/site.js"))?;
    fs::write(assets.join("icon.svg"), include_str!("../assets/icon.svg"))?;
    fs::write(out.join(".nojekyll"), "")?;
    fs::write(out.join("404.html"), not_found(&ws.version))?;

    // Search: the guide, the reference pages and every API item.
    search::write(&cx, &pages)?;

    // Every link on every page this wrote resolves.
    check::links(out, &pages)?;
    Ok(pages.len())
}

fn not_found(version: &str) -> String {
    let p = Page {
        path: "404.html".into(),
        title: "Not found".into(),
        description: "This page does not exist.".into(),
        section: Section::Start,
        body: "<p>Nothing lives at this address. Try <a href=\"/stave/\">the front page</a> or search (press <kbd>/</kbd>).</p>".into(),
        sidebar: Vec::new(),
        source: None,
        prev: None,
        next: None,
        kicker: String::new(),
        wide: true,
    };
    // A 404 is served at any depth: its links are absolute.
    layout(&p, version).replace("href=\"assets/", "href=\"/stave/assets/").replace("src=\"assets/", "src=\"/stave/assets/").replace("data-root=\"\"", "data-root=\"/stave/\"")
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        // Rustdoc's lock and build bookkeeping are not pages.
        if name == ".lock" || name == "crates.js.tmp" {
            continue;
        }
        let ty = e.file_type()?;
        if ty.is_dir() {
            copy_dir(&e.path(), &to.join(&name))?;
        } else {
            fs::copy(e.path(), to.join(&name))?;
        }
    }
    Ok(())
}
