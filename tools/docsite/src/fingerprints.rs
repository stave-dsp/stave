//! The fingerprints on the model catalogue (DSP.md R18): every committed
//! `crates/stave-test/fingerprints/**/*.fingerprint`, drawn as SVG at
//! build time — the small-signal response, the harmonics against level,
//! the compression and the aliasing, each against its reference where it
//! has one — with its R19 metrics beside them. Nothing drawn is committed:
//! the plots are computed from the fingerprint's `plot` lines (points the
//! characterise tests derived from its tables), so they never drift from
//! what CI diffs.
//!
//! The build fails on a fingerprint that does not parse or names a
//! catalogued model (`catalogue …`) the catalogue lacks. A supply's
//! fingerprint (load steps, no tone) has nothing to plot and is listed
//! with its file.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::html::esc;
use crate::reference::GITHUB;
use crate::Result;

pub const DIR: &str = "crates/stave-test/fingerprints";

/// One fingerprint, as the catalogue reads it.
pub struct Fingerprint {
    pub case: String,
    pub file: String,
    pub model: String,
    pub catalogue: Option<String>,
    pub reference: String,
    pub playback: String,
    pub versions: String,
    /// `rate → role` of each table.
    pub tables: Vec<(String, String)>,
    /// `(rate, run, name) → value`.
    pub metrics: BTreeMap<(String, String, String), f64>,
    /// `kind → [(series, values)]`.
    pub plots: BTreeMap<String, Vec<(String, Vec<f64>)>>,
}

impl Fingerprint {
    /// The anchor its section has on the catalogue page.
    pub fn anchor(&self) -> String {
        format!("fp-{}", self.case.replace(['/', '_'], "-"))
    }

    /// The shipping-rate series name (a rate in Hz).
    fn shipping(&self) -> Option<&str> {
        self.tables.iter().find(|(_, role)| role != "oversampled").map(|(r, _)| r.as_str())
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "fingerprint") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every committed fingerprint.
pub fn read(repo: &Path) -> Result<Vec<Fingerprint>> {
    let mut out = Vec::new();
    for path in files(&repo.join(DIR)) {
        let file = path.strip_prefix(repo).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let text = fs::read_to_string(&path).map_err(|e| format!("{file}: {e}"))?;
        let mut lines = text.lines();
        let case = lines
            .next()
            .and_then(|l| l.strip_prefix("# stave fingerprint v1: "))
            .ok_or_else(|| format!("{file}: not a stave fingerprint"))?
            .to_string();
        let mut fp = Fingerprint {
            case,
            file: file.clone(),
            model: String::new(),
            catalogue: None,
            reference: String::new(),
            playback: String::new(),
            versions: String::new(),
            tables: Vec::new(),
            metrics: BTreeMap::new(),
            plots: BTreeMap::new(),
        };
        let mut in_table = false;
        for line in lines {
            if in_table {
                in_table = line != "end";
                continue;
            }
            let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
            let w: Vec<&str> = rest.split_whitespace().collect();
            let num = |s: &str| s.parse::<f64>().map_err(|e| format!("{file}: {s}: {e}"));
            match head {
                "model" => fp.model = rest.into(),
                "catalogue" => fp.catalogue = Some(rest.into()),
                "reference" => fp.reference = rest.into(),
                "playback" => fp.playback = rest.into(),
                "versions" => fp.versions = rest.into(),
                "metric" if w.len() == 4 => {
                    fp.metrics.insert((w[0].into(), w[1].into(), w[2].into()), num(w[3])?);
                }
                "plot" if w.len() >= 3 => {
                    let values = w[2..].iter().map(|s| num(s)).collect::<std::result::Result<Vec<_>, _>>()?;
                    fp.plots.entry(w[0].into()).or_default().push((w[1].into(), values));
                }
                "table" => {
                    fp.tables.push((w.first().unwrap_or(&"").to_string(), w.get(1).unwrap_or(&"").to_string()));
                    in_table = true;
                }
                _ => {}
            }
        }
        out.push(fp);
    }
    if out.is_empty() {
        return Err(format!("{DIR}: no fingerprints").into());
    }
    Ok(out)
}

const W: f64 = 300.0;
const H: f64 = 190.0;
const L: f64 = 44.0; // left margin
const R: f64 = 10.0;
const T: f64 = 22.0;
const B: f64 = 30.0;

/// A series: its legend label, CSS class, dashed or not, points.
pub(crate) struct Series {
    pub(crate) label: String,
    pub(crate) class: &'static str,
    pub(crate) dashed: bool,
    pub(crate) points: Vec<(f64, f64)>,
}

fn nice_step(span: f64) -> f64 {
    let raw = span / 4.0;
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p)
}

/// A line plot: `x_log` for frequency axes; `hline` a reference level.
pub(crate) fn plot(title: &str, x_label: &str, y_label: &str, x_log: bool, series: &[Series], hline: Option<(f64, &str)>) -> String {
    let pts = series.iter().flat_map(|s| s.points.iter());
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for &(x, y) in pts {
        if !x.is_finite() || !y.is_finite() || (x_log && x <= 0.0) {
            continue;
        }
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if let Some((h, _)) = hline {
        y0 = y0.min(h);
        y1 = y1.max(h);
    }
    if !x0.is_finite() {
        return String::new();
    }
    if (y1 - y0) < 1.0 {
        let m = 0.5 * (y0 + y1);
        y0 = m - 0.5;
        y1 = m + 0.5;
    }
    let step = nice_step(y1 - y0);
    y0 = (y0 / step).floor() * step;
    y1 = (y1 / step).ceil() * step;
    if x1 <= x0 {
        x1 = x0 + 1.0;
    }
    let fx = |x: f64| {
        let t = if x_log { (x.log10() - x0.log10()) / (x1.log10() - x0.log10()) } else { (x - x0) / (x1 - x0) };
        L + t * (W - L - R)
    };
    let fy = |y: f64| T + (1.0 - (y - y0) / (y1 - y0)) * (H - T - B);
    let mut s = format!(r#"<svg class="fp-plot" viewBox="0 0 {W} {H}" role="img" aria-label="{}"><text class="title" x="{L}" y="13">{}</text>"#, esc(title), esc(title));
    // Grid and ticks.
    let mut y = y0;
    while y <= y1 + 1e-9 {
        let py = fy(y);
        let _ = write!(s, r#"<line class="grid" x1="{L}" x2="{}" y1="{py:.1}" y2="{py:.1}"/><text class="tick" x="{}" y="{:.1}" text-anchor="end">{}</text>"#, W - R, L - 4.0, py + 3.5, fmt_tick(y));
        y += step;
    }
    if x_log {
        for d in [10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 1e4, 2e4] {
            if d >= x0 * 0.999 && d <= x1 * 1.001 {
                let px = fx(d);
                let _ = write!(s, r#"<line class="grid" x1="{px:.1}" x2="{px:.1}" y1="{T}" y2="{}"/><text class="tick" x="{px:.1}" y="{}" text-anchor="middle">{}</text>"#, H - B, H - B + 12.0, if d >= 1e3 { format!("{}k", d / 1e3) } else { format!("{d}") });
            }
        }
    } else {
        let xs = nice_step(x1 - x0);
        let mut x = (x0 / xs).ceil() * xs;
        while x <= x1 + 1e-9 {
            let px = fx(x);
            let _ = write!(s, r#"<line class="grid" x1="{px:.1}" x2="{px:.1}" y1="{T}" y2="{}"/><text class="tick" x="{px:.1}" y="{}" text-anchor="middle">{}</text>"#, H - B, H - B + 12.0, fmt_tick(x));
            x += xs;
        }
    }
    let _ = write!(s, r#"<text class="axis" x="{}" y="{}" text-anchor="end">{}</text>"#, W - R, H - 3.0, esc(x_label));
    let _ = write!(s, r#"<text class="axis" x="4" y="{}" transform="rotate(-90 4 {})" dy="8" text-anchor="middle">{}</text>"#, (T + H - B) / 2.0, (T + H - B) / 2.0, esc(y_label));
    if let Some((h, label)) = hline {
        let py = fy(h);
        let _ = write!(s, r#"<line class="limit" x1="{L}" x2="{}" y1="{py:.1}" y2="{py:.1}"/><text class="tick limit-text" x="{}" y="{:.1}" text-anchor="end">{}</text>"#, W - R, W - R - 2.0, py - 3.0, esc(label));
    }
    for se in series {
        let d: Vec<String> = se
            .points
            .iter()
            .filter(|(x, y)| x.is_finite() && y.is_finite() && (!x_log || *x > 0.0))
            .map(|&(x, y)| format!("{:.1},{:.1}", fx(x), fy(y.clamp(y0, y1))))
            .collect();
        if d.is_empty() {
            continue;
        }
        let dash = if se.dashed { r#" stroke-dasharray="4 3""# } else { "" };
        let _ = write!(s, r#"<polyline class="{}" points="{}"{dash}><title>{}</title></polyline>"#, se.class, d.join(" "), esc(&se.label));
        if d.len() <= 16 {
            for p in &d {
                let (cx, cy) = p.split_once(',').unwrap();
                let _ = write!(s, r#"<circle class="{}" cx="{cx}" cy="{cy}" r="2"/>"#, se.class);
            }
        }
    }
    s.push_str("</svg>");
    s
}

fn fmt_tick(v: f64) -> String {
    if v.abs() >= 100.0 || v == v.round() {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn rate_label(series: &str, fp: &Fingerprint) -> String {
    if series == "reference" {
        return "reference".into();
    }
    let role = fp.tables.iter().find(|(r, _)| r == series).map_or("", |(_, role)| role.as_str());
    let khz = series.parse::<f64>().map_or(series.to_string(), |r| format!("{} kHz", r / 1e3));
    match role {
        "oversampled" => format!("model, {khz} (oversampled)"),
        _ => format!("model, {khz}"),
    }
}

fn class_of(series: &str, fp: &Fingerprint) -> (&'static str, bool) {
    if series == "reference" {
        ("ref", true)
    } else if Some(series) == fp.shipping() {
        ("ship", false)
    } else {
        ("os", false)
    }
}

/// The series of `kind` grouped by series name, `(x, y)` from the values
/// at `xi`, `yi`, keeping only rows where `keep` holds.
fn series_of(fp: &Fingerprint, kind: &str, xi: usize, yi: usize, keep: impl Fn(&[f64]) -> bool) -> Vec<Series> {
    let mut by: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for (name, v) in fp.plots.get(kind).into_iter().flatten() {
        if v.len() > xi.max(yi) && keep(v) {
            by.entry(name.clone()).or_default().push((v[xi], v[yi]));
        }
    }
    let mut out: Vec<Series> = by
        .into_iter()
        .map(|(name, mut points)| {
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (class, dashed) = class_of(&name, fp);
            Series {
                label: rate_label(&name, fp),
                class,
                dashed,
                points,
            }
        })
        .collect();
    // The reference first, so the model draws over it.
    out.sort_by_key(|s| match s.class {
        "ref" => 0,
        "os" => 1,
        _ => 2,
    });
    out
}

/// One fingerprint's section: its plots and metrics.
pub fn section(fp: &Fingerprint) -> String {
    let mut s = format!(r#"<section class="fingerprint" id="{}"><h3><code>{}</code></h3><p>{} · reference: {} · playback {} (dB SPL, full scale) · <a href="{GITHUB}/blob/main/{}">fingerprint</a></p>"#, fp.anchor(), esc(&fp.case), esc(&fp.model), esc(&fp.reference), esc(&fp.playback), esc(&fp.file));
    if fp.plots.is_empty() {
        s.push_str("<p>Load steps only (a supply): its B+, ripple and recovery are in the file, with nothing here to plot.</p></section>");
        return s;
    }
    s.push_str(r#"<div class="fp-plots">"#);
    s.push_str(&plot("Small-signal response", "Hz", "dB", true, &series_of(fp, "response", 0, 1, |_| true), None));
    // Harmonics: the hottest level's spectrum and the knee's, model against reference.
    let levels: Vec<f64> = {
        let mut l: Vec<f64> = fp.plots.get("harmonics").into_iter().flatten().map(|(_, v)| v[0]).collect();
        l.sort_by(f64::total_cmp);
        l.dedup();
        l
    };
    let mut harm = Vec::new();
    for (i, &level) in levels.iter().rev().take(2).enumerate() {
        for mut se in series_of(fp, "harmonics", 1, 2, |v| v[0] == level) {
            if se.class == "os" {
                continue;
            }
            se.label = format!("{} at {level:+} dB re knee", se.label);
            if i == 1 {
                se.class = if se.class == "ref" { "ref faint" } else { "ship faint" };
            }
            harm.push(se);
        }
    }
    s.push_str(&plot("Harmonics H2–H15", "harmonic", "dBc", false, &harm, None));
    s.push_str(&plot("Compression (1 kHz)", "dB re knee", "gain dB", false, &series_of(fp, "compression", 0, 1, |_| true), None));
    let mut alias = series_of(fp, "anmr", 0, 1, |_| true);
    for se in &mut alias {
        se.label = format!("ANMR, {}", se.label);
    }
    for mut se in series_of(fp, "aliasing", 0, 1, |_| true) {
        se.label = format!("inharmonic floor (dBc), {}", se.label);
        se.dashed = true;
        alias.push(se);
    }
    s.push_str(&plot("Aliasing (1 kHz)", "dB re knee", "dB", false, &alias, Some((-10.0, "ANMR −10 dB: audible above"))));
    s.push_str("</div>");
    // The metrics at the shipping rate, a row per run.
    if let Some(rate) = fp.shipping() {
        let names = [
            ("nmr_total_db", "NMR total"),
            ("nmr_max_db", "NMR worst band"),
            ("erb_max_db", "ERB error max"),
            ("rnonlin", "Rnonlin"),
            ("gedlee", "GedLee G<sub>m</sub>"),
        ];
        let runs: Vec<&String> = {
            let mut r: Vec<&String> = fp.metrics.keys().filter(|k| k.0 == rate).map(|k| &k.1).collect();
            r.dedup();
            r
        };
        if !runs.is_empty() {
            let _ = write!(s, r#"<table class="fp-metrics"><thead><tr><th>run, {} kHz</th>"#, rate.parse::<f64>().map_or(0.0, |r| r / 1e3));
            for (_, label) in names {
                let _ = write!(s, "<th>{label}</th>");
            }
            s.push_str("</tr></thead><tbody>");
            for run in runs {
                let _ = write!(s, "<tr><td><code>{}</code></td>", esc(run));
                for (key, _) in names {
                    match fp.metrics.get(&(rate.to_string(), run.clone(), key.to_string())) {
                        Some(v) if key.ends_with("_db") => {
                            let _ = write!(s, "<td>{v:.1} dB</td>");
                        }
                        Some(v) => {
                            let _ = write!(s, "<td>{v:.4}</td>");
                        }
                        None => s.push_str("<td>—</td>"),
                    }
                }
                s.push_str("</tr>");
            }
            s.push_str("</tbody></table>");
        }
    }
    let _ = write!(s, r#"<p class="fp-versions">metrics: <code>{}</code></p></section>"#, esc(&fp.versions));
    s
}
