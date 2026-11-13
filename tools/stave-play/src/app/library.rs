//! Where plugins and projects live on disk.

use std::path::{Path, PathBuf};

/// The user's VST3 folder (when HOME is set) and the system's.
pub fn vst3_dirs() -> Vec<PathBuf> {
    let user = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(USER_VST3));
    user.into_iter()
        .chain([PathBuf::from(SYSTEM_VST3)])
        .collect()
}

const USER_VST3: &str = "Library/Audio/Plug-Ins/VST3";
const SYSTEM_VST3: &str = "/Library/Audio/Plug-Ins/VST3";

/// Every `.vst3` bundle directly inside `dirs`, sorted by name
/// (case-insensitive). Missing directories are skipped.
pub fn scan_vst3(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter(|p| is_vst3(p))
        .collect();
    out.sort_by_key(|p| display_name(p).to_lowercase());
    out
}

pub fn is_vst3(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "vst3")
}

/// Every catalog stage, as the `stage:<name>` path that opens it.
pub fn stages() -> impl Iterator<Item = PathBuf> {
    stave_host::stages::CATALOG
        .iter()
        .map(|m| PathBuf::from(format!("{}{}", stave_host::stages::PREFIX, m.name)))
}

/// "Foo" for ".../Foo.vst3"; the whole `stage:<name>` for a catalog
/// stage; `au:<name>` for an Audio Unit (or its `.component`).
pub fn display_name(p: &Path) -> String {
    if stave_host::opened::stage_name(p).is_some() {
        return p.display().to_string();
    }
    if let Some(q) = stave_host::opened::au_query(p) {
        return format!("au:{q}");
    }
    p.file_stem()
        .map_or_else(|| "?".into(), |s| s.to_string_lossy().into_owned())
}

/// The plugin a fresh project opens.
pub fn default_plugin() -> PathBuf {
    vst3_dirs().remove(0).join("StaveTilt.vst3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_sorted_vst3_bundles_only() {
        let root = std::env::temp_dir().join(format!("stave-play-scan-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        for (dir, name) in [
            (&a, "zeta.vst3"),
            (&a, "Alpha.vst3"),
            (&b, "mid.vst3"),
            (&b, "x.clap"),
        ] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let found = scan_vst3(&[a, b, root.join("missing")]);
        let names: Vec<_> = found.iter().map(|p| display_name(p)).collect();
        assert_eq!(names, ["Alpha", "mid", "zeta"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stages_are_listed_by_their_full_name() {
        let first = stages().next().unwrap();
        assert_eq!(display_name(&first), "stage:filter/peak");
    }
}
