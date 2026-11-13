//! stave-play's app: the project, the live [`Session`], and the frame.
//!
//! ```text
//! mod.rs         state + the per-frame layout
//! session.rs     one live plugin (teardown order by construction)
//! library.rs     plugin + project locations on disk
//! panels.rs      top bar, project sidebar, params, status bar
//! central.rs     the view tabs and the active view
//! keyboard.rs    piano + sample pads; held-note bookkeeping
//! screenshot.rs  headless --screenshot capture
//! voicing.rs     the Voicing view's place in the app (device hand-over)
//! ```

mod central;
mod keyboard;
pub(crate) mod library;
mod panels;
mod screenshot;
mod session;
mod voicing;

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Instant;

use crossbeam_queue::ArrayQueue;
use eframe::egui;
use std::num::NonZeroUsize;

use stave_host::config::{HostConfig, SampleRate};

use crate::analysis::AnalysisRing;
use crate::audio::{self, Shared, MAX_EVENTS};
use crate::debug_log::DebugLog;
use crate::project::{self, Project, ViewId};
use crate::views;
use crate::voicing::{self as voicing_view, Voicing};

use keyboard::Keyboard;
pub use screenshot::CaptureRegion;
use screenshot::Screenshot;
use session::Session;

/// Per-view UI state (one per analysis tab).
#[derive(Default)]
struct Views {
    scope: views::scope::ScopeState,
    spectrum: views::spectrum::SpectrumState,
    meters: views::meters::MetersState,
    phase: views::phase::PhaseState,
    response: views::response::ResponseState,
    spectrogram: views::spectrogram::SpectrogramState,
    profiles: views::profiles::ProfilesState,
    validate: views::validate::ValidateState,
    perf: views::perf::PerfState,
}

pub struct HostApp {
    project: Project,
    project_path: Option<PathBuf>,
    project_dir: PathBuf,
    projects: Vec<(Project, PathBuf)>,
    dirty: bool,
    shared: Shared,
    session: Option<Session>,
    device_name: String,
    status: String,
    keyboard: Keyboard,
    views: Views,
    last_frame: Instant,
    /// The window's root NSView — the parent the plugin editor floats in.
    ns_view: *mut std::ffi::c_void,
    /// Where the Plugin tab reserved space this frame (egui points).
    editor_rect: egui::Rect,
    screenshot: Option<Screenshot>,
    /// `--set` pairs, applied through the controller once the plugin is up.
    pending_sets: Vec<(String, f64)>,
    voicing: Voicing,
    /// The main plugin was dropped so a comparison could take the device;
    /// leaving the Voicing view brings it back.
    suspended_for_voicing: bool,
}

/// What the command line hands the app.
pub struct Launch {
    pub plugin: Option<PathBuf>,
    pub sample_rate: SampleRate,
    pub block_size: NonZeroUsize,
    pub sets: Vec<(String, f64)>,
    pub screenshot: Option<PathBuf>,
    pub screenshot_note: Option<i16>,
    pub screenshot_region: CaptureRegion,
    pub voicing: voicing_view::Launch,
}

impl HostApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch, voicing: Voicing) -> Self {
        let project_dir = project::projects_dir();
        let projects = project::list_projects(&project_dir);
        let log = DebugLog::new();
        log.info(format!(
            "projects dir: {} ({} on disk)",
            project_dir.display(),
            projects.len()
        ));
        let plugin = launch.plugin.unwrap_or_else(library::default_plugin);
        let mut app = Self {
            project: Project::untitled(plugin, launch.sample_rate, launch.block_size),
            project_path: None,
            project_dir,
            projects,
            dirty: false,
            shared: Shared {
                events: Arc::new(ArrayQueue::new(MAX_EVENTS)),
                samples: Arc::new(ArrayQueue::new(8)),
                peak: Arc::new(AtomicU32::new(0)),
                output_tap: AnalysisRing::new(),
                input_tap: AnalysisRing::new(),
                log,
            },
            session: None,
            device_name: String::new(),
            status: String::new(),
            keyboard: Keyboard::new(),
            views: Views::default(),
            last_frame: Instant::now(),
            ns_view: screenshot::root_ns_view(cc),
            editor_rect: egui::Rect::NOTHING,
            screenshot: launch
                .screenshot
                .map(|p| Screenshot::new(p, launch.screenshot_note, launch.screenshot_region)),
            pending_sets: launch.sets,
            voicing,
            suspended_for_voicing: false,
        };
        if launch.voicing.wants_voicing() {
            // Straight to the comparison: no main plugin to load, but the
            // device still decides the rate the takes run at.
            app.project.active_view = ViewId::Voicing;
            app.suspended_for_voicing = true;
            match audio::pick_device_profile(app.project.sample_rate) {
                Ok(p) => {
                    app.project.sample_rate = p.sample_rate;
                    app.device_name = p.device_name;
                }
                Err(e) => app.fail(format!(
                    "audio device probe failed: {}",
                    crate::errors::with_causes(&e)
                )),
            }
        } else {
            app.reload();
        }
        app
    }

    /// Tear the current plugin down and bring the project's up.
    fn reload(&mut self) {
        self.voicing.release_device(); // one stream on the device at a time
        self.session = None; // Session::drop: editor first, then the plugin
        self.keyboard.release_all();
        let profile = match audio::pick_device_profile(self.project.sample_rate) {
            Ok(p) => p,
            Err(e) => {
                let e = crate::errors::with_causes(&e);
                return self.fail(format!("audio device probe failed: {e}"));
            }
        };
        // The device decides the rate; write it back so the picker never lies.
        self.project.sample_rate = profile.sample_rate;
        self.device_name = profile.device_name;
        let config = self.host_config();
        self.shared
            .log
            .info(format!("loading {}", self.project.plugin_path.display()));
        match Session::start(
            &self.project.plugin_path,
            config,
            self.shared.clone(),
            self.ns_view,
        ) {
            Ok(s) => {
                self.shared.log.info(format!("up: {}", s.name));
                self.session = Some(s);
                self.status = "ready".into();
            }
            Err(e) => self.fail(format!("load failed: {}", crate::errors::with_causes(&e))),
        }
    }

    fn fail(&mut self, msg: String) {
        self.shared.log.error(msg.clone());
        self.status = msg;
    }

    fn host_config(&self) -> HostConfig {
        HostConfig {
            sample_rate: self.project.sample_rate,
            max_block: self.project.block_size,
            ..HostConfig::default()
        }
    }

    fn apply_pending_sets(&mut self) {
        let Some(s) = &self.session else { return };
        for (name, plain) in std::mem::take(&mut self.pending_sets) {
            if let Err(e) = s.set(&name, plain) {
                self.shared.log.warn(format!("--set {name}: {e}"));
            }
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl eframe::App for HostApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(0.001, 0.25);
        self.last_frame = now;
        self.apply_pending_sets();
        self.tick_screenshot(ctx);
        // The Voicing view owns the letter and digit keys.
        let voicing = self.project.active_view == ViewId::Voicing;
        if !voicing {
            self.octave_shortcuts(ctx);
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.draw_topbar(ui));
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| self.draw_statusbar(ui));
        if !voicing {
            // Declared after `bottom`, so egui stacks it above the status row.
            egui::TopBottomPanel::bottom("keyboard")
                .resizable(false)
                .min_height(208.0)
                .show(ctx, |ui| self.draw_keyboard_panel(ui));
        }
        egui::SidePanel::left("sidebar")
            .default_width(170.0)
            .show(ctx, |ui| self.draw_sidebar(ui));
        if !voicing {
            egui::SidePanel::right("params")
                .resizable(true)
                .default_width(300.0)
                .min_width(220.0)
                .show(ctx, |ui| self.draw_params(ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_view_tabs(ui);
            self.draw_central(ui, dt);
        });

        if voicing {
            self.release_held_notes();
        } else {
            self.sync_held_notes(ctx);
        }
        self.place_editor(ctx);
        ctx.request_repaint();
    }
}
