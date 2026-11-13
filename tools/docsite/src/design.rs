//! The Design studies reference page (DSP.md G2, G3): every committed
//! `crates/stave-test/design/*.design`, drawn as SVG at build time —
//! the response across a production run's built units (nominal, median
//! and the 5–95 % band), each part's sensitivity across the band (dB
//! per 1 % change), which grade dominates the spread, and the spread of
//! any single-frequency quantities (THD, gain). Nothing drawn is
//! committed: the files are what `design_studies.rs` measures and CI
//! diffs.
//!
//! The build fails on a study that does not parse or has nothing to
//! draw.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::fingerprints::{plot, Series};
use crate::html::esc;
use crate::reference::{src_link, Ctx, Generated};
use crate::Result;

const DIR: &str = "crates/stave-test/design";
const PATH: &str = "reference/design/index.html";
/// Series colours for the parts, in the order the study ranks them.
const PALETTE: usize = 8;

struct Part {
    name: String,
    grade: String,
    share: f64,
}

struct Scalar {
    name: String,
    unit: String,
    /// nominal, p5, p50, p95, linear σ.
    values: [f64; 5],
    slopes: Vec<(String, f64)>,
}

struct Study {
    case: String,
    file: String,
    model: String,
    units: String,
    verdict: String,
    parts: Vec<Part>,
    /// `hz → [nominal, p5, p50, p95]`.
    bands: Vec<(f64, [f64; 4])>,
    /// `part → [(hz, dB per %)]`.
    slopes: BTreeMap<String, Vec<(f64, f64)>>,
    scalars: Vec<Scalar>,
}

fn read(repo: &Path) -> Result<Vec<Study>> {
    let mut paths: Vec<_> = fs::read_dir(repo.join(DIR))
        .map_err(|e| format!("{DIR}: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "design"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let file = format!("{DIR}/{}", path.file_name().unwrap_or_default().to_string_lossy());
        let text = fs::read_to_string(&path).map_err(|e| format!("{file}: {e}"))?;
        let mut lines = text.lines();
        let case = lines
            .next()
            .and_then(|l| l.strip_prefix("# stave design v1: "))
            .ok_or_else(|| format!("{file}: not a stave design study"))?
            .to_string();
        let mut s = Study {
            case,
            file: file.clone(),
            model: String::new(),
            units: String::new(),
            verdict: String::new(),
            parts: Vec::new(),
            bands: Vec::new(),
            slopes: BTreeMap::new(),
            scalars: Vec::new(),
        };
        for line in lines {
            let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
            let w: Vec<&str> = rest.split_whitespace().collect();
            let num = |s: &str| s.parse::<f64>().map_err(|e| format!("{file}: {s}: {e}"));
            match head {
                "model" => s.model = rest.into(),
                "units" => s.units = rest.into(),
                "verdict" => s.verdict = rest.into(),
                "part" if w.len() == 5 => s.parts.push(Part {
                    name: w[0].into(),
                    grade: format!("±{} % {}", w[1], w[2]),
                    share: num(w[4])?,
                }),
                "band" if w.len() == 5 => s.bands.push((num(w[0])?, [num(w[1])?, num(w[2])?, num(w[3])?, num(w[4])?])),
                "slope" if w.len() == 3 => s.slopes.entry(w[0].into()).or_default().push((num(w[1])?, num(w[2])?)),
                "scalar" if w.len() == 7 => s.scalars.push(Scalar {
                    name: w[0].into(),
                    unit: w[1].into(),
                    values: [num(w[2])?, num(w[3])?, num(w[4])?, num(w[5])?, num(w[6])?],
                    slopes: Vec::new(),
                }),
                "scalar-slope" if w.len() == 3 => {
                    let v = num(w[2])?;
                    let sc = s.scalars.iter_mut().find(|x| x.name == w[0]).ok_or_else(|| format!("{file}: a slope of no scalar {}", w[0]))?;
                    sc.slopes.push((w[1].into(), v));
                }
                "" | "#" => {}
                _ if head.starts_with('#') => {}
                _ => return Err(format!("{file}: unreadable line {line:?}").into()),
            }
        }
        if s.parts.is_empty() || (s.bands.is_empty() && s.scalars.is_empty()) {
            return Err(format!("{file}: nothing to draw").into());
        }
        out.push(s);
    }
    if out.is_empty() {
        return Err(format!("{DIR}: no design studies").into());
    }
    Ok(out)
}

fn section(s: &Study) -> String {
    let mut h = format!(
        r#"<section class="fingerprint" id="design-{}"><h2><code>{}</code></h2><p>{} · {} · <a href="{}">study</a></p>"#,
        esc(&s.case.replace('_', "-")),
        esc(&s.case),
        esc(&s.model),
        esc(&s.units),
        format_args!("{}/blob/main/{}", crate::reference::GITHUB, esc(&s.file)),
    );
    if !s.bands.is_empty() {
        h.push_str(r#"<div class="fp-plots">"#);
        let line = |label: &str, class: &'static str, dashed: bool, k: usize| Series {
            label: label.into(),
            class,
            dashed,
            points: s.bands.iter().map(|(hz, v)| (*hz, v[k])).collect(),
        };
        let band = [
            line("5th percentile", "os", true, 1),
            line("95th percentile", "os", true, 3),
            line("median unit", "ref", false, 2),
            line("nominal", "ship", false, 0),
        ];
        h.push_str(&plot("Response across built units", "Hz", "dB", true, &band, None));
        // Parts in the study's rank order, each its own colour.
        let series: Vec<Series> = s
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let points = s.slopes.get(&p.name)?.clone();
                Some(Series {
                    label: p.name.clone(),
                    class: CLASSES[i % PALETTE],
                    dashed: i >= PALETTE,
                    points,
                })
            })
            .collect();
        h.push_str(&plot("Sensitivity per part", "Hz", "dB per 1 %", true, &series, Some((0.0, ""))));
        h.push_str("</div>");
        h.push_str(r#"<p class="design-legend">"#);
        for (i, p) in s.parts.iter().enumerate() {
            let _ = write!(h, r#"<span><svg viewBox="0 0 14 4" width="14" height="4"><line class="{}" x1="0" x2="14" y1="2" y2="2"/></svg> {}</span> "#, CLASSES[i % PALETTE], esc(&p.name));
        }
        h.push_str("<span>— band: nominal (accent), median unit, 5th and 95th percentiles (dashed)</span></p>");
    }
    h.push_str(r#"<table class="cost"><thead><tr><th>Part</th><th>Grade</th><th>Share of the spread it most affects</th></tr></thead><tbody>"#);
    for p in &s.parts {
        let _ = write!(h, "<tr><td><code>{}</code></td><td>{}</td><td><code>{:.1} %</code></td></tr>", esc(&p.name), esc(&p.grade), p.share * 100.0);
    }
    let _ = write!(h, "</tbody></table><p><strong>{}</strong></p>", esc(&s.verdict));
    if !s.scalars.is_empty() {
        h.push_str(r#"<table class="cost"><thead><tr><th>Quantity</th><th>Nominal</th><th>5 %</th><th>Median</th><th>95 %</th><th>Linear σ</th><th>Slope per 1 %</th></tr></thead><tbody>"#);
        for sc in &s.scalars {
            let slopes: Vec<String> = sc.slopes.iter().map(|(p, v)| format!("{} {v:+.4}", esc(p))).collect();
            let [n, p5, p50, p95, sigma] = sc.values;
            let u = esc(&sc.unit);
            let _ = write!(
                h,
                "<tr><td><code>{}</code></td><td><code>{n:.3} {u}</code></td><td><code>{p5:.3}</code></td><td><code>{p50:.3}</code></td><td><code>{p95:.3}</code></td><td><code>{sigma:.3}</code></td><td><code>{}</code></td></tr>",
                esc(&sc.name),
                slopes.join(", ")
            );
        }
        h.push_str("</tbody></table>");
    }
    h.push_str("</section>");
    h
}

const CLASSES: [&str; PALETTE] = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];

pub fn page(cx: &Ctx<'_>) -> Result<Generated> {
    let studies = read(cx.repo)?;
    let mut body = format!(
        "<p>What a circuit does across the parts it is built from (DSP.md G2, G3): each part's <em>sensitivity</em> — the change in response per 1 % change of its value, the slope at the nominal design — and a <em>Monte Carlo</em> run of built units, each part drawn from its grade (seeded, so every machine builds the same units), with the share of the spread each grade causes to first order. Measured by <code>stave_measure::design</code> ({}), committed by {} and drawn here; CI fails when a study moves.</p>",
        src_link("crates/stave-measure/src/design/mod.rs"),
        src_link("crates/stave-test/tests/design_studies.rs"),
    );
    for s in &studies {
        body.push_str(&section(s));
    }
    Ok(Generated {
        path: PATH,
        title: "Design studies",
        description: format!("Part sensitivities and Monte Carlo tolerance spreads of {} circuits, with the part grade that dominates each.", studies.len()),
        from: format!("Generated from {}", src_link(DIR)),
        body,
        wide: false,
    })
}
