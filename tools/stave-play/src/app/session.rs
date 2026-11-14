//! One live plugin or catalog stage: loaded, playing, with its editor and
//! handles. The fields are in teardown order — the editor (which binds
//! the live instance) detaches first, then the stream drops the
//! instance it owns — so "zero the telemetry pointer before dropping the
//! stream" is a property of the type, not a comment every caller must
//! honor.

use std::path::Path;
use std::sync::atomic::Ordering;

use stave_telemetry::Telemetry;
use stave_host::config::HostConfig;
use stave_host::param::{Param, ParamError};
use stave_host::instance::Instance;
use stave_host::opened::{OpenError, Opened};
use stave_host::stages::CatalogHandle;
use stave_host::plugin::{ControllerHandle, LoadedPlugin};

use crate::audio::{self, AudioError, AudioStream, Shared};
use crate::editor_view::EditorView;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Load(#[from] OpenError),
    #[error("could not start audio")]
    Audio(#[from] AudioError),
    /// Audio Units are measured and auditioned offline (the Voicing tab,
    /// `--characterise`); the live session hosts VST3 and stages.
    #[cfg(target_os = "macos")]
    #[error("{0}: an Audio Unit plays in Listen (Voicing) and --characterise, not live")]
    OfflineOnly(String),
}

enum ParamSource {
    Plugin(Option<ControllerHandle>),
    Stage(CatalogHandle),
}

pub struct Session {
    editor: Option<EditorView>,
    params: ParamSource,
    /// Owned by the plugin inside `stream`; valid while `stream` lives.
    telemetry: *const Telemetry,
    _stream: AudioStream,
    pub name: String,
    pub inputs: usize,
    pub outputs: usize,
}

impl Session {
    /// Open `path` (a bundle or `stage:<name>`), start it on the
    /// default device, and attach its editor under `ns_view` (null: no
    /// editor).
    pub fn start(
        path: &Path,
        config: HostConfig,
        shared: Shared,
        ns_view: *mut std::ffi::c_void,
    ) -> Result<Self, SessionError> {
        let loaded = Opened::open(path, config)?;
        let name = loaded.name();
        match loaded {
            Opened::Plugin(plugin) => Self::start_plugin(*plugin, name, shared, ns_view),
            #[cfg(target_os = "macos")]
            Opened::AudioUnit(_) => Err(SessionError::OfflineOnly(name)),
            Opened::Stage(stage) => {
                let params = ParamSource::Stage(stage.handle());
                let (inputs, outputs) = (stage.layout().inputs, stage.layout().outputs);
                let stream = audio::start(stage, shared)?;
                Ok(Self {
                    editor: None,
                    params,
                    telemetry: std::ptr::null(),
                    _stream: stream,
                    name,
                    inputs,
                    outputs,
                })
            }
        }
    }

    fn start_plugin(
        plugin: LoadedPlugin,
        name: String,
        shared: Shared,
        ns_view: *mut std::ffi::c_void,
    ) -> Result<Self, SessionError> {
        let log = shared.log.clone();
        log.plugin_latency
            .store(plugin.latency(), Ordering::Relaxed);
        for s in plugin.info().load.steps() {
            let line = format!("load: {} {} ({} µs)", s.step, s.detail, s.micros);
            if s.ok {
                log.info(line)
            } else {
                log.warn(line)
            }
        }
        let (inputs, outputs) = (plugin.layout().inputs, plugin.layout().outputs);
        let params = ParamSource::Plugin(plugin.controller_handle());
        let telemetry = plugin.telemetry_raw();
        let editor_owner = plugin.editor_owner_raw();
        let stream = audio::start(plugin, shared)?;
        // SAFETY: the controller lives inside `stream`, which outlives
        // the editor (field order).
        let editor = unsafe { EditorView::attach(editor_owner, ns_view) };
        if editor.is_none() {
            log.info("plugin has no editor view (or attach failed)");
        }
        Ok(Self {
            editor,
            params,
            telemetry,
            _stream: stream,
            name,
            inputs,
            outputs,
        })
    }

    /// The plugin's telemetry (stave plugins only).
    pub fn telemetry(&self) -> Option<&Telemetry> {
        // SAFETY: owned by the plugin `self._stream` keeps alive.
        unsafe { self.telemetry.as_ref() }
    }

    pub fn editor(&self) -> Option<&EditorView> {
        self.editor.as_ref()
    }

    pub fn params(&self) -> Vec<Param> {
        match &self.params {
            ParamSource::Plugin(c) => c.as_ref().map(|c| c.params()).unwrap_or_default(),
            ParamSource::Stage(m) => m.params(),
        }
    }

    /// Write a param: through the controller for a plugin (the real
    /// ABI path), through the stage's handle for a catalog stage.
    pub fn set(&self, name: &str, plain: f64) -> Result<(), ParamError> {
        match &self.params {
            ParamSource::Plugin(Some(c)) => c.set(name, plain),
            ParamSource::Plugin(None) => Err(ParamError::NotFound(name.into())),
            ParamSource::Stage(m) => m.set(name, plain),
        }
    }

    /// Write a param at a normalized position: its own taper.
    pub fn set_normalized(&self, name: &str, normalized: f64) -> Result<(), ParamError> {
        match &self.params {
            ParamSource::Plugin(Some(c)) => c.set_normalized(name, normalized),
            ParamSource::Plugin(None) => Err(ParamError::NotFound(name.into())),
            ParamSource::Stage(m) => m.set_normalized(name, normalized),
        }
    }

    /// A param's value at a normalized position, as its owner prints it.
    pub fn display(&self, name: &str, normalized: f64) -> String {
        match &self.params {
            ParamSource::Plugin(Some(c)) => c.display(name, normalized),
            ParamSource::Plugin(None) => None,
            ParamSource::Stage(m) => m.display(name, normalized),
        }
        .unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(ed) = self.editor.take() {
            // SAFETY: the instance it binds is still alive (stream not
            // yet dropped — it is the next field).
            unsafe { ed.detach() };
        }
        self.telemetry = std::ptr::null();
    }
}
