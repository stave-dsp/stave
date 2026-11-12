//! One live plugin or module: loaded, playing, with its editor and
//! handles. The fields are in teardown order — the editor (which binds
//! the live instance) detaches first, then the stream drops the
//! instance it owns — so "zero the telemetry pointer before dropping the
//! stream" is a property of the type, not a comment every caller must
//! honor.

use std::path::Path;
use std::sync::atomic::Ordering;

use stave_telemetry::Telemetry;
use stave_host::config::HostConfig;
use stave_host::control::{Control, ControlError};
use stave_host::instance::Instance;
use stave_host::loaded::{LoadError, Loaded};
use stave_host::modules::ModuleHandle;
use stave_host::plugin::{ControllerHandle, LoadedPlugin};

use crate::audio::{self, AudioError, AudioStream, Shared};
use crate::editor_view::EditorView;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("could not start audio")]
    Audio(#[from] AudioError),
}

enum Controls {
    Plugin(Option<ControllerHandle>),
    Module(ModuleHandle),
}

pub struct Session {
    editor: Option<EditorView>,
    controls: Controls,
    /// Owned by the plugin inside `stream`; valid while `stream` lives.
    telemetry: *const Telemetry,
    _stream: AudioStream,
    pub name: String,
    pub outputs: usize,
}

impl Session {
    /// Open `path` (a bundle or `module:<name>`), start it on the
    /// default device, and attach its editor under `ns_view` (null: no
    /// editor).
    pub fn start(
        path: &Path,
        config: HostConfig,
        shared: Shared,
        ns_view: *mut std::ffi::c_void,
    ) -> Result<Self, SessionError> {
        let loaded = Loaded::open(path, config)?;
        let name = loaded.name();
        match loaded {
            Loaded::Plugin(plugin) => Self::start_plugin(*plugin, name, shared, ns_view),
            Loaded::Module(module) => {
                let controls = Controls::Module(module.handle());
                let outputs = module.layout().outputs;
                let stream = audio::start(module, shared)?;
                Ok(Self {
                    editor: None,
                    controls,
                    telemetry: std::ptr::null(),
                    _stream: stream,
                    name,
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
        let outputs = plugin.layout().outputs;
        let controls = Controls::Plugin(plugin.controller_handle());
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
            controls,
            telemetry,
            _stream: stream,
            name,
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

    pub fn controls(&self) -> Vec<Control> {
        match &self.controls {
            Controls::Plugin(c) => c.as_ref().map(|c| c.controls()).unwrap_or_default(),
            Controls::Module(m) => m.controls(),
        }
    }

    /// Write a control: through the controller for a plugin (the real
    /// ABI path), through the module's handle for a module.
    pub fn set(&self, name: &str, plain: f64) -> Result<(), ControlError> {
        match &self.controls {
            Controls::Plugin(Some(c)) => c.set(name, plain),
            Controls::Plugin(None) => Err(ControlError::NotFound(name.into())),
            Controls::Module(m) => m.set(name, plain),
        }
    }

    /// Write a control at a normalized position: its own taper.
    pub fn set_normalized(&self, name: &str, normalized: f64) -> Result<(), ControlError> {
        match &self.controls {
            Controls::Plugin(Some(c)) => c.set_normalized(name, normalized),
            Controls::Plugin(None) => Err(ControlError::NotFound(name.into())),
            Controls::Module(m) => m.set_normalized(name, normalized),
        }
    }

    /// A control's value at a normalized position, as its owner prints it.
    pub fn display(&self, name: &str, normalized: f64) -> String {
        match &self.controls {
            Controls::Plugin(Some(c)) => c.display(name, normalized),
            Controls::Plugin(None) => None,
            Controls::Module(m) => m.display(name, normalized),
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
