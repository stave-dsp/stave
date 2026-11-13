//! Published curves as references (DSP.md R23): a manufacturer's graph,
//! digitised once, with its source cited, so a device model can be held
//! to the part where no hardware capture exists.
//!
//! A table is `datasheet/<name>.txt` under `crates/stave-test/`: the
//! R1 two-column format (`x y`, SI units) with a `#` header, and a
//! `# curve name=value ...` line starting each curve of a family (a
//! plate curve per grid voltage, an output curve per base current):
//!
//! ```text
//! # title: ECC83 anode characteristics, Ia against Va at several Vg.
//! # document: Philips, ECC83 data sheet
//! # url: https://frank.pocnet.net/sheets/030/e/ECC83.pdf
//! # figure: "Ia = f(Va)", page 5
//! # method: 300 dpi; Va calibrated on the 0 and 400 V gridlines ...
//! # x: plate voltage (V)
//! # y: plate current (A)
//! # scale-x: linear
//! # scale-y: linear
//! # full-scale-x: 400
//! # full-scale-y: 4e-3
//! # uncertainty-x: 4
//! # uncertainty-y: 0.06e-3
//! # conditions: none
//! # curve vg=0
//! 50 0.62e-3
//! 100 1.55e-3
//! ```
//!
//! Every key above is required: a table says where it came from, how it
//! was read, and how far the reading can be trusted. The uncertainties
//! are the largest digitising error — absolute for a linear axis, a
//! percentage (`5%`) for a log one — so a test's budget is the reading's
//! uncertainty plus the tolerance it grants the model, never a number
//! that happened to fit.

use std::path::{Path, PathBuf};

use super::{Axis, Reference, Source, FIXTURES};

/// How far a digitised value can be off.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Uncertainty {
    /// In the axis's SI unit (a linear axis).
    Absolute(f64),
    /// A fraction of the value (a log axis: `5%` is 0.05).
    Relative(f64),
}

impl Uncertainty {
    /// The uncertainty at `value`, in its unit.
    pub fn at(self, value: f64) -> f64 {
        match self {
            Uncertainty::Absolute(u) => u,
            Uncertainty::Relative(r) => r * value.abs(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        match text.strip_suffix('%') {
            Some(percent) => percent
                .trim()
                .parse::<f64>()
                .ok()
                .map(|p| Self::Relative(p / 100.0)),
            None => first_number(text).map(Self::Absolute),
        }
    }
}

/// One curve of a [`Datasheet`]: its parameters (`vg = −1.5`, `ib =
/// 100e-6`) and its points, as a [`Reference`] a [`Case`](super::Case)
/// compares against.
#[derive(Clone, Debug)]
pub struct Curve {
    /// The curve's line as written: `vg=-1.5`.
    pub label: String,
    pub parameters: Vec<(String, f64)>,
    pub reference: Reference,
}

impl Curve {
    /// The parameter `key`; panics, naming the curve, when it is absent.
    pub fn parameter(&self, key: &str) -> f64 {
        self.parameters
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("curve `{}` has no parameter `{key}`", self.label))
    }

    /// The parameter `key`, if the curve has it.
    pub fn get(&self, key: &str) -> Option<f64> {
        self.parameters
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
    }

    /// The curve read the other way, `x` as a function of `y` (a diode's
    /// forward voltage at each current read off an I–V graph), sorted by
    /// `y`, named `<curve> transposed`.
    pub fn transposed(&self) -> Reference {
        let r = &self.reference;
        let mut points: Vec<(f64, f64)> = r
            .values()
            .iter()
            .copied()
            .zip(r.abscissa().iter().copied())
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (abscissa, values) = points.into_iter().unzip();
        Reference::from_points(
            format!("{} transposed", r.name()),
            r.source().clone(),
            r.path.clone(),
            Axis::Sweep,
            abscissa,
            values,
        )
    }
}

/// A digitised datasheet table: its citation, its reading's
/// uncertainty, and its curves.
#[derive(Clone, Debug)]
pub struct Datasheet {
    name: String,
    path: PathBuf,
    header: Vec<(String, String)>,
    curves: Vec<Curve>,
}

/// Header keys every table must carry.
const REQUIRED: [&str; 15] = [
    "title",
    "document",
    "url",
    "figure",
    "method",
    "x",
    "y",
    "scale-x",
    "scale-y",
    "full-scale-x",
    "full-scale-y",
    "uncertainty-x",
    "uncertainty-y",
    "conditions",
    "notes",
];

impl Datasheet {
    /// `datasheet/<name>.txt`. Panics, naming the file and line, on a
    /// missing header key, a malformed line, or a curve with no points.
    pub fn load(name: &str) -> Self {
        let path = Path::new(FIXTURES).join(format!("datasheet/{name}.txt"));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Self::parse(name, path, &text).unwrap_or_else(|e| panic!("{e}"))
    }

    fn parse(name: &str, path: PathBuf, text: &str) -> Result<Self, String> {
        let at = |line: usize| format!("{}:{}", path.display(), line + 1);
        let mut header = Vec::new();
        let mut raw: Vec<Raw> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix('#') {
                let comment = comment.trim();
                if let Some(label) = comment.strip_prefix("curve ") {
                    let label = label.trim().to_string();
                    let parameters = parse_pairs(&label)
                        .ok_or_else(|| format!("{}: malformed curve line `{line}`", at(i)))?;
                    raw.push(Raw {
                        label,
                        parameters,
                        xs: vec![],
                        ys: vec![],
                    });
                } else if let Some((key, value)) = comment.split_once(':') {
                    let key = key.trim();
                    if !key.is_empty() && !key.contains(' ') {
                        header.push((key.to_string(), value.trim().to_string()));
                    }
                }
                continue;
            }
            let mut fields = line.split_whitespace().map(str::parse::<f64>);
            let (Some(Ok(x)), Some(Ok(y)), None) = (fields.next(), fields.next(), fields.next())
            else {
                return Err(format!("{}: malformed line `{line}`", at(i)));
            };
            let Some(curve) = raw.last_mut() else {
                return Err(format!("{}: a point before any `# curve` line", at(i)));
            };
            curve.xs.push(x);
            curve.ys.push(y);
        }
        let mut sheet = Self {
            name: name.to_string(),
            path: path.clone(),
            header,
            curves: vec![],
        };
        for key in REQUIRED {
            if sheet.get(key).is_none_or(str::is_empty) {
                return Err(format!("{}: no `# {key}:` header", path.display()));
            }
        }
        for key in ["full-scale-x", "full-scale-y"] {
            sheet
                .number(key)
                .ok_or_else(|| format!("{}: `{key}` is not a number", path.display()))?;
        }
        for key in ["uncertainty-x", "uncertainty-y"] {
            sheet.uncertainty(key).ok_or_else(|| {
                format!(
                    "{}: `{key}` is neither a number nor a percentage",
                    path.display()
                )
            })?;
        }
        if raw.is_empty() {
            return Err(format!("{}: no curves", path.display()));
        }
        let axis = sheet.axis();
        let source = Source::Datasheet {
            document: sheet.document().to_string(),
            figure: sheet.figure().to_string(),
        };
        for Raw {
            label,
            parameters,
            xs,
            ys,
        } in raw
        {
            if xs.is_empty() {
                return Err(format!("{}: curve `{label}` has no points", path.display()));
            }
            let reference = Reference::from_points(
                format!("{name}[{label}]"),
                source.clone(),
                path.clone(),
                axis,
                xs,
                ys,
            );
            sheet.curves.push(Curve {
                label,
                parameters,
                reference,
            });
        }
        Ok(sheet)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A header field, as written.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn require(&self, key: &str) -> &str {
        self.get(key).expect("checked when parsed")
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(first_number)
    }

    fn uncertainty(&self, key: &str) -> Option<Uncertainty> {
        self.get(key).and_then(Uncertainty::parse)
    }

    /// The document the figure is printed in.
    pub fn document(&self) -> &str {
        self.require("document")
    }

    /// The figure and page.
    pub fn figure(&self) -> &str {
        self.require("figure")
    }

    pub fn url(&self) -> &str {
        self.require("url")
    }

    /// The axis's printed full scale, SI.
    pub fn full_scale_x(&self) -> f64 {
        self.number("full-scale-x").expect("checked when parsed")
    }

    pub fn full_scale_y(&self) -> f64 {
        self.number("full-scale-y").expect("checked when parsed")
    }

    /// The largest digitising error of an abscissa.
    pub fn uncertainty_x(&self) -> Uncertainty {
        self.uncertainty("uncertainty-x")
            .expect("checked when parsed")
    }

    /// The largest digitising error of a value.
    pub fn uncertainty_y(&self) -> Uncertainty {
        self.uncertainty("uncertainty-y")
            .expect("checked when parsed")
    }

    /// A fixed operating condition of the chart (`vg2`, `t`), if stated.
    pub fn condition(&self, key: &str) -> Option<f64> {
        parse_pairs(self.require("conditions"))
            .unwrap_or_default()
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn curves(&self) -> &[Curve] {
        &self.curves
    }

    /// The curve whose parameters include every `key = value` given.
    pub fn curve(&self, wanted: &[(&str, f64)]) -> &Curve {
        self.curves
            .iter()
            .find(|c| wanted.iter().all(|(k, v)| c.get(k) == Some(*v)))
            .unwrap_or_else(|| panic!("{}: no curve {wanted:?}", self.path.display()))
    }

    /// Time for seconds, frequency for hertz, else a sweep: from the unit
    /// in the `x` header's last parentheses.
    fn axis(&self) -> Axis {
        let x = self.require("x");
        let unit = x
            .rsplit_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map(|(unit, _)| unit.trim());
        match unit {
            Some("s") => Axis::Time,
            Some("Hz") => Axis::Frequency,
            _ => Axis::Sweep,
        }
    }
}

/// A curve as parsed, before it becomes a [`Reference`].
struct Raw {
    label: String,
    parameters: Vec<(String, f64)>,
    xs: Vec<f64>,
    ys: Vec<f64>,
}

/// `name=value name=value ...` (SI numbers); `none` is no pairs.
fn parse_pairs(text: &str) -> Option<Vec<(String, f64)>> {
    if text.trim() == "none" {
        return Some(vec![]);
    }
    text.split_whitespace()
        .map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_string(), v.parse::<f64>().ok()?))
        })
        .collect()
}

/// The first whitespace-separated token of `text`, as a number (a header
/// value may carry a unit or a remark after it: `0.06e-3 A (1.5% FS)`).
fn first_number(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
# title: test
# document: Acme, TX1 data sheet
# url: https://example.com/tx1.pdf
# figure: Fig. 1, page 2
# method: by hand
# x: plate voltage (V)
# y: plate current (A)
# scale-x: linear
# scale-y: log
# full-scale-x: 400
# full-scale-y: 4e-3 A
# uncertainty-x: 4 V
# uncertainty-y: 5%
# conditions: vg2=250 t=25
# notes: none
# curve vg=0
0 0
100 1e-3
# curve vg=-1.5 vg2=200
100 0.5e-3
";

    #[test]
    fn parses_header_and_curves() {
        let sheet = Datasheet::parse("tx1", PathBuf::from("tx1.txt"), TABLE).unwrap();
        assert_eq!(sheet.curves().len(), 2);
        assert_eq!(sheet.full_scale_y(), 4e-3);
        assert_eq!(sheet.uncertainty_x(), Uncertainty::Absolute(4.0));
        assert_eq!(sheet.uncertainty_y(), Uncertainty::Relative(0.05));
        assert_eq!(sheet.condition("vg2"), Some(250.0));
        assert_eq!(sheet.condition("vf"), None);
        let c = sheet.curve(&[("vg", -1.5)]);
        assert_eq!(c.parameter("vg2"), 200.0);
        assert_eq!(c.reference.values(), [0.5e-3]);
        assert_eq!(c.reference.axis(), Axis::Sweep);
        assert!(matches!(c.reference.source(), Source::Datasheet { .. }));
        let t = sheet.curve(&[("vg", 0.0)]).transposed();
        assert_eq!(
            (t.abscissa(), t.values()),
            (&[0.0, 1e-3][..], &[0.0, 100.0][..])
        );
    }

    #[test]
    fn rejects_a_table_without_its_citation() {
        let text = TABLE.replace("# url: https://example.com/tx1.pdf\n", "");
        let e = Datasheet::parse("tx1", PathBuf::from("tx1.txt"), &text).unwrap_err();
        assert!(e.contains("`# url:`"), "{e}");
        let text = TABLE.replace("100 0.5e-3", "100 0.5e-3 7");
        assert!(Datasheet::parse("tx1", PathBuf::from("tx1.txt"), &text).is_err());
    }
}
