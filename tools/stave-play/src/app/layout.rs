//! The workbench layout: which regions are open, the theme, and each
//! mode's last view. Saved beside the projects (`layout.json`) whenever
//! it changes, so the window comes back the way it was left; a
//! `--screenshot` run starts from the defaults and saves nothing.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::theme::ThemeChoice;
use crate::project::{Mode, ViewId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The library side bar (plugins, projects): on demand.
    pub library: bool,
    /// The params inspector (in the modes that have one).
    pub inspector: bool,
    /// The bottom panel (keyboard, samples).
    pub panel: bool,
    pub theme: ThemeChoice,
    /// The view each mode shows when it is opened again.
    pub last_view: HashMap<Mode, ViewId>,
    /// Only the editor area (and the header and status bar).
    #[serde(skip)]
    pub focus: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            library: false,
            inspector: true,
            panel: true,
            theme: ThemeChoice::System,
            last_view: HashMap::new(),
            focus: false,
        }
    }
}

/// Which regions a frame draws: the layout's choices, narrowed by what
/// the mode has and by focus mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Regions {
    pub activity: bool,
    pub library: bool,
    pub inspector: bool,
    pub panel: bool,
}

impl Layout {
    pub fn regions(&self, mode: Mode) -> Regions {
        let open = !self.focus;
        Regions {
            activity: open,
            library: open && self.library,
            inspector: open && self.inspector && mode_has_inspector(mode),
            panel: open && self.panel && mode_has_panel(mode),
        }
    }

    /// The view `mode` opens on: the last one shown there, else its first.
    pub fn view_for(&self, mode: Mode) -> ViewId {
        self.last_view
            .get(&mode)
            .copied()
            .filter(|v| v.mode() == mode)
            .unwrap_or_else(|| mode.first())
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(io::Error::other)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        // Write then rename: a crash mid-write never leaves half a file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}

/// Params sit beside the plugin and beside what measures it; the other
/// modes use the full width.
pub fn mode_has_inspector(mode: Mode) -> bool {
    matches!(mode, Mode::Play | Mode::Measure)
}

/// Listening owns the keys (and plays its own takes): no keyboard there.
pub fn mode_has_panel(mode: Mode) -> bool {
    mode != Mode::Listen
}

pub fn path() -> PathBuf {
    let projects = crate::project::projects_dir();
    projects.parent().map_or_else(|| PathBuf::from("layout.json"), |d| d.join("layout.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_takes_the_whole_width_and_the_keys() {
        let l = Layout::default();
        let r = l.regions(Mode::Listen);
        assert!(r.activity && !r.inspector && !r.panel && !r.library);
        let r = l.regions(Mode::Play);
        assert!(r.inspector && r.panel && !r.library, "library opens on demand");
    }

    #[test]
    fn focus_mode_leaves_only_the_editor() {
        let l = Layout { focus: true, library: true, ..Layout::default() };
        let r = l.regions(Mode::Measure);
        assert_eq!(r, Regions { activity: false, library: false, inspector: false, panel: false });
    }

    #[test]
    fn a_mode_reopens_on_its_last_view() {
        let mut l = Layout::default();
        assert_eq!(l.view_for(Mode::Measure), ViewId::Response);
        l.last_view.insert(Mode::Measure, ViewId::Scope);
        assert_eq!(l.view_for(Mode::Measure), ViewId::Scope);
        // A stale entry naming another mode's view is ignored.
        l.last_view.insert(Mode::Debug, ViewId::Scope);
        assert_eq!(l.view_for(Mode::Debug), ViewId::Debugger);
    }

    #[test]
    fn round_trips_and_old_files_get_defaults() {
        let dir = std::env::temp_dir().join(format!("stave-play-layout-{}", std::process::id()));
        let path = dir.join("layout.json");
        let mut l = Layout { library: true, theme: ThemeChoice::Light, focus: true, ..Layout::default() };
        l.last_view.insert(Mode::Measure, ViewId::Phase);
        l.save(&path).unwrap();
        let back = Layout::load(&path).unwrap();
        assert_eq!(back, Layout { focus: false, ..l }, "focus is per session");
        std::fs::write(&path, r#"{"panel":false}"#).unwrap();
        assert_eq!(Layout::load(&path).unwrap(), Layout { panel: false, ..Layout::default() });
        std::fs::remove_dir_all(dir).unwrap();
    }
}
