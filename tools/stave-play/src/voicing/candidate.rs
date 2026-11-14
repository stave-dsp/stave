//! What is compared: a plugin or catalog stage with a preset and param
//! settings, the dry material (the bypass anchor), a recording aligned to
//! the material (a capture), or a blessed take (an earlier version's
//! sound, pinned by "bless this sound").
//!
//! On the command line a candidate is one string:
//!
//! ```text
//! dry
//! stage:clipper/diode;drive=0.6;label=diode hot
//! ~/Library/Audio/Plug-Ins/VST3/StaveTilt.vst3;preset=Bright
//! wav:capture.wav
//! blessed:~/…/blessed/tilt-bright-20261113T081500Z.json
//! ```
//!
//! Pieces after the first `;` are `name=value` param settings (plain
//! units, as `--set` takes them; a catalog stage's choice also by its
//! option's label, `oversample=4x`), `preset=NAME`, `label=TEXT`, or `ref`
//! (this candidate is the session's reference).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A param setting in the param's own units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Source {
    /// The material itself, unprocessed (the bypass anchor).
    Dry,
    /// A `.vst3` bundle or `stage:<name>`.
    Plugin { path: PathBuf },
    /// A WAV aligned to the material (a capture, or a take from elsewhere).
    Recording { path: PathBuf },
    /// A bless record (its take WAV, checked against the material).
    Blessed { record: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub label: String,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum SpecError {
    #[error("empty candidate")]
    Empty,
    #[error("'{0}': expected name=value, preset=NAME, label=TEXT or ref")]
    Piece(String),
    #[error("'{piece}': '{value}' is not a number")]
    Number { piece: String, value: String },
    #[error("{0} takes no settings or preset")]
    NotAPlugin(&'static str),
}

impl Candidate {
    pub fn dry() -> Self {
        Self {
            label: "dry".into(),
            source: Source::Dry,
            preset: None,
            settings: Vec::new(),
        }
    }

    pub fn plugin(path: PathBuf) -> Self {
        let mut c = Self {
            label: String::new(),
            source: Source::Plugin { path },
            preset: None,
            settings: Vec::new(),
        };
        c.label = c.default_label();
        c
    }

    pub fn recording(path: PathBuf) -> Self {
        let label = stem(&path);
        Self {
            label,
            source: Source::Recording { path },
            preset: None,
            settings: Vec::new(),
        }
    }

    pub fn blessed(record: PathBuf) -> Self {
        let label = format!("blessed {}", stem(&record));
        Self {
            label,
            source: Source::Blessed { record },
            preset: None,
            settings: Vec::new(),
        }
    }

    /// The name, preset and settings, briefly ("stage:clipper/diode
    /// drive=0.6").
    pub fn default_label(&self) -> String {
        let base = match &self.source {
            Source::Dry => return "dry".into(),
            Source::Plugin { path } => crate::app::library::display_name(path),
            Source::Recording { path } => stem(path),
            Source::Blessed { record } => format!("blessed {}", stem(record)),
        };
        let mut parts = vec![base];
        if let Some(p) = &self.preset {
            parts.push(format!("[{p}]"));
        }
        parts.extend(self.settings.iter().map(|s| format!("{}={}", s.name, trim_number(s.value))));
        parts.join(" ")
    }

    /// Whether processing it needs a plugin instance.
    pub fn is_plugin(&self) -> bool {
        matches!(self.source, Source::Plugin { .. })
    }

    /// Set (or replace) a setting.
    pub fn set(&mut self, name: &str, value: f64) {
        match self.settings.iter_mut().find(|s| s.name == name) {
            Some(s) => s.value = value,
            None => self.settings.push(Setting {
                name: name.into(),
                value,
            }),
        }
    }

    /// One command-line candidate; the flag says whether it was marked
    /// `ref`.
    pub fn parse(spec: &str) -> Result<(Self, bool), SpecError> {
        let mut pieces = spec.split(';').map(str::trim);
        let head = pieces.next().filter(|h| !h.is_empty()).ok_or(SpecError::Empty)?;
        let mut c = if head.eq_ignore_ascii_case("dry") || head.eq_ignore_ascii_case("bypass") {
            Self::dry()
        } else if let Some(p) = head.strip_prefix("wav:") {
            Self::recording(expand_home(p))
        } else if let Some(p) = head.strip_prefix("blessed:") {
            Self::blessed(expand_home(p))
        } else if head.starts_with(stave_host::stages::PREFIX) {
            Self::plugin(PathBuf::from(head))
        } else {
            Self::plugin(expand_home(head))
        };
        let (mut label, mut is_ref) = (None, false);
        for piece in pieces.filter(|p| !p.is_empty()) {
            if piece.eq_ignore_ascii_case("ref") {
                is_ref = true;
                continue;
            }
            let (k, v) = piece
                .split_once('=')
                .ok_or_else(|| SpecError::Piece(piece.into()))?;
            let (k, v) = (k.trim(), v.trim());
            match k {
                "label" => label = Some(v.to_string()),
                _ if !c.is_plugin() => {
                    return Err(SpecError::NotAPlugin(match c.source {
                        Source::Dry => "dry",
                        Source::Recording { .. } => "a recording",
                        _ => "a blessed take",
                    }))
                }
                "preset" => c.preset = Some(v.to_string()),
                _ => {
                    let value = v
                        .parse()
                        .ok()
                        .or_else(|| stage_option(head, k, v))
                        .ok_or_else(|| SpecError::Number {
                            piece: piece.into(),
                            value: v.into(),
                        })?;
                    c.set(k, value);
                }
            }
        }
        c.label = label.unwrap_or_else(|| c.default_label());
        Ok((c, is_ref))
    }
}

/// A catalog stage's choice setting `name` given by an option's label:
/// its index. Labels match ignoring case and everything but letters and
/// digits (`4x`, `ECC83_PHILIPS` for "ECC83 (Philips)", `jrc4558 table`);
/// failing an exact match, the one option whose label holds the text or
/// is held in it (`T12AX7` for "12AX7"), if only one does.
fn stage_option(head: &str, name: &str, label: &str) -> Option<f64> {
    use stave_core::params::Params;
    let spec = stave_host::stages::find(head.strip_prefix(stave_host::stages::PREFIX)?)?;
    let params = (spec.params)();
    let param = (0..params.count())
        .filter_map(|i| params.by_index(i))
        .find(|p| p.meta().name == name)?;
    let key = |s: &str| -> String { s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect() };
    let want = key(label);
    if want.is_empty() {
        return None;
    }
    let options: Vec<(u32, String)> = (0..param.step_count())
        .filter_map(|i| param.step_name(i).map(|n| (i, key(n))))
        .collect();
    if let Some((i, _)) = options.iter().find(|(_, n)| *n == want) {
        return Some(f64::from(*i));
    }
    let mut near = options.iter().filter(|(_, n)| !n.is_empty() && (n.contains(&want) || want.contains(n.as_str())));
    match (near.next(), near.next()) {
        (Some((i, _)), None) => Some(f64::from(*i)),
        _ => None,
    }
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map_or_else(|| p.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// `0.5`, `3`, `-12.25`: no trailing zeros.
pub fn trim_number(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// A leading `~/` is the home directory (a shell would have expanded it
/// outside quotes; specs are often quoted for their `;`).
pub fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_kind() {
        let (c, r) = Candidate::parse("stage:clipper/diode;drive=0.6;mix = 1;label=hot;ref").unwrap();
        assert!(r);
        assert_eq!(c.label, "hot");
        assert_eq!(c.source, Source::Plugin { path: "stage:clipper/diode".into() });
        assert_eq!(
            c.settings,
            [Setting { name: "drive".into(), value: 0.6 }, Setting { name: "mix".into(), value: 1.0 }]
        );
        let (c, r) = Candidate::parse("dry").unwrap();
        assert_eq!((c.source, r), (Source::Dry, false));
        let (c, _) = Candidate::parse("A.vst3;preset=Bright").unwrap();
        assert_eq!(c.preset.as_deref(), Some("Bright"));
        assert_eq!(c.label, "A [Bright]");
        let (c, _) = Candidate::parse("wav:/x/cap.wav").unwrap();
        assert_eq!(c.source, Source::Recording { path: "/x/cap.wav".into() });
        let (c, _) = Candidate::parse("blessed:/b/r.json").unwrap();
        assert_eq!(c.source, Source::Blessed { record: "/b/r.json".into() });
    }

    #[test]
    fn default_labels_name_the_settings() {
        let (c, _) = Candidate::parse("stage:coupler;hz=20").unwrap();
        assert_eq!(c.label, "stage:coupler hz=20");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Candidate::parse(""), Err(SpecError::Empty));
        assert_eq!(Candidate::parse("x.vst3;drive"), Err(SpecError::Piece("drive".into())));
        assert!(matches!(Candidate::parse("x.vst3;drive=hot"), Err(SpecError::Number { .. })));
        assert_eq!(Candidate::parse("dry;drive=1"), Err(SpecError::NotAPlugin("dry")));
    }

    #[test]
    fn choices_take_their_labels() {
        let value = |spec: &str| Candidate::parse(spec).unwrap().0.settings[0].value;
        assert_eq!(value("stage:circuit/ts808;oversample=4x"), 2.0);
        assert_eq!(value("stage:circuit/ts808;oversample=2"), 2.0);
        assert_eq!(value("stage:circuit/ts808;oversample=8X"), 3.0);
        assert_eq!(value("stage:circuit/ts808;opamp=JRC4558"), 0.0);
        assert_eq!(value("stage:circuit/ts808;opamp=JRC4558_TABLE"), 1.0);
        assert_eq!(value("stage:circuit/common_cathode;tube=ECC83_PHILIPS"), 8.0);
        assert_eq!(value("stage:circuit/common_cathode;tube=T12AX7"), 0.0);
        assert_eq!(value("stage:circuit/phase90;mode=frozen"), 1.0);
        // An unknown label, a float setting, an ambiguous label, a plugin.
        for spec in [
            "stage:circuit/ts808;oversample=3x",
            "stage:circuit/ts808;drive=hot",
            "stage:circuit/common_cathode;tube=12a",
            "x.vst3;oversample=4x",
        ] {
            assert!(matches!(Candidate::parse(spec), Err(SpecError::Number { .. })), "{spec}");
        }
    }

    #[test]
    fn numbers_print_short() {
        assert_eq!(trim_number(0.5), "0.5");
        assert_eq!(trim_number(3.0), "3");
        assert_eq!(trim_number(-0.0), "0");
    }
}
