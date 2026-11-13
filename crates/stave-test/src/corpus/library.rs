//! Reading the sample library: `_manifest.csv`, `_corpus.json`, its
//! directories and WAV headers — never anything but WAV, never `._*`
//! AppleDouble sidecars (an exFAT drive written from macOS is full of
//! them), never an AIFF (Ableton's protected AIFF-C must not be decoded).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::spec::{glob, Filters};

/// One `_manifest.csv` row that could be read: a WAV, not a sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: String,
    pub category: String,
    pub kind: String,
    pub rate: u32,
    pub channels: u16,
    pub seconds: f64,
}

/// Whether the library may decode this path: a `.wav` whose name is not
/// an AppleDouble sidecar.
pub fn readable(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    !name.starts_with("._") && Path::new(name).extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"))
}

/// The file name of a library-relative path.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `_manifest.csv`'s readable rows, sorted by path.
pub fn manifest(root: &Path) -> Option<Vec<Row>> {
    let text = std::fs::read_to_string(root.join("_manifest.csv")).ok()?;
    let mut lines = text.lines();
    let header = csv_fields(lines.next()?);
    let col = |name: &str| header.iter().position(|h| h == name);
    let (p, c, k, f, r, ch, s) = (col("path")?, col("category")?, col("kind")?, col("format")?, col("rate")?, col("channels")?, col("seconds")?);
    let mut rows: Vec<Row> = lines
        .map(csv_fields)
        .filter(|v| v.len() == header.len() && v[f].eq_ignore_ascii_case("wav") && readable(&v[p]))
        .map(|v| Row {
            path: v[p].clone(),
            category: v[c].clone(),
            kind: v[k].clone(),
            rate: v[r].parse().unwrap_or(0),
            channels: v[ch].parse().unwrap_or(0),
            seconds: v[s].parse().unwrap_or(0.0),
        })
        .collect();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Some(rows)
}

/// The manifest rows a selection admits.
pub fn from_manifest<'a>(rows: &'a [Row], category: &str, f: &Filters) -> impl Iterator<Item = &'a Row> {
    let (category, f) = (category.to_string(), f.clone());
    rows.iter().filter(move |r| {
        glob(&category, &r.category)
            && f.kind.as_deref().is_none_or(|k| k == r.kind)
            && f.admit(file_name(&r.path), r.seconds, r.rate, r.channels)
    })
}

/// One CSV line's fields (RFC 4180 quoting).
pub fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    out.push(field);
    out
}

/// `_corpus.json`'s sets: each set's library-relative files, in order.
pub fn sets(root: &Path) -> Option<HashMap<String, Vec<String>>> {
    let text = std::fs::read_to_string(root.join("_corpus.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let map = v.as_object()?;
    Some(
        map.iter()
            .map(|(set, entries)| {
                let files = entries
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.get("file")?.as_str().map(String::from))
                    .collect();
                (set.clone(), files)
            })
            .collect(),
    )
}

/// Every readable WAV under `dir` (library-relative), sorted.
pub fn walk(root: &Path, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let rel = relative(rel);
                if readable(&rel) {
                    out.push(rel);
                }
            }
        }
    }
    out.sort();
    out
}

/// A relative path with `/` separators.
pub fn relative(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// The absolute path of a library-relative one.
pub fn absolute(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

/// Whether a library file passes `f` by its header (no filter: whether
/// it exists and is readable).
pub fn admit_file(root: &Path, rel: &str, f: &Filters) -> bool {
    if !readable(rel) {
        return false;
    }
    let path = absolute(root, rel);
    if *f == Filters::default() {
        return path.is_file();
    }
    stave_host::wav::read_info(&path).is_ok_and(|i| {
        let seconds = i.frames as f64 / f64::from(i.sample_rate.max(1));
        f.admit(file_name(rel), seconds, i.sample_rate, i.channels)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quoting() {
        assert_eq!(csv_fields(r#"a,"b, c","d ""e""",,f"#), ["a", "b, c", "d \"e\"", "", "f"]);
    }

    #[test]
    fn only_wavs_are_readable() {
        assert!(readable("Test Corpus/kicks/Kick 007.wav"));
        assert!(!readable("Test Corpus/kicks/._Kick 007.wav"));
        assert!(!readable("Drums/Snare LD.aif"));
        assert!(readable("x/A.WAV"));
    }

    #[test]
    fn a_manifest_is_read_and_filtered() {
        let dir = std::env::temp_dir().join(format!("stave-corpus-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_manifest.csv"),
            "path,category,source,kind,format,rate,channels,bits,seconds,bpm,key,size,mtime\n\
             \"S/b/BASS, CLEAN.wav\",S/b,Stems,other,wav,44100,1,24,158.6,,,1,1\n\
             S/b/._BASS.wav,S/b,Stems,other,wav,44100,1,24,158.6,,,1,1\n\
             S/b/BASS.aif,S/b,Stems,other,aif,44100,1,24,158.6,,,1,1\n\
             S/g/GTR.wav,S/g,Stems,other,wav,44100,2,24,3.0,,,1,1\n",
        )
        .unwrap();
        let rows = manifest(&dir).unwrap();
        assert_eq!(rows.len(), 2);
        let f = Filters { min_seconds: Some(10.0), ..Filters::default() };
        let picked: Vec<_> = from_manifest(&rows, "S/*", &f).map(|r| r.path.as_str()).collect();
        assert_eq!(picked, ["S/b/BASS, CLEAN.wav"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
