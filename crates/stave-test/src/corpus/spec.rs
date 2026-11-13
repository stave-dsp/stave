//! `corpus.spec`: the committed roles and their selection rules (the
//! format is described at the top of the file).

use crate::sha256;

/// Where a role looks.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// One library-relative file.
    File(String),
    /// A `_corpus.json` set (else `Test Corpus/NAME/`).
    Set(String),
    /// Every WAV under a library directory.
    Dir(String),
    /// `_manifest.csv` rows whose category matches the glob.
    Manifest(String),
}

/// The `; key=value` filters on a selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub min_seconds: Option<f64>,
    pub max_seconds: Option<f64>,
    pub min_rate: Option<u32>,
    pub channels: Option<u16>,
}

impl Filters {
    /// Whether a file of this name, length, rate and width passes
    /// (`kind` is checked by the manifest reader).
    pub fn admit(&self, name: &str, seconds: f64, rate: u32, channels: u16) -> bool {
        self.name.as_deref().is_none_or(|g| glob(g, name))
            && self.min_seconds.is_none_or(|m| seconds >= m)
            && self.max_seconds.is_none_or(|m| seconds <= m)
            && self.min_rate.is_none_or(|m| rate >= m)
            && self.channels.is_none_or(|c| channels == c)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub source: Source,
    pub filters: Filters,
}

/// One role.
#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    pub name: String,
    pub about: String,
    pub instrument: String,
    pub technique: String,
    pub used_by: Vec<String>,
    pub required: bool,
    pub selects: Vec<Select>,
    /// Files sharing their first `n` path components are one item.
    pub combine: Option<usize>,
    pub count: usize,
    /// Start and length, seconds.
    pub excerpt: Option<(f64, f64)>,
    pub fill: Option<String>,
    /// SHA-256 of the role's lines (trimmed, comments and blanks
    /// dropped, joined by `\n`): a lock taken under other rules is stale.
    pub digest: String,
}

/// The whole spec.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub roles: Vec<Role>,
}

/// The committed spec.
pub const TEXT: &str = include_str!("../../corpus.spec");

impl Spec {
    /// The committed spec, parsed.
    pub fn committed() -> Self {
        Self::parse(TEXT).unwrap_or_else(|e| panic!("crates/stave-test/corpus.spec: {e}"))
    }

    pub fn role(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.name == name)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut blocks: Vec<(usize, Vec<&str>)> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("role ") {
                blocks.push((n + 1, vec![line]));
            } else if raw.starts_with(char::is_whitespace) {
                blocks
                    .last_mut()
                    .ok_or(format!("line {}: a key before any role", n + 1))?
                    .1
                    .push(line);
            } else {
                return Err(format!("line {}: expected `role NAME` or an indented key", n + 1));
            }
        }
        let roles: Vec<Role> = blocks.iter().map(|(n, lines)| role(lines).map_err(|e| format!("role at line {n}: {e}"))).collect::<Result<_, _>>()?;
        for (i, r) in roles.iter().enumerate() {
            if roles[..i].iter().any(|o| o.name == r.name) {
                return Err(format!("role {} twice", r.name));
            }
        }
        Ok(Self { roles })
    }
}

fn role(lines: &[&str]) -> Result<Role, String> {
    let name = lines[0].strip_prefix("role ").unwrap_or_default().trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("role name `{name}`: letters, digits and _"));
    }
    let mut r = Role {
        name,
        about: String::new(),
        instrument: String::new(),
        technique: String::new(),
        used_by: Vec::new(),
        required: false,
        selects: Vec::new(),
        combine: None,
        count: 0,
        excerpt: None,
        fill: None,
        digest: sha256::hex(lines.join("\n").as_bytes()),
    };
    for line in &lines[1..] {
        let (key, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let value = value.trim();
        let number = |v: &str| v.parse::<f64>().map_err(|_| format!("{key}: not a number: {v}"));
        match key {
            "about" => r.about = value.into(),
            "instrument" => r.instrument = value.into(),
            "technique" => r.technique = value.into(),
            "used-by" => r.used_by = value.split_whitespace().map(String::from).collect(),
            "required" => {
                r.required = match value {
                    "yes" => true,
                    "no" => false,
                    v => return Err(format!("required: yes or no, not {v}")),
                }
            }
            "select" => r.selects.push(select(value)?),
            "combine" => {
                let depth = value.strip_prefix("sum ").ok_or("combine: `sum DEPTH`")?;
                r.combine = Some(number(depth.trim())? as usize);
            }
            "count" => r.count = number(value)? as usize,
            "excerpt" => {
                let v: Vec<f64> = value.split_whitespace().map(number).collect::<Result<_, _>>()?;
                let [start, len] = v[..] else {
                    return Err("excerpt: START SECONDS".into());
                };
                r.excerpt = Some((start, len));
            }
            "fill" => r.fill = Some(value.into()),
            other => return Err(format!("unknown key {other}")),
        }
    }
    if r.selects.is_empty() || r.count == 0 {
        return Err(format!("{}: needs a select and a count", r.name));
    }
    Ok(r)
}

fn select(value: &str) -> Result<Select, String> {
    let mut parts = value.split(';').map(str::trim);
    let head = parts.next().unwrap_or_default();
    let (kind, arg) = head.split_once(char::is_whitespace).ok_or(format!("select `{head}`: KIND ARG"))?;
    let arg = arg.trim().to_string();
    let source = match kind {
        "file" => Source::File(arg),
        "set" => Source::Set(arg),
        "dir" => Source::Dir(arg),
        "manifest" => Source::Manifest(arg),
        other => return Err(format!("select: file, set, dir or manifest, not {other}")),
    };
    let mut f = Filters::default();
    for p in parts.filter(|p| !p.is_empty()) {
        let (k, v) = p.split_once('=').ok_or(format!("filter `{p}`: key=value"))?;
        let n = |v: &str| v.parse::<f64>().map_err(|_| format!("{k}: not a number: {v}"));
        match k.trim() {
            "name" => f.name = Some(v.into()),
            "kind" => f.kind = Some(v.into()),
            "min-seconds" => f.min_seconds = Some(n(v)?),
            "max-seconds" => f.max_seconds = Some(n(v)?),
            "min-rate" => f.min_rate = Some(n(v)? as u32),
            "channels" => f.channels = Some(n(v)? as u16),
            other => return Err(format!("unknown filter {other}")),
        }
    }
    Ok(Select { source, filters: f })
}

/// `*` matches any run of characters (`/` included), `?` one.
pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_spec_parses() {
        let s = Spec::committed();
        assert!(s.roles.len() >= 10);
        for r in &s.roles {
            assert!(!r.about.is_empty() && !r.used_by.is_empty(), "{}", r.name);
            assert!(r.required || r.fill.is_some() || r.selects.iter().any(|s| matches!(s.source, Source::Manifest(_))), "{}", r.name);
        }
        for role in ["guitar_di", "bass_di", "drums_loop", "mix_bus", "vocal", "piano", "synth_pad", "transients"] {
            assert!(s.role(role).is_some(), "{role}");
        }
    }

    #[test]
    fn roles_parse_their_keys() {
        let s = Spec::parse(
            "# c\nrole a\n  about x\n  used-by C13 M4\n  required yes\n  select manifest Stems/*; name=*GTR*.wav; min-seconds=10\n  select set kicks\n  combine sum 2\n  count 3\n  excerpt 30 20\n",
        )
        .unwrap();
        let r = &s.roles[0];
        assert_eq!(r.used_by, ["C13", "M4"]);
        assert!(r.required);
        assert_eq!(r.selects[0].source, Source::Manifest("Stems/*".into()));
        assert_eq!(r.selects[0].filters.name.as_deref(), Some("*GTR*.wav"));
        assert_eq!(r.selects[0].filters.min_seconds, Some(10.0));
        assert_eq!(r.selects[1].source, Source::Set("kicks".into()));
        assert_eq!((r.combine, r.count, r.excerpt), (Some(2), 3, Some((30.0, 20.0))));
        // The digest is of the role's own lines (the same as scripts/corpus.py).
        let lines = ["role a", "about x", "used-by C13 M4", "required yes", "select manifest Stems/*; name=*GTR*.wav; min-seconds=10", "select set kicks", "combine sum 2", "count 3", "excerpt 30 20"];
        assert_eq!(r.digest, sha256::hex(lines.join("\n").as_bytes()));
        assert!(Spec::parse("role a\n  select set x\n").is_err(), "no count");
        assert!(Spec::parse("role a\n  frob 1\n").is_err());
        assert!(Spec::parse("  about x\n").is_err());
    }

    #[test]
    fn globs() {
        assert!(glob("Stems/*/BASS", "Stems/Song A/BASS"));
        assert!(glob("*GTR*.wav", "RY GTR CLEAN.wav"));
        assert!(!glob("*GTR*.wav", "BASS.wav"));
        assert!(glob("a?c", "abc") && !glob("a?c", "ac"));
        assert!(glob("*", "") && glob("F#*", "F# chop"));
    }
}
