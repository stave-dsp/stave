//! The generated reference: every page here is computed from the code at
//! build time (types, the real parts drawn, the binaries run, cargo's
//! own metadata, git's tags), never written by hand.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use stave_core::params::Unit;
use stave_face::photo::Rgb;
use stave_face::{CapStyle, Coat, Coverings, Finish, Hardware, Palette, Theme, Typography};
use stave_storybook::catalogue::{self, Text};
use stave_storybook::controller::StoryParams;
use stave_storybook::stories;
use stave_ui::style::Color;

use crate::html::{esc, rel, text_of, Group, Link, Page, Section};
use crate::meta::{self, Workspace};
use crate::rustdoc;
use crate::Result;

pub const GITHUB: &str = "https://github.com/stave-dsp/stave";

/// What every generator reads.
pub struct Ctx<'a> {
    pub repo: &'a Path,
    pub doc: &'a Path,
    pub out: &'a Path,
    pub ws: &'a Workspace,
}

/// One generated page: its path, title, summary and body.
pub struct Generated {
    pub path: &'static str,
    pub title: &'static str,
    pub description: String,
    /// The source the page is computed from, shown as its kicker.
    pub from: String,
    pub body: String,
    pub wide: bool,
}

pub fn src_link(path: &str) -> String {
    format!("<a href=\"{GITHUB}/blob/main/{path}\"><code>{}</code></a>", esc(path))
}

/// A link from site page `from` to the API page of `doc_path`.
fn api(from: &str, doc_path: &str, text: &str) -> String {
    format!("<a href=\"{}\"><code>{}</code></a>", rel(from, &format!("api/{doc_path}")), esc(text))
}

/// The doc path of a public item `name` (its last path segment) in `krate`.
fn find_item(all: &BTreeMap<String, String>, name: &str) -> Result<String> {
    let hits: Vec<&String> = all.iter().filter(|(k, _)| k.rsplit("::").next() == Some(name)).map(|(_, v)| v).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("no public item named {name}").into()),
        many => {
            // Prefer the shortest path (the re-export at the root).
            Ok(many.iter().min_by_key(|v| v.len()).map(|v| (*v).clone()).unwrap_or_default())
        }
    }
}

// ------------------------------------------------------------ crates

pub fn crates(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/crates/index.html";
    let pkgs = &cx.ws.packages;
    let names: Vec<&str> = pkgs.iter().map(|p| p.name.as_str()).collect();
    let deps_of = |n: &str| -> Vec<(String, Option<String>)> { pkgs.iter().find(|p| p.name == n).map(|p| p.deps.clone()).unwrap_or_default() };
    // Layers: a crate sits one column right of its deepest dependency.
    let mut layer: BTreeMap<String, usize> = BTreeMap::new();
    type Deps<'a> = dyn Fn(&str) -> Vec<(String, Option<String>)> + 'a;
    fn depth(n: &str, deps: &Deps<'_>, memo: &mut BTreeMap<String, usize>, guard: usize) -> usize {
        if let Some(d) = memo.get(n) {
            return *d;
        }
        assert!(guard < 64, "a dependency cycle through {n}");
        let d = deps(n).iter().map(|(d, _)| depth(d, deps, memo, guard + 1) + 1).max().unwrap_or(0);
        memo.insert(n.to_string(), d);
        d
    }
    for n in &names {
        depth(n, &deps_of, &mut layer, 0);
    }
    // Transitive reduction: an edge a → c is drawn only when no other
    // path a → … → c exists.
    let reach = |from: &str, to: &str, skip: &str| -> bool {
        let mut stack: Vec<String> = deps_of(from).into_iter().map(|(d, _)| d).filter(|d| d != skip).collect();
        let mut seen = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if seen.insert(n.clone()) {
                stack.extend(deps_of(&n).into_iter().map(|(d, _)| d));
            }
        }
        false
    };
    let mut edges = Vec::new();
    for p in pkgs {
        for (d, cfg) in &p.deps {
            if !reach(&p.name, d, d) {
                edges.push((p.name.clone(), d.clone(), cfg.clone()));
            }
        }
    }
    // Rows within a column: by the mean row of what each depends on.
    let cols = layer.values().max().copied().unwrap_or(0) + 1;
    let mut rows: BTreeMap<String, f64> = BTreeMap::new();
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); cols];
    for c in 0..cols {
        let mut col: Vec<(f64, String)> = layer
            .iter()
            .filter(|(_, l)| **l == c)
            .map(|(n, _)| {
                let ds: Vec<f64> = deps_of(n).iter().filter_map(|(d, _)| rows.get(d).copied()).collect();
                let key = if ds.is_empty() { f64::MAX } else { ds.iter().sum::<f64>() / ds.len() as f64 };
                (key, n.clone())
            })
            .collect();
        col.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (i, (_, n)) in col.iter().enumerate() {
            rows.insert(n.clone(), i as f64);
        }
        columns[c] = col.into_iter().map(|(_, n)| n).collect();
    }
    let (bw, bh, cw, rh, pad) = (176.0, 30.0, 232.0, 44.0, 16.0);
    let max_rows = columns.iter().map(Vec::len).max().unwrap_or(1) as f64;
    let width = pad * 2.0 + (cols as f64 - 1.0) * cw + bw;
    let height = pad * 2.0 + (max_rows - 1.0) * rh + bh;
    let pos = |n: &str| -> (f64, f64) {
        let c = layer[n] as f64;
        let col_len = columns[layer[n]].len() as f64;
        let off = (max_rows - col_len) * rh / 2.0;
        (pad + c * cw, pad + off + rows[n] * rh)
    };
    let mut svg = format!(r#"<svg class="graph" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img" aria-label="The workspace's crates, each to the right of what it depends on">"#);
    for (from, to, cfg) in &edges {
        let (x1, y1) = pos(from);
        let (x2, y2) = pos(to);
        let (sx, sy, ex, ey) = (x1, y1 + bh / 2.0, x2 + bw, y2 + bh / 2.0);
        let mid = (sx + ex) / 2.0;
        let class = if cfg.is_some() { "edge gated" } else { "edge" };
        let _ = write!(
            svg,
            r#"<path class="{class}" data-from="{from}" data-to="{to}" d="M{sx},{sy} C{mid},{sy} {mid},{ey} {ex},{ey}"><title>{from} → {to}{}</title></path>"#,
            cfg.as_ref().map(|c| format!(" ({})", esc(c))).unwrap_or_default()
        );
    }
    for p in pkgs {
        let (x, y) = pos(&p.name);
        let href = match &p.lib {
            Some(lib) if cx.doc.join(lib).join("index.html").exists() => rel(PATH, &format!("api/{lib}/index.html")),
            _ => format!("#{}", p.name),
        };
        let _ = write!(
            svg,
            r#"<a href="{href}" class="node" data-name="{n}"><rect x="{x}" y="{y}" width="{bw}" height="{bh}" rx="3"/><text x="{tx}" y="{ty}">{n}</text><title>{d}</title></a>"#,
            n = esc(&p.name),
            tx = x + 10.0,
            ty = y + bh / 2.0 + 4.5,
            d = esc(&p.description)
        );
    }
    svg.push_str("</svg>");

    let mut body = String::new();
    let _ = write!(
        body,
        "<p>The {} packages of the workspace, from <code>cargo metadata</code>. Each crate sits to the right of everything it depends on; an edge is drawn only where no longer path already connects the two, and a dashed edge is a dependency on one platform only. Hover an edge for its <code>cfg</code>; click a crate for its API.</p><div class=\"graph-wrap\">{svg}</div>",
        pkgs.len()
    );
    body.push_str("<h2 id=\"packages\">Packages</h2><table class=\"crates\"><thead><tr><th>Package</th><th>What it is</th><th>Depends on</th><th>Used by</th></tr></thead><tbody>");
    for p in pkgs {
        let used: Vec<&str> = pkgs.iter().filter(|q| q.deps.iter().any(|(d, _)| *d == p.name)).map(|q| q.name.as_str()).collect();
        let deps: Vec<String> = p
            .deps
            .iter()
            .map(|(d, cfg)| match cfg {
                Some(c) => format!("<a href=\"#{d}\">{d}</a> <span class=\"cfg\" title=\"{}\">{}</span>", esc(c), esc(&platforms_of(c))),
                None => format!("<a href=\"#{d}\">{d}</a>"),
            })
            .collect();
        let name = match &p.lib {
            Some(lib) if cx.doc.join(lib).join("index.html").exists() => api(PATH, &format!("{lib}/index.html"), &p.name),
            _ => format!("<code>{}</code>", esc(&p.name)),
        };
        let kinds: Vec<&str> = p.kinds.iter().map(String::as_str).collect();
        let _ = write!(
            body,
            "<tr id=\"{}\"><td>{name}<div class=\"meta\">{} · {}{}</div></td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&p.name),
            esc(&p.dir),
            kinds.join(", "),
            if p.publish { "" } else { " · unpublished" },
            esc(&p.description),
            deps.join(", "),
            used.iter().map(|u| format!("<a href=\"#{u}\">{u}</a>")).collect::<Vec<_>>().join(", ")
        );
    }
    body.push_str("</tbody></table>");
    Ok(Generated {
        path: PATH,
        title: "Crate graph",
        description: format!("The {} packages of the Stave workspace and how they depend on each other, from cargo metadata.", pkgs.len()),
        from: format!("Generated from <code>cargo metadata</code> · {}", src_link("Cargo.toml")),
        body,
        wide: true,
    })
}

/// The platforms a `cfg(...)` holds on, as words.
fn platforms_of(cfg: &str) -> String {
    let on: Vec<&str> = meta::PLATFORMS.iter().filter(|(_, f)| meta::cfg_holds(cfg, f).unwrap_or(false)).map(|(n, _)| *n).collect();
    if on.is_empty() {
        "no platform".into()
    } else {
        on.join(", ")
    }
}

// --------------------------------------------------------- platforms

pub fn platforms(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/platforms/index.html";
    let pkgs = &cx.ws.packages;
    let mut body = String::new();
    body.push_str("<p>Which crate does each platform's part of the work, evaluated from the <code>cfg</code> of every platform-gated dependency in the workspace, and which CI jobs run on each platform, read from the workflows.</p>");
    // Every package with platform-gated workspace dependencies.
    body.push_str("<h2 id=\"by-platform\">Platform crates, by package</h2><table class=\"matrix\"><thead><tr><th>Package</th>");
    for (name, _) in meta::PLATFORMS {
        let _ = write!(body, "<th>{name}</th>");
    }
    body.push_str("</tr></thead><tbody>");
    let mut rows = 0;
    for p in pkgs {
        let gated: Vec<&(String, Option<String>)> = p.deps.iter().filter(|(_, c)| c.as_ref().is_some_and(|c| c.contains("unix") || c.contains("windows") || c.contains("target_os"))).collect();
        if gated.is_empty() {
            continue;
        }
        rows += 1;
        let _ = write!(body, "<tr><td><code>{}</code><div class=\"meta\">{}</div></td>", esc(&p.name), esc(&p.description));
        for (_, facts) in meta::PLATFORMS {
            let mut on = Vec::new();
            for (d, c) in &gated {
                if meta::cfg_holds(c.as_deref().unwrap_or_default(), facts)? {
                    on.push(format!("<a href=\"{}#{d}\"><code>{d}</code></a>", rel(PATH, "reference/crates/index.html")));
                }
            }
            let _ = write!(body, "<td>{}</td>", if on.is_empty() { "—".to_string() } else { on.join("<br>") });
        }
        body.push_str("</tr>");
    }
    body.push_str("</tbody></table>");
    if rows == 0 {
        return Err("no platform-gated dependencies found".into());
    }
    // Formats: the umbrella exports both from one cdylib.
    let stave = pkgs.iter().find(|p| p.name == "stave").ok_or("no stave package")?;
    let formats: Vec<&str> = ["stave-vst3", "stave-clap"].into_iter().filter(|f| stave.deps.iter().any(|(d, c)| d == f && c.is_none())).collect();
    let _ = write!(
        body,
        "<h2 id=\"formats\">Formats</h2><p><code>stave</code> depends on {} on every platform, so <code>stave::export!</code> makes one cdylib both a VST3 and a CLAP plugin wherever it builds.</p>",
        formats.iter().map(|f| format!("<a href=\"{}#{f}\"><code>{f}</code></a>", rel(PATH, "reference/crates/index.html"))).collect::<Vec<_>>().join(" and ")
    );
    if formats.len() != 2 {
        return Err("stave no longer depends on both format crates unconditionally; the platform page needs a new reading".into());
    }
    // CI.
    let jobs = meta::jobs(cx.repo)?;
    body.push_str("<h2 id=\"ci\">CI jobs, by platform</h2><table class=\"matrix\"><thead><tr><th>Workflow</th><th>Job</th>");
    for (name, _) in meta::PLATFORMS {
        let _ = write!(body, "<th>{name}</th>");
    }
    body.push_str("</tr></thead><tbody>");
    for (wf, js) in meta::jobs_by_workflow(&jobs) {
        for j in js {
            let _ = write!(body, "<tr><td>{}<div class=\"meta\">{}</div></td><td>{}</td>", src_link(&format!(".github/workflows/{wf}")), esc(&j.trigger), esc(&j.name));
            for (name, _) in meta::PLATFORMS {
                let on = j.runners.iter().any(|r| meta::runner_platform(r) == *name);
                let _ = write!(body, "<td class=\"{}\">{}</td>", if on { "yes" } else { "no" }, if on { "✓" } else { "" });
            }
            body.push_str("</tr>");
        }
    }
    body.push_str("</tbody></table>");
    Ok(Generated {
        path: PATH,
        title: "Platforms and formats",
        description: "What runs where: the platform crate each package uses on macOS, Windows and Linux, the plugin formats, and the CI jobs on each platform.".into(),
        from: format!("Generated from <code>cargo metadata</code> and {}", src_link(".github/workflows")),
        body,
        wide: false,
    })
}

// ------------------------------------------------------------- units

/// What a `Unit` variant is called in code: exhaustive, so a new
/// variant does not build until this page shows it.
fn unit_syntax(u: &Unit) -> (&'static str, String) {
    match u {
        Unit::Plain { decimals } => ("Plain", format!("Unit::Plain {{ decimals: {decimals} }}")),
        Unit::Db => ("Db", "Unit::Db".into()),
        Unit::DbFloor(f) => ("DbFloor", format!("Unit::DbFloor({f})")),
        Unit::Hz => ("Hz", "Unit::Hz".into()),
        Unit::Ms => ("Ms", "Unit::Ms".into()),
        Unit::Percent => ("Percent", "Unit::Percent".into()),
        Unit::Ratio => ("Ratio", "Unit::Ratio".into()),
        Unit::Custom { label, decimals } => ("Custom", format!("Unit::Custom {{ label: \"{label}\", decimals: {decimals} }}")),
    }
}

pub fn units(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/units/index.html";
    // Values chosen to show each unit's switch points; what they read
    // as is the code's, below.
    let samples: Vec<(Unit, Vec<f64>)> = vec![
        (Unit::Plain { decimals: 2 }, vec![0.0, 0.5, -1.25, 1234.5678]),
        (Unit::Db, vec![-60.0, -6.02, 0.0, 3.5, 24.0]),
        (Unit::DbFloor(-60), vec![-80.0, -60.0, -59.9, -12.0, 0.0]),
        (Unit::Hz, vec![20.0, 440.0, 999.94, 999.96, 2400.0, 20000.0]),
        (Unit::Ms, vec![0.5, 12.5, 999.94, 999.96, 1250.0, 10000.0]),
        (Unit::Percent, vec![0.0, 0.25, 0.333, 1.0]),
        (Unit::Ratio, vec![1.0, 4.0, 20.0]),
        (Unit::Custom { label: "kΩ", decimals: 2 }, vec![2.2, 47.0]),
    ];
    let all = rustdoc::all_items(cx.doc, "stave_core")?;
    let page = find_item(&all, "Unit")?;
    let html = rustdoc::read(cx.doc, &page)?;
    let mut body = format!(
        "<p>Every kind of {} a parameter can declare, what it is for, and what values read as — each row's text is <code>Unit::format</code>'s, computed when this page was built, and parses back with <code>Unit::parse</code> as shown.</p>",
        api(PATH, &page, "Unit")
    );
    let mut seen = BTreeSet::new();
    for (unit, values) in &samples {
        let (variant, syntax) = unit_syntax(unit);
        seen.insert(variant);
        let doc = variant_doc(&html, variant).ok_or_else(|| format!("Unit::{variant} has no docs on {page}"))?;
        let _ = write!(
            body,
            "<h2 id=\"{v}\"><code>{v}</code></h2><p>{doc}</p><table class=\"units\"><thead><tr><th>Value</th><th>Reads</th><th>Parses back to</th></tr></thead><tbody>",
            v = variant.to_lowercase(),
            doc = esc(&doc)
        );
        for &v in values {
            let text = unit.format(v).to_string();
            let back = unit.parse(&text).map(|b| format!("{b}")).unwrap_or_else(|| "—".into());
            let _ = write!(body, "<tr><td><code>{v}</code></td><td><code>{}</code></td><td><code>{}</code></td></tr>", esc(&text), esc(&back));
        }
        let _ = write!(body, "</tbody></table><p class=\"meta\">As declared: <code>{}</code> · label <code>{}</code></p>", esc(&syntax), esc(&format!("{:?}", unit.label())));
    }
    if seen.len() != 8 {
        return Err("every Unit variant needs a sample on the units page".into());
    }
    Ok(Generated {
        path: PATH,
        title: "Units",
        description: "Every Unit a parameter can declare, with what values read as and parse back to — computed from Unit::format and Unit::parse.".into(),
        from: format!("Generated from {} by running it", src_link("crates/stave-core/src/params/display.rs")),
        body,
        wide: false,
    })
}

/// The first paragraph of variant `name`'s docs on an enum's page.
fn variant_doc(html: &str, name: &str) -> Option<String> {
    let at = html.find(&format!("id=\"variant.{name}\""))?;
    let next = html[at + 10..].find("<section id=\"variant.").map(|n| n + at + 10).unwrap_or(html.len());
    let d = html[at..next].find("<div class=\"docblock\">")? + at;
    Some(rustdoc::summary(&rustdoc::docblock_after(html, d)?))
}

// ------------------------------------------------------------ themes

fn byte(c: f64) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn hex_color(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

fn hex_rgb(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", byte(linear_to_srgb(c[0])), byte(linear_to_srgb(c[1])), byte(linear_to_srgb(c[2])))
}

fn swatch(name: &str, css: &str, note: &str) -> String {
    format!("<li><span class=\"chip\" style=\"background:{css}\"></span><code>{}</code><span>{}</span></li>", esc(name), esc(&format!("{css}{note}")))
}

pub fn themes(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/themes/index.html";
    let all = rustdoc::all_items(cx.doc, "stave_face")?;
    let theme_page = find_item(&all, "Theme")?;
    let mut body = format!(
        "<p>Every built-in {} (<code>Theme::all()</code>): the showcase story drawn on it with the embedded fonts, and each colour and measure it holds. A theme is the whole look as data; parts never mint a colour, a font or a coordinate.</p>",
        api(PATH, &theme_page, "Theme")
    );
    let dir = cx.out.join("reference/themes");
    fs::create_dir_all(&dir)?;
    body.push_str("<div class=\"theme-index\">");
    for t in Theme::all() {
        let _ = write!(body, "<a href=\"#{n}\">{n}</a>", n = esc(t.name));
    }
    body.push_str("</div>");
    for t in Theme::all() {
        let params = Arc::new(StoryParams::default());
        let face = stories::showcase(&params, t.clone());
        let canvas = catalogue::draw(face, params, &[], 1.0, Text::Type);
        let file = format!("{}.png", t.name);
        fs::write(dir.join(&file), stave_raster::png::encode(&canvas))?;
        // Exhaustive destructuring: a new field does not build until it shows here.
        let Theme { name, finish, palette, hardware, typography, spacing, coverings } = t;
        let Finish { coat, surface, plate, desk, radius } = finish;
        let Palette { ink, ink_soft, accent, screen, phosphor } = palette;
        let Hardware { caps, cap, pointer, lamp } = hardware;
        let Coverings { tolex, piping, grille, badge } = coverings;
        let Typography { title: _, title_size, tagline: _, tagline_size, legend: _, legend_size, caption: _, caption_size, value: _, value_size } = typography;
        let coat = match coat {
            Coat::Anodized => "Anodized",
            Coat::Powder => "Powder",
            Coat::Hammertone => "Hammertone",
            Coat::Crinkle => "Crinkle",
            Coat::Enamel => "Enamel",
        };
        let caps = match caps {
            CapStyle::Aluminium => "Aluminium",
            CapStyle::Dome => "Dome",
            CapStyle::Soft => "Soft",
            CapStyle::Chicken => "Chicken",
        };
        let alpha = |c: Color| if c.a < 1.0 { format!(" · α {:.2}", c.a) } else { String::new() };
        let _ = write!(
            body,
            "<section class=\"theme\" id=\"{n}\"><h2 id=\"theme-{n}\"><code>Theme::{n}()</code></h2><figure class=\"shot\"><img src=\"{file}\" alt=\"The showcase story on the {n} theme\" width=\"{w}\" height=\"{h}\" loading=\"lazy\"></figure><div class=\"theme-data\">",
            n = esc(name),
            w = canvas.w(),
            h = canvas.h()
        );
        let _ = write!(
            body,
            "<div><h3>Palette</h3><ul class=\"swatches\">{}{}{}{}{}</ul></div>",
            swatch("ink", &hex_color(ink), &alpha(ink)),
            swatch("ink_soft", &hex_color(ink_soft), &alpha(ink_soft)),
            swatch("accent", &hex_color(accent), &alpha(accent)),
            swatch("screen", &hex_color(screen), &alpha(screen)),
            swatch("phosphor", &hex_color(phosphor), &alpha(phosphor)),
        );
        let _ = write!(
            body,
            "<div><h3>Finish · <code>Coat::{coat}</code></h3><ul class=\"swatches\">{}{}{}<li><span class=\"chip none\"></span><code>radius</code><span>{radius} pt</span></li></ul></div>",
            swatch("surface", &hex_rgb(surface), " (linear → sRGB)"),
            swatch("plate", &hex_rgb(plate), ""),
            swatch("desk", &hex_rgb(desk), ""),
        );
        let _ = write!(
            body,
            "<div><h3>Hardware · <code>CapStyle::{caps}</code></h3><ul class=\"swatches\">{}{}{}</ul></div>",
            swatch("cap", &hex_rgb(cap), ""),
            swatch("pointer", &hex_color(pointer), &alpha(pointer)),
            swatch("lamp", &hex_rgb(lamp), if lamp.iter().any(|c| *c > 1.0) { " (hot core, > 1)" } else { "" }),
        );
        let _ = write!(
            body,
            "<div><h3>Coverings</h3><ul class=\"swatches\">{}{}{}{}</ul></div>",
            swatch("tolex", &hex_rgb(tolex), ""),
            swatch("piping", &hex_rgb(piping), ""),
            swatch("grille", &hex_rgb(grille), ""),
            swatch("badge", &hex_rgb(badge), ""),
        );
        let _ = write!(
            body,
            "<div><h3>Type and space</h3><table class=\"kv\"><tr><td>title</td><td>{title_size} pt</td></tr><tr><td>tagline</td><td>{tagline_size} pt</td></tr><tr><td>legend</td><td>{legend_size} pt</td></tr><tr><td>caption</td><td>{caption_size} pt</td></tr><tr><td>value</td><td>{value_size} pt</td></tr><tr><td>spacing</td><td>{}</td></tr></table></div>",
            spacing.iter().map(|s| format!("{s}")).collect::<Vec<_>>().join(" · ")
        );
        body.push_str("</div></section>");
    }
    Ok(Generated {
        path: PATH,
        title: "Themes",
        description: format!("The {} built-in themes, each drawn and listed colour by colour, from Theme::all().", Theme::all().len()),
        from: format!("Generated from {} · pictures drawn at build time", src_link("crates/stave-face/src/theme.rs")),
        body,
        wide: true,
    })
}

// --------------------------------------------------------- catalogue

pub fn catalogue_page(cx: &Ctx<'_>) -> Result<(Generated, usize)> {
    const PATH: &str = "reference/catalogue/index.html";
    let all = rustdoc::all_items(cx.doc, "stave_face")?;
    let part_trait = find_item(&all, "Part")?;
    // Every public part type has a picture.
    let mut parts: Vec<&str> = Vec::new();
    for e in catalogue::ENTRIES {
        if !parts.contains(&e.part) {
            parts.push(e.part);
        }
    }
    for imp in rustdoc::implementors(cx.doc, &part_trait, "Part")? {
        if !parts.contains(&imp.as_str()) {
            return Err(format!("{imp} implements Part but has no picture in tools/storybook/src/catalogue.rs").into());
        }
    }
    let dir = cx.out.join("reference/catalogue");
    fs::create_dir_all(&dir)?;
    let mut body = format!(
        "<p>Every part of the design system, drawn by the face runtime from the real part when this page was built — alone, on the graphite theme, at rest and with the readings that light it. The same list is held to golden pictures by {}. A part is anything that implements {}.</p><div class=\"theme-index\">",
        src_link("tools/storybook/tests/parts.rs"),
        api(PATH, &part_trait, "Part")
    );
    for p in &parts {
        let _ = write!(body, "<a href=\"#{}\">{p}</a>", p.to_lowercase());
    }
    body.push_str("</div>");
    for part in &parts {
        let page = find_item(&all, part)?;
        let summary = rustdoc::top_doc(&rustdoc::read(cx.doc, &page)?).map(|d| rustdoc::summary(&d)).unwrap_or_default();
        if summary.is_empty() {
            return Err(format!("{part} has no doc comment; the catalogue shows each part's own summary").into());
        }
        let _ = write!(body, "<section class=\"part\" id=\"{}\"><h2 id=\"part-{}\">{}</h2><p>{}</p><div class=\"shots\">", part.to_lowercase(), part.to_lowercase(), api(PATH, &page, part), esc(&summary));
        for e in catalogue::ENTRIES.iter().filter(|e| e.part == *part) {
            let canvas = catalogue::picture(e, 2.0, Text::Type);
            let file = format!("{}.png", e.name);
            fs::write(dir.join(&file), stave_raster::png::encode(&canvas))?;
            let _ = write!(
                body,
                "<figure><div class=\"stage\"><img src=\"{file}\" alt=\"{alt}\" width=\"{w}\" height=\"{h}\" loading=\"lazy\"></div><figcaption>{alt}</figcaption></figure>",
                alt = esc(e.shows),
                w = canvas.w() / 2,
                h = canvas.h() / 2
            );
        }
        body.push_str("</div></section>");
    }
    Ok((
        Generated {
            path: PATH,
            title: "Part catalogue",
            description: format!("All {} part types of the design system, each drawn from the real part at build time.", parts.len()),
            from: format!("Generated from {} · pictures drawn at build time", src_link("tools/storybook/src/catalogue.rs")),
            body,
            wide: true,
        },
        parts.len(),
    ))
}

// --------------------------------------------------------------- CLI

pub fn cli(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "reference/cli/index.html";
    let mut body = String::from("<p>Every binary in the workspace, and its own <code>--help</code>, captured by running it when this page was built.</p>");
    let mut n = 0;
    for p in &cx.ws.packages {
        for bin in &p.bins {
            let out = Command::new(env!("CARGO")).args(["run", "-q", "--locked", "-p", &p.name, "--bin", bin, "--", "--help"]).current_dir(cx.repo).output()?;
            if !out.status.success() {
                return Err(format!("`{bin} --help` failed: {}", String::from_utf8_lossy(&out.stderr)).into());
            }
            let help = String::from_utf8(out.stdout)?;
            if help.trim().is_empty() {
                return Err(format!("`{bin} --help` printed nothing").into());
            }
            n += 1;
            let _ = write!(
                body,
                "<h2 id=\"{bin}\"><code>{bin}</code></h2><p>{}</p><p class=\"meta\">Package <code>{}</code> · <code>cargo run -p {} --bin {bin} -- --help</code></p><pre class=\"terminal\"><code>{}</code></pre>",
                esc(&p.description),
                esc(&p.name),
                esc(&p.name),
                esc(help.trim_end())
            );
        }
    }
    if n == 0 {
        return Err("no binaries found".into());
    }
    Ok(Generated {
        path: PATH,
        title: "Command-line tools",
        description: "The --help of every binary in the workspace (stave-bench, stave-play, stave-events and more), captured by running each one.".into(),
        from: "Generated by running each binary with <code>--help</code>".into(),
        body,
        wide: false,
    })
}

// ------------------------------------------------------ plugin checks

/// One test `plugin_checks!` mounts: its name, its doc, its conditions.
struct Check {
    name: String,
    doc: String,
    when: Vec<&'static str>,
}

fn checks_of(src: &str) -> Result<Vec<Check>> {
    let body = &src[src.find("macro_rules! plugin_checks").ok_or("no plugin_checks! macro")?..];
    // Only this macro: the helper macros after it mount tests of their own.
    let body = match body["macro_rules!".len()..].find("macro_rules!") {
        Some(end) => &body[..end + "macro_rules!".len()],
        None => body,
    };
    let mut out = Vec::new();
    let mut doc = String::new();
    let mut when = Vec::new();
    let mut is_test = false;
    // Inside a documented `mod` of tests (one check, many tests): its depth.
    let mut in_mod: Option<i32> = None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(depth) = in_mod.as_mut() {
            *depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
            if *depth <= 0 {
                in_mod = None;
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("mod ").filter(|r| r.ends_with('{')) {
            // A group of tests that is one check (`passes_validation`).
            let name = rest.trim_end_matches('{').trim().to_string();
            if doc.trim().is_empty() {
                return Err(format!("plugin_checks!: mod {name} has no doc comment").into());
            }
            out.push(Check { name, doc: doc.trim().to_string(), when: std::mem::take(&mut when) });
            doc.clear();
            in_mod = Some(1);
            continue;
        }
        if let Some(d) = t.strip_prefix("///") {
            doc.push_str(d.trim());
            doc.push(' ');
        } else if t == "#[test]" {
            is_test = true;
        } else if t.starts_with("#[cfg(debug_assertions)]") {
            when.push("debug builds only");
        } else if t.starts_with("#[cfg_attr(miri, ignore)]") {
            when.push("skipped under Miri");
        } else if let Some(rest) = t.strip_prefix("fn ") {
            if is_test {
                let name = rest.split('(').next().unwrap_or_default().to_string();
                if doc.trim().is_empty() {
                    return Err(format!("plugin_checks!: test {name} has no doc comment").into());
                }
                out.push(Check { name, doc: doc.trim().to_string(), when: std::mem::take(&mut when) });
            }
            doc.clear();
            when.clear();
            is_test = false;
        } else if !t.starts_with("#[") {
            doc.clear();
            when.clear();
            is_test = false;
        }
    }
    if out.is_empty() {
        return Err("plugin_checks! mounts no tests?".into());
    }
    Ok(out)
}

/// A doc comment's `[`text`](crate::path)` / `[`crate::path`]` links, made
/// API links (the paths are stave-test's).
fn link_doc(doc: &str, from: &str, all: &BTreeMap<String, String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = doc;
    while let Some(i) = rest.find("[`") {
        out.push_str(&esc(&rest[..i]));
        rest = &rest[i + 2..];
        let end = rest.find("`]").ok_or("unterminated doc link")?;
        let path = rest[..end].trim_start_matches("crate::").to_string();
        rest = &rest[end + 2..];
        let target = all.get(&path).cloned().or_else(|| {
            // A module: its index page.
            let m = format!("stave_test/{}/index.html", path.replace("::", "/"));
            Some(m)
        });
        match target {
            Some(t) => out.push_str(&api(from, &t, &path)),
            None => return Err(format!("doc link to unknown {path}").into()),
        }
    }
    out.push_str(&esc(rest));
    // Inline code.
    let mut s = String::new();
    for (k, part) in out.split('`').enumerate() {
        if k % 2 == 1 {
            let _ = write!(s, "<code>{part}</code>");
        } else {
            s.push_str(part);
        }
    }
    Ok(s)
}

pub fn plugin_checks(cx: &Ctx<'_>) -> Result<(Generated, usize)> {
    const PATH: &str = "reference/plugin-checks/index.html";
    const SRC: &str = "crates/stave-test/src/plugin_checks.rs";
    let src = fs::read_to_string(cx.repo.join(SRC))?;
    let checks = checks_of(&src)?;
    let all = rustdoc::all_items(cx.doc, "stave_test")?;
    let config = find_item(&all, "Config")?;
    let mac = all.get("plugin_checks").cloned().ok_or("no plugin_checks macro page")?;
    let mut body = format!(
        "<p>One line in a plugin's <code>tests/checks.rs</code> — {} — mounts each of these as a test of its own. This list is read from the macro's source, so it is exactly what the macro expands to. Its options are a {}.</p><pre class=\"rust\"><code>stave_test::plugin_checks!(my_plugin::MyPlugin);</code></pre><table class=\"checks\"><thead><tr><th>Test</th><th>What it holds the plugin to</th></tr></thead><tbody>",
        api(PATH, &mac, "plugin_checks!"),
        api(PATH, &config, "Config")
    );
    for c in &checks {
        let _ = write!(
            body,
            "<tr id=\"{n}\"><td><code>{n}</code>{w}</td><td>{d}</td></tr>",
            n = esc(&c.name),
            w = c.when.iter().map(|w| format!("<div class=\"meta\">{w}</div>")).collect::<String>(),
            d = link_doc(&c.doc, PATH, &all)?
        );
    }
    body.push_str("</tbody></table>");
    // The options, from Config's own field docs.
    let html = rustdoc::read(cx.doc, &config)?;
    body.push_str("<h2 id=\"options\">Options</h2><table class=\"checks\"><thead><tr><th>Field</th><th>Meaning</th></tr></thead><tbody>");
    let mut fields = 0;
    let mut rest = html.as_str();
    while let Some(i) = rest.find("id=\"structfield.") {
        rest = &rest[i + 16..];
        let name = &rest[..rest.find('"').unwrap_or(0)];
        let ty_end = rest.find("</code>").unwrap_or(0);
        let ty = text_of(&rest[..ty_end]);
        let ty = ty.split_once(':').map(|(_, t)| t.trim().to_string()).unwrap_or_default();
        let doc = rest.find("<div class=\"docblock\">").and_then(|d| rustdoc::docblock_after(rest, d)).map(|b| rustdoc::summary(&b)).unwrap_or_default();
        fields += 1;
        let _ = write!(body, "<tr><td><code>{}</code><div class=\"meta\"><code>{}</code></div></td><td>{}</td></tr>", esc(name), esc(&ty), esc(&doc));
    }
    body.push_str("</tbody></table>");
    if fields == 0 {
        return Err("Config has no documented fields".into());
    }
    let n = checks.len();
    Ok((
        Generated {
            path: PATH,
            title: "Plugin checks",
            description: format!("The {n} tests stave_test::plugin_checks! mounts on a plugin, read from the macro itself."),
            from: format!("Generated from {}", src_link(SRC)),
            body,
            wide: false,
        },
        n,
    ))
}

// --------------------------------------------------------- changelog

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(repo).output()?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)).into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// A release's notes, `releases/<tag>.md`: a title line, then paragraphs,
/// `- ` bullets and short headings (a line followed by bullets). Every tag
/// has notes, or the site does not build.
fn release_notes(cx: &Ctx<'_>, tag: &str) -> Result<String> {
    let path = cx.repo.join("releases").join(format!("{tag}.md"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e} — every release tag needs its notes", path.display()))?;
    let inline = |s: &str| {
        let mut out = String::new();
        for (k, part) in esc(s).split('`').enumerate() {
            if k % 2 == 1 {
                let _ = write!(out, "<code>{part}</code>");
            } else {
                out.push_str(part);
            }
        }
        out
    };
    let mut html = String::from("<div class=\"notes\">");
    let lines: Vec<&str> = text.lines().skip(1).collect();
    let mut in_list = false;
    for (k, line) in lines.iter().enumerate() {
        let line = line.trim_end();
        if let Some(item) = line.strip_prefix("- ") {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            let _ = write!(html, "<li>{}</li>", inline(item));
            continue;
        }
        if in_list {
            html.push_str("</ul>");
            in_list = false;
        }
        if line.is_empty() {
            continue;
        }
        let heading = lines.get(k + 1).is_some_and(|next| next.starts_with("- "));
        let _ = write!(html, "{}", if heading { format!("<h3>{}</h3>", inline(line)) } else { format!("<p>{}</p>", inline(line)) });
    }
    if in_list {
        html.push_str("</ul>");
    }
    html.push_str("</div>");
    Ok(html)
}

pub fn changelog(cx: &Ctx<'_>) -> Result<Generated> {
    const PATH: &str = "changelog/index.html";
    let tags: Vec<String> = git(cx.repo, &["tag", "--list", "v*", "--sort=-v:refname"])?.lines().map(String::from).collect();
    if tags.is_empty() {
        return Err("no release tags (v*) — the deploy checkout needs fetch-depth: 0".into());
    }
    let mut body = String::from("<p>Every release, newest first, from the repository's <code>v*</code> tags: its notes (<code>releases/&lt;tag&gt;.md</code>), then the commits it added since the release before it. Commit subjects follow the ledger's items (S, L, A, D, F, T, C, V, U, X …).</p>");
    let mut spans: Vec<(String, Option<String>, String)> = Vec::new();
    spans.push(("Unreleased".into(), None, format!("{}..HEAD", tags[0])));
    for (k, t) in tags.iter().enumerate() {
        let range = match tags.get(k + 1) {
            Some(prev) => format!("{prev}..{t}"),
            None => t.clone(),
        };
        spans.push((t.clone(), Some(t.clone()), range));
    }
    for (title, tag, range) in spans {
        let log = git(cx.repo, &["log", "--no-merges", "--format=%h%x09%cs%x09%s", &range])?;
        let lines: Vec<&str> = log.lines().collect();
        if lines.is_empty() && tag.is_none() {
            continue;
        }
        let date = match &tag {
            Some(t) => git(cx.repo, &["log", "-1", "--format=%cs", t])?.trim().to_string(),
            None => "since the last release".into(),
        };
        let id = title.replace('.', "-").to_lowercase();
        let heading = match &tag {
            Some(t) => format!("<a href=\"{GITHUB}/releases/tag/{t}\">{t}</a>"),
            None => title.clone(),
        };
        let _ = write!(body, "<h2 id=\"{id}\">{heading} <span class=\"date\">{date} · {} commits</span></h2>", lines.len());
        if let Some(t) = &tag {
            body.push_str(&release_notes(cx, t)?);
        }
        body.push_str("<ul class=\"commits\">");
        for l in lines {
            let mut f = l.splitn(3, '\t');
            let (h, d, s) = (f.next().unwrap_or_default(), f.next().unwrap_or_default(), f.next().unwrap_or_default());
            let _ = write!(body, "<li><a href=\"{GITHUB}/commit/{h}\"><code>{h}</code></a> <span class=\"date\">{d}</span> {}</li>", esc(s));
        }
        body.push_str("</ul>");
    }
    Ok(Generated {
        path: PATH,
        title: "Changelog",
        description: format!("Every Stave release from its git tag, newest first ({} releases).", tags.len()),
        from: "Generated from git tags and history".into(),
        body,
        wide: false,
    })
}

// --------------------------------------------------- reference index

/// The reference hub: every crate's API, then the generated pages.
pub fn index(cx: &Ctx<'_>, generated: &[(&str, &str, String)]) -> Result<Generated> {
    const PATH: &str = "reference/index.html";
    let mut body = String::from("<p>The API of every crate, as rustdoc documents it (with links to the source), and the pages computed from the code: nothing below is written by hand.</p><h2 id=\"api\">API, by crate</h2><p>Start with <code>stave</code>: it is the one dependency a plugin takes, and its prelude re-exports what most plugins need.</p><div class=\"crate-list\">");
    for p in &cx.ws.packages {
        let Some(lib) = &p.lib else { continue };
        if p.name == "stave-guide" || !cx.doc.join(lib).join("index.html").exists() {
            continue;
        }
        let summary = rustdoc::top_doc(&rustdoc::read(cx.doc, &format!("{lib}/index.html"))?).map(|d| rustdoc::summary(&d)).unwrap_or_default();
        let _ = write!(
            body,
            "<a class=\"crate{}\" href=\"{}\"><strong>{}</strong><span>{}</span></a>",
            if p.name == "stave" { " primary" } else { "" },
            rel(PATH, &format!("api/{lib}/index.html")),
            esc(&p.name),
            esc(if summary.is_empty() { &p.description } else { &summary })
        );
    }
    body.push_str("</div><h2 id=\"generated\">Generated from the code</h2><div class=\"cards\">");
    for (path, title, desc) in generated {
        let _ = write!(body, "<a href=\"{}\"><strong>{}</strong><span>{}</span></a>", rel(PATH, path), esc(title), esc(desc));
    }
    body.push_str("</div>");
    Ok(Generated {
        path: PATH,
        title: "Reference",
        description: "The API of every Stave crate and the reference pages generated from the code.".into(),
        from: String::new(),
        body,
        wide: false,
    })
}

/// The reference section's sidebar.
pub fn sidebar(cx: &Ctx<'_>, generated: &[(&str, &str, String)]) -> Vec<Group> {
    let mut api_links = Vec::new();
    for p in &cx.ws.packages {
        if let Some(lib) = &p.lib {
            if p.name != "stave-guide" && cx.doc.join(lib).join("index.html").exists() {
                api_links.push(Link { title: p.name.clone(), path: format!("api/{lib}/index.html") });
            }
        }
    }
    vec![
        Group { title: "Reference".into(), path: Some("reference/index.html".into()), links: generated.iter().map(|(p, t, _)| Link { title: t.to_string(), path: p.to_string() }).collect() },
        Group { title: "API".into(), path: None, links: api_links },
    ]
}

/// A generated page laid out.
pub fn page(g: &Generated, sidebar: Vec<Group>) -> Page {
    Page {
        path: g.path.to_string(),
        title: g.title.to_string(),
        description: g.description.clone(),
        section: Section::Reference,
        body: g.body.clone(),
        sidebar,
        source: None,
        prev: None,
        next: None,
        kicker: g.from.clone(),
        wide: g.wide,
    }
}
