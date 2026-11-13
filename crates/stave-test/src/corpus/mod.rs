//! The reference corpus (DSP.md R22): program material by role, from the
//! sample library `STAVE_SAMPLE_LIBRARY` names, picked by the committed
//! rules in `crates/stave-test/corpus.spec` and pinned by a local lock of
//! checksums. The audio is never committed; without the library (CI) a
//! check gets a [`Skip`] that says why, prints it and passes.
//!
//! ```text
//! spec.rs     the committed roles and selection rules
//! library.rs  _manifest.csv, _corpus.json, directories, WAV headers
//! c13.rs      C13: a null test on program material, judged by NMR
//! rate.rs     rate conversion for material played at another rate
//! ```
//!
//! A run resolves a role through the lock when the lock was taken under
//! the role's current rules (`scripts/corpus.sh lock` writes it), so it
//! plays exactly the files the lock names and fails on checksum drift;
//! otherwise it resolves the rules live against the library and says so.

pub mod c13;
pub mod library;
pub mod rate;
pub mod spec;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use spec::{Role, Spec};

use crate::sha256;

/// Why a role has nothing to play: the library is absent, or the role
/// has no material in it. Tests print it and pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Skip(pub String);

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One library file an item plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    /// Library-relative, `/`-separated.
    pub path: String,
    /// From the lock: SHA-256, size and modification time (seconds).
    pub locked: Option<Locked>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Locked {
    pub sha256: String,
    pub bytes: u64,
    pub mtime: u64,
}

/// One item of a role: a file, or files summed (`combine`).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub role: String,
    pub name: String,
    pub files: Vec<Pick>,
    pub excerpt: Option<(f64, f64)>,
}

/// An item's audio.
#[derive(Clone, Debug)]
pub struct Material {
    pub role: String,
    pub name: String,
    pub rate: u32,
    /// One or two channels, equal length.
    pub channels: Vec<Vec<f32>>,
    /// Each file and its SHA-256 (`None`: unlocked and not hashed).
    pub files: Vec<(String, Option<String>)>,
}

impl Material {
    pub fn mono(&self) -> Vec<f32> {
        let n = self.channels.first().map_or(0, Vec::len);
        let k = self.channels.len().max(1) as f32;
        (0..n).map(|i| self.channels.iter().map(|c| c[i]).sum::<f32>() / k).collect()
    }

    pub fn seconds(&self) -> f64 {
        self.channels.first().map_or(0, Vec::len) as f64 / f64::from(self.rate.max(1))
    }

    /// The channels at `rate` (converted if the material's differs).
    pub fn at_rate(&self, rate: f64) -> Vec<Vec<f32>> {
        let from = f64::from(self.rate);
        if (from - rate).abs() <= 0.5 {
            return self.channels.clone();
        }
        self.channels.iter().map(|c| rate::convert_rate(c, from, rate)).collect()
    }

    /// `role/name` and the files' checksums, for a log line.
    pub fn describe(&self) -> String {
        let sums: Vec<String> = self
            .files
            .iter()
            .map(|(_, s)| s.as_deref().map_or_else(|| "unlocked".into(), |s| s[..12].to_string()))
            .collect();
        let sums = match sums.len() {
            0..=3 => sums.join(" "),
            n => format!("{} … ({n} files summed)", sums[..2].join(" ")),
        };
        format!("{}/{} ({:.1} s at {} Hz; sha256 {sums})", self.role, self.name, self.seconds(), self.rate)
    }
}

/// The library, its index files and the local lock.
pub struct Corpus {
    pub root: PathBuf,
    pub spec: Spec,
    manifest: Option<Vec<library::Row>>,
    sets: Option<HashMap<String, Vec<String>>>,
    lock: Option<Lock>,
}

impl Corpus {
    /// The configured library ([`library_root`]) with the committed spec
    /// and the local lock ([`lock_path`]).
    pub fn open() -> Result<Self, Skip> {
        let root = library_root().ok_or_else(|| {
            Skip("skipped: library absent (STAVE_SAMPLE_LIBRARY is not set; see stave.env.example)".into())
        })?;
        if !root.is_dir() {
            return Err(Skip(format!("skipped: library absent ({} is not mounted)", root.display())));
        }
        let lock = lock_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Lock::parse(&t));
        Ok(Self::at(&root, Spec::committed(), lock))
    }

    /// A library at `root` under `spec`, with an optional lock.
    pub fn at(root: &Path, spec: Spec, lock: Option<Lock>) -> Self {
        Self {
            root: root.to_path_buf(),
            manifest: library::manifest(root),
            sets: library::sets(root),
            spec,
            lock,
        }
    }

    /// A role's items: from the lock when it was taken under the role's
    /// current rules, else resolved live. No items is a [`Skip`] naming
    /// the role and how to fill it.
    pub fn role(&self, name: &str) -> Result<Vec<Item>, Skip> {
        let role = self.spec.role(name).unwrap_or_else(|| panic!("no role {name} in corpus.spec"));
        let items = match self.lock.as_ref().and_then(|l| l.items(role)) {
            Some(items) => items,
            None => self.resolve(role),
        };
        if items.is_empty() {
            let fill = role.fill.as_deref().map_or_else(String::new, |f| format!("; to fill it: {f}"));
            return Err(Skip(format!("skipped: role {name} has no material in the library{fill}")));
        }
        Ok(items)
    }

    /// Whether the lock pins `role` (taken under its current rules).
    pub fn locked(&self, role: &Role) -> bool {
        self.lock.as_ref().is_some_and(|l| l.items(role).is_some())
    }

    /// A role's items from its rules alone, against the library as it is.
    pub fn resolve(&self, role: &Role) -> Vec<Item> {
        let mut files: Vec<String> = Vec::new();
        for s in &role.selects {
            let found: Vec<String> = match &s.source {
                spec::Source::File(p) => vec![p.clone()],
                spec::Source::Set(set) => match self.sets.as_ref().and_then(|m| m.get(set)) {
                    Some(list) => list.clone(),
                    None => library::walk(&self.root, &format!("Test Corpus/{set}")),
                },
                spec::Source::Dir(d) => library::walk(&self.root, d),
                spec::Source::Manifest(glob) => {
                    let rows = self.manifest.as_deref().unwrap_or_default();
                    library::from_manifest(rows, glob, &s.filters).map(|r| r.path.clone()).collect()
                }
            };
            let header_filters = !matches!(s.source, spec::Source::Manifest(_));
            for f in found {
                let ok = if header_filters {
                    library::admit_file(&self.root, &f, &s.filters)
                } else {
                    library::readable(&f) && library::absolute(&self.root, &f).is_file()
                };
                if ok && !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        let groups: Vec<(String, Vec<String>)> = match role.combine {
            Some(depth) => {
                let mut groups: Vec<(String, Vec<String>)> = Vec::new();
                for f in files {
                    let key = f.split('/').take(depth).collect::<Vec<_>>().join("/");
                    match groups.iter_mut().find(|g| g.0 == key) {
                        Some(g) => g.1.push(f),
                        None => groups.push((key, vec![f])),
                    }
                }
                groups
            }
            None => files.into_iter().map(|f| (f.clone(), vec![f])).collect(),
        };
        groups
            .into_iter()
            .take(role.count)
            .map(|(key, files)| Item {
                role: role.name.clone(),
                name: item_name(&key),
                files: files.into_iter().map(|path| Pick { path, locked: None }).collect(),
                excerpt: role.excerpt,
            })
            .collect()
    }

    /// An item's audio: each file read (its excerpt only), checked
    /// against the lock (size and time first; the SHA-256 when either
    /// moved), and summed at unity if there are several (mono files on
    /// both channels of a stereo sum).
    pub fn load(&self, item: &Item) -> Result<Material, String> {
        let mut rate = None;
        let mut channels: Vec<Vec<f32>> = Vec::new();
        let mut files = Vec::new();
        for pick in &item.files {
            let path = library::absolute(&self.root, &pick.path);
            if !library::readable(&pick.path) {
                return Err(format!("{}: not a WAV the corpus may read", pick.path));
            }
            let sha = verify(&path, pick)?;
            let info = stave_host::wav::read_info(&path).map_err(|e| format!("{}: {e}", pick.path))?;
            if *rate.get_or_insert(info.sample_rate) != info.sample_rate {
                return Err(format!("{}: {} Hz among files at {} Hz", pick.path, info.sample_rate, rate.unwrap_or(0)));
            }
            let (start, frames) = item.excerpt.map_or((0, usize::MAX), |(s, l)| {
                let r = f64::from(info.sample_rate);
                ((s * r) as usize, (l * r) as usize)
            });
            let audio = stave_host::wav::read_range(&path, start.min(info.frames.saturating_sub(1)), frames)
                .map_err(|e| format!("{}: {e}", pick.path))?;
            let mut a: Vec<Vec<f32>> = audio.channels.into_iter().take(2).collect();
            let width = a.len().max(channels.len());
            for set in [&mut channels, &mut a] {
                if set.len() == 1 && width == 2 {
                    set.push(set[0].clone());
                }
            }
            if channels.is_empty() {
                channels = a;
            } else {
                for (acc, add) in channels.iter_mut().zip(a) {
                    if add.len() > acc.len() {
                        acc.resize(add.len(), 0.0);
                    }
                    acc.iter_mut().zip(add).for_each(|(x, y)| *x += y);
                }
                let n = channels.iter().map(Vec::len).max().unwrap_or(0);
                channels.iter_mut().for_each(|c| c.resize(n, 0.0));
            }
            files.push((pick.path.clone(), sha));
        }
        if channels.first().is_none_or(Vec::is_empty) {
            return Err(format!("{}/{}: no audio", item.role, item.name));
        }
        Ok(Material {
            role: item.role.clone(),
            name: item.name.clone(),
            rate: rate.unwrap_or(0),
            channels,
            files,
        })
    }
}

/// The checksum a file's lock entry gives it, after checking the file
/// still is that file; `None` when unlocked.
fn verify(path: &Path, pick: &Pick) -> Result<Option<String>, String> {
    let Some(lock) = &pick.locked else { return Ok(None) };
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e} (locked, now missing)", pick.path))?;
    if meta.len() == lock.bytes && mtime(&meta) == lock.mtime {
        return Ok(Some(lock.sha256.clone()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", pick.path))?;
    let sha = sha256::hex(&bytes);
    if sha == lock.sha256 {
        Ok(Some(sha))
    } else {
        Err(format!(
            "corpus drift: {} is not the file the lock names (sha256 {} now, {} locked); \
             scripts/corpus.sh check shows every drift, scripts/corpus.sh lock accepts the library as it is",
            pick.path,
            &sha[..12],
            &lock.sha256[..12]
        ))
    }
}

pub(crate) fn mtime(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// An item's name: the last component of its path (no extension).
fn item_name(key: &str) -> String {
    let last = library::file_name(key);
    last.strip_suffix(".wav").or_else(|| last.strip_suffix(".WAV")).unwrap_or(last).to_string()
}

/// The local lock (`scripts/corpus.sh lock`): per role the digest of the
/// rules it was taken under, and each item's files with SHA-256, size and
/// modification time. Tab-separated lines:
///
/// ```text
/// role  NAME  DIGEST
/// item  NAME  INDEX  SHA256  BYTES  MTIME  PATH
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lock {
    roles: HashMap<String, String>,
    items: Vec<(String, usize, Pick)>,
}

impl Lock {
    pub fn parse(text: &str) -> Self {
        let mut lock = Self::default();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            match f[..] {
                ["role", name, digest] => {
                    lock.roles.insert(name.into(), digest.into());
                }
                ["item", name, index, sha, bytes, mtime, path] => {
                    let (Ok(index), Ok(bytes), Ok(mtime)) = (index.parse(), bytes.parse(), mtime.parse()) else { continue };
                    let locked = Locked { sha256: sha.into(), bytes, mtime };
                    lock.items.push((name.into(), index, Pick { path: path.into(), locked: Some(locked) }));
                }
                _ => {}
            }
        }
        lock
    }

    /// The role's locked items, if the lock was taken under its rules.
    pub fn items(&self, role: &Role) -> Option<Vec<Item>> {
        if self.roles.get(&role.name) != Some(&role.digest) {
            return None;
        }
        let mut items: Vec<Item> = Vec::new();
        for (name, index, pick) in self.items.iter().filter(|i| i.0 == role.name) {
            while items.len() <= *index {
                items.push(Item { role: name.clone(), name: String::new(), files: Vec::new(), excerpt: role.excerpt });
            }
            items[*index].files.push(pick.clone());
        }
        items.retain(|i| !i.files.is_empty());
        for item in &mut items {
            let key = match role.combine {
                Some(depth) => item.files[0].path.split('/').take(depth).collect::<Vec<_>>().join("/"),
                None => item.files[0].path.clone(),
            };
            item.name = item_name(&key);
        }
        Some(items)
    }
}

/// The sample library's root: `STAVE_SAMPLE_LIBRARY` from the
/// environment, else from the local configuration files the scripts
/// read (`.stave/local.env` in the checkout, `~/.config/stave/env`).
pub fn library_root() -> Option<PathBuf> {
    config("STAVE_SAMPLE_LIBRARY").map(PathBuf::from)
}

/// The lock's path: `STAVE_CORPUS_LOCK`, else `corpus.lock` beside this
/// machine's configuration (`${XDG_CONFIG_HOME:-~/.config}/stave/`).
pub fn lock_path() -> Option<PathBuf> {
    if let Some(p) = config("STAVE_CORPUS_LOCK") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("stave/corpus.lock"))
}

/// A configuration key, read as scripts/lib/config.sh reads it: the
/// environment wins, then `.stave/local.env`, then the machine's file.
pub fn config(key: &str) -> Option<String> {
    if let Some(v) = std::env::var_os(key) {
        return Some(v.to_string_lossy().into_owned()).filter(|v| !v.is_empty());
    }
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let files = [Some(PathBuf::from(".stave/local.env")), home.map(|h| h.join("stave/env"))];
    files
        .into_iter()
        .flatten()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .find_map(|text| config_value(&text, key))
}

/// `KEY=value` or `KEY="value"` from a shell-style config file (the
/// last assignment wins, as when the shell sources it).
pub fn config_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("export ").unwrap_or(line).strip_prefix(key)?.strip_prefix('=')?;
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            (!v.is_empty()).then(|| v.to_string())
        })
}

#[cfg(test)]
mod tests;
