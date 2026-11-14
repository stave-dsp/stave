//! What the standalone remembers between runs, in the platform's
//! per-user settings folder (named by the plugin's id):
//!
//! ```text
//! macOS    ~/Library/Application Support/<id>/
//! Windows  %APPDATA%\<id>\
//! Linux    $XDG_CONFIG_HOME/<id>/   (default ~/.config/<id>/)
//! ```
//!
//! `settings.txt` holds the audio and MIDI choices as `key=value` lines
//! (hand-editable; an unknown or broken line is ignored); `state.bin`
//! is the plugin's state chunk, the same bytes a DAW session stores.
//! Both are written to a temporary file and renamed over the old one, so
//! a crash mid-write never leaves half a file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A device choice: the system default, or a device by name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    Default,
    Named(String),
}

/// Which MIDI inputs feed the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Midi {
    None,
    All,
    Named(String),
}

/// The audio and MIDI choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub output: Device,
    pub input: Device,
    /// Off until chosen: an effect's input on a laptop is the built-in
    /// microphone, and its output the speakers beside it.
    pub input_on: bool,
    /// Hz; 0 is the output device's own rate.
    pub sample_rate: u32,
    /// Frames per device buffer; 0 is the device's default.
    pub buffer: u32,
    pub midi: Midi,
}

impl Settings {
    /// The first-run choices: default devices, input off, MIDI from every
    /// input for a plugin that plays notes.
    pub fn first_run(takes_notes: bool) -> Self {
        Settings {
            output: Device::Default,
            input: Device::Default,
            input_on: false,
            sample_rate: 0,
            buffer: 0,
            midi: if takes_notes { Midi::All } else { Midi::None },
        }
    }

    /// Parse `key=value` lines over `self`.
    pub fn parse(mut self, text: &str) -> Self {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            let device = || if value.is_empty() { Device::Default } else { Device::Named(value.to_string()) };
            match key {
                "output" => self.output = device(),
                "input" => self.input = device(),
                "input_on" => self.input_on = value == "true",
                "sample_rate" => self.sample_rate = value.parse().unwrap_or(self.sample_rate),
                "buffer" => self.buffer = value.parse().unwrap_or(self.buffer),
                "midi" => {
                    self.midi = match value {
                        "" | "none" => Midi::None,
                        "all" => Midi::All,
                        name => Midi::Named(name.to_string()),
                    }
                }
                _ => {}
            }
        }
        self
    }

    /// The `key=value` text [`parse`](Self::parse) reads back.
    pub fn text(&self) -> String {
        let device = |d: &Device| match d {
            Device::Default => String::new(),
            Device::Named(n) => n.clone(),
        };
        let midi = match &self.midi {
            Midi::None => "none".to_string(),
            Midi::All => "all".to_string(),
            Midi::Named(n) => n.clone(),
        };
        format!(
            "output={}\ninput={}\ninput_on={}\nsample_rate={}\nbuffer={}\nmidi={}\n",
            device(&self.output),
            device(&self.input),
            self.input_on,
            self.sample_rate,
            self.buffer,
            midi
        )
    }
}

/// The settings folder and its two files.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The folder for the plugin whose id is `id`, under the platform's
    /// per-user settings root (created on first write).
    pub fn for_plugin(id: &str) -> Option<Self> {
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
        Some(Store { dir: config_root()?.join(safe) })
    }

    /// A store in `dir` (tests).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn settings(&self, takes_notes: bool) -> Settings {
        let first = Settings::first_run(takes_notes);
        match fs::read_to_string(self.dir.join("settings.txt")) {
            Ok(text) => first.parse(&text),
            Err(_) => first,
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        self.write("settings.txt", settings.text().as_bytes())
    }

    pub fn state(&self) -> Option<Vec<u8>> {
        fs::read(self.dir.join("state.bin")).ok()
    }

    pub fn save_state(&self, bytes: &[u8]) -> io::Result<()> {
        self.write("state.bin", bytes)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{name}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, self.dir.join(name))
    }
}

/// The platform's per-user settings root.
fn config_root() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_their_text() {
        let s = Settings {
            output: Device::Named("MacBook Pro Speakers".into()),
            input: Device::Default,
            input_on: true,
            sample_rate: 48_000,
            buffer: 256,
            midi: Midi::Named("Keystep 37".into()),
        };
        assert_eq!(Settings::first_run(false).parse(&s.text()), s);
    }

    #[test]
    fn broken_lines_keep_the_first_run_choices() {
        let s = Settings::first_run(true).parse("sample_rate=fast\nnonsense\nbuffer=128\nwhat=ever\n");
        assert_eq!(s, Settings { buffer: 128, ..Settings::first_run(true) });
        assert_eq!(Settings::first_run(true).midi, Midi::All);
        assert_eq!(Settings::first_run(false).midi, Midi::None);
    }

    #[test]
    fn the_store_writes_and_reads_back_both_files() {
        let dir = std::env::temp_dir().join(format!("stave-standalone-store-{}", std::process::id()));
        let store = Store::at(&dir);
        assert_eq!(store.state(), None);
        let s = Settings { buffer: 64, ..Settings::first_run(false) };
        store.save_settings(&s).unwrap();
        store.save_state(&[1, 2, 3]).unwrap();
        assert_eq!(store.settings(false), s);
        assert_eq!(store.state(), Some(vec![1, 2, 3]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_plugin_id_names_one_safe_folder() {
        let store = Store::for_plugin("com.example/../tilt").unwrap();
        assert!(store.dir().ends_with("com.example_.._tilt"));
    }
}
