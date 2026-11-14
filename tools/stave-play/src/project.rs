//! Project files. A "project" in stave-play is a tiny JSON document recording
//! which plugin to load and a few host preferences. Lives in
//! `~/Library/Application Support/stave-play/projects/<name>.json` on macOS,
//! `$XDG_DATA_HOME/stave-play/projects/` on Linux. Plugin state (presets)
//! aren't persisted yet — comes when stave-vst3 implements IUnitInfo / state
//! streams.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};
#[cfg(test)]
use stave_host::config::HostConfig;
use stave_host::config::{non_zero, SampleRate};

/// stave-play's block size when none is given: larger than the host
/// library's default, for glitch-free interactive playback.
pub const DEFAULT_BLOCK: NonZeroUsize = non_zero(1024);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub plugin_path: PathBuf,
    pub sample_rate: SampleRate,
    pub block_size: NonZeroUsize,
    /// Which analysis view was active when the project was last saved.
    #[serde(default)]
    pub active_view: ViewId,
    /// Default octave for the on-screen piano.
    #[serde(default = "default_octave")]
    pub octave: i32,
}

fn default_octave() -> i32 {
    4
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewId {
    #[default]
    Plugin,
    Profiles,
    Response,
    Spectrogram,
    Scope,
    Spectrum,
    Meters,
    Phase,
    Debugger,
    Validate,
    Perf,
    Debug,
    Voicing,
}

impl ViewId {
    pub const ALL: [ViewId; 13] = [
        ViewId::Plugin,
        ViewId::Voicing,
        ViewId::Profiles,
        ViewId::Response,
        ViewId::Spectrogram,
        ViewId::Scope,
        ViewId::Spectrum,
        ViewId::Meters,
        ViewId::Phase,
        ViewId::Debugger,
        ViewId::Validate,
        ViewId::Perf,
        ViewId::Debug,
    ];

    /// The tab label: what the view shows, not how it is built.
    pub fn label(self) -> &'static str {
        match self {
            ViewId::Plugin => "Plugin",
            ViewId::Profiles => "Profiles",
            ViewId::Response => "Response",
            ViewId::Spectrogram => "Spectrogram",
            ViewId::Scope => "Scope",
            ViewId::Spectrum => "Spectrum",
            ViewId::Meters => "Meters",
            ViewId::Phase => "Phase",
            ViewId::Debugger => "Telemetry",
            ViewId::Validate => "Validate",
            ViewId::Perf => "CPU",
            ViewId::Debug => "Log",
            ViewId::Voicing => "Voicing",
        }
    }

    /// The mode (the job) the view belongs to.
    pub fn mode(self) -> Mode {
        match self {
            ViewId::Plugin => Mode::Play,
            ViewId::Voicing => Mode::Listen,
            ViewId::Validate => Mode::Validate,
            ViewId::Debugger | ViewId::Debug => Mode::Debug,
            ViewId::Profiles
            | ViewId::Response
            | ViewId::Spectrogram
            | ViewId::Scope
            | ViewId::Spectrum
            | ViewId::Meters
            | ViewId::Phase
            | ViewId::Perf => Mode::Measure,
        }
    }

    /// A view by its label or a mode's name, any case (`--view`).
    pub fn parse(name: &str) -> Option<ViewId> {
        let name = name.to_ascii_lowercase();
        ViewId::ALL
            .into_iter()
            .find(|v| v.label().eq_ignore_ascii_case(&name) || format!("{v:?}").eq_ignore_ascii_case(&name))
            .or_else(|| Mode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(&name)).map(Mode::first))
    }
}

/// The five jobs the activity bar switches between. A mode owns one or
/// more views; with more than one they show as tabs in the header.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    /// Hear the plugin: its editor, the params, the keyboard.
    Play,
    /// Compare by ear: loudness-matched A/B, blind tests, null, bless.
    Listen,
    /// Read it: live analysers, offline profiles, CPU.
    Measure,
    /// Check it hosts correctly.
    Validate,
    /// Look inside: telemetry taps, the log.
    Debug,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Play, Mode::Listen, Mode::Measure, Mode::Validate, Mode::Debug];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Play => "Play",
            Mode::Listen => "Listen",
            Mode::Measure => "Measure",
            Mode::Validate => "Validate",
            Mode::Debug => "Debug",
        }
    }

    /// The mode's views, in tab order.
    pub fn views(self) -> &'static [ViewId] {
        match self {
            Mode::Play => &[ViewId::Plugin],
            Mode::Listen => &[ViewId::Voicing],
            Mode::Measure => &[
                ViewId::Response,
                ViewId::Profiles,
                ViewId::Spectrum,
                ViewId::Spectrogram,
                ViewId::Scope,
                ViewId::Meters,
                ViewId::Phase,
                ViewId::Perf,
            ],
            Mode::Validate => &[ViewId::Validate],
            Mode::Debug => &[ViewId::Debugger, ViewId::Debug],
        }
    }

    pub fn first(self) -> ViewId {
        self.views()[0]
    }

    /// One line for the activity bar's tooltip and the palette.
    pub fn about(self) -> &'static str {
        match self {
            Mode::Play => "the plugin, its params and the keyboard",
            Mode::Listen => "loudness-matched A/B, blind tests, null, bless",
            Mode::Measure => "response, profiles, spectrum, scope, meters, CPU",
            Mode::Validate => "host-conformance checks",
            Mode::Debug => "telemetry taps and the log",
        }
    }
}

impl Project {
    /// A new untitled project pointing at the bundled sine plugin (or the
    /// caller's chosen path).
    pub fn untitled(
        plugin_path: PathBuf,
        sample_rate: SampleRate,
        block_size: NonZeroUsize,
    ) -> Self {
        let name = crate::app::library::display_name(&plugin_path);
        Self {
            name,
            plugin_path,
            sample_rate,
            block_size,
            active_view: ViewId::Plugin,
            octave: 4,
        }
    }

    pub fn save(&self, dir: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.json", sanitise(&self.name)));
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&path, json)?;
        Ok(path)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(io::Error::other)
    }
}

/// Scan the project directory for `*.json` files. Returns `(Project, path)`
/// pairs; bad files are skipped silently — caller can log if it cares.
pub fn list_projects(dir: &Path) -> Vec<(Project, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(proj) = Project::load(&p) {
            out.push((proj, p));
        }
    }
    out.sort_by_key(|(p, _)| p.name.to_lowercase());
    out
}

pub fn projects_dir() -> PathBuf {
    if let Some(data) = dirs::data_dir() {
        return data.join("stave-play").join("projects");
    }
    PathBuf::from("./stave-play-projects")
}

fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stave-play-proj-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_load_round_trip_with_a_sanitised_file_name() {
        let dir = tmp("rt");
        let mut p = Project::untitled(
            PathBuf::from("/x/My Plug.vst3"),
            SampleRate::new(96_000.0).unwrap(),
            NonZeroUsize::new(256).unwrap(),
        );
        p.active_view = ViewId::Spectrum;
        let path = p.save(&dir).unwrap();
        assert_eq!(path.file_name().unwrap(), "My_Plug.json");
        let back = Project::load(&path).unwrap();
        assert_eq!(
            (back.name, back.sample_rate.hz(), back.active_view),
            (p.name, 96_000.0, ViewId::Spectrum)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn listing_skips_junk_and_sorts_by_name() {
        let dir = tmp("ls");
        for name in ["zeta", "Alpha"] {
            Project::untitled(
                PathBuf::from(format!("/{name}.vst3")),
                HostConfig::default().sample_rate,
                DEFAULT_BLOCK,
            )
            .save(&dir)
            .unwrap();
        }
        fs::write(dir.join("broken.json"), "{").unwrap();
        fs::write(dir.join("notes.txt"), "hi").unwrap();
        let names: Vec<String> = list_projects(&dir)
            .into_iter()
            .map(|(p, _)| p.name)
            .collect();
        assert_eq!(names, ["Alpha", "zeta"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_view_belongs_to_exactly_the_mode_that_lists_it() {
        for v in ViewId::ALL {
            let owners: Vec<Mode> = Mode::ALL.into_iter().filter(|m| m.views().contains(&v)).collect();
            assert_eq!(owners, [v.mode()], "{v:?}");
        }
    }

    #[test]
    fn views_parse_by_label_variant_or_mode() {
        assert_eq!(ViewId::parse("cpu"), Some(ViewId::Perf));
        assert_eq!(ViewId::parse("Perf"), Some(ViewId::Perf));
        assert_eq!(ViewId::parse("listen"), Some(ViewId::Voicing));
        assert_eq!(ViewId::parse("measure"), Some(ViewId::Response));
        assert_eq!(ViewId::parse("nope"), None);
    }

    #[test]
    fn old_files_get_defaults() {
        let p: Project = serde_json::from_str(
            r#"{"name":"n","plugin_path":"/p","sample_rate":1.0,"block_size":2}"#,
        )
        .unwrap();
        assert_eq!((p.active_view, p.octave), (ViewId::Plugin, 4));
    }

    #[test]
    fn a_file_with_an_impossible_rate_or_block_is_refused() {
        for json in [
            r#"{"name":"n","plugin_path":"/p","sample_rate":0.0,"block_size":2}"#,
            r#"{"name":"n","plugin_path":"/p","sample_rate":48000.0,"block_size":0}"#,
        ] {
            assert!(serde_json::from_str::<Project>(json).is_err(), "{json}");
        }
    }
}
