//! stave-play's app: the project, the live [`Session`], and the
//! workbench around them, in VS Code's shape:
//!
//! ```text
//! +----+---------+------------------------------+-----------+
//! | A  | library | header: plugin · view tabs ⌕ |           |
//! | c  | (on     +------------------------------+ inspector |
//! | t  | demand) |  editor area: the active view| (params)  |
//! | i  |         +------------------------------+           |
//! | v  |         |  panel: keyboard | samples   |           |
//! +----+---------+------------------------------+-----------+
//! | status: device · rate · block · latency · CPU · peak     |
//! +---------------------------------------------------------+
//! ```
//!
//! ```text
//! mod.rs         state + the per-frame layout
//! commands.rs    command ids, sources, the keymap, fuzzy matching
//! workbench.rs   the workbench's own commands and what they do
//! palette.rs     the command palette
//! layout.rs      which regions are open (persisted)
//! theme.rs       the design system's tokens as egui visuals
//! shell.rs       activity bar, header, status bar
//! tools.rs       the shared command registry as a palette source; runs
//! output.rs      the Output panel: a registry run's result
//! panels.rs      library side bar, params inspector; plugin + project ops
//! central.rs     the editor area: the active view
//! keyboard.rs    piano strip + sample chips; held-note bookkeeping
//! session.rs     one live plugin (teardown order by construction)
//! library.rs     plugin + project locations on disk
//! screenshot.rs  headless --screenshot capture
//! voicing.rs     the Listen mode's place in the app (device hand-over)
//! ```

mod central;
mod commands;
mod keyboard;
mod layout;
pub(crate) mod library;
mod palette;
mod panels;
mod screenshot;
mod session;
mod shell;
pub(crate) mod theme;
mod tools;
mod output;
mod voicing;
mod workbench;

use std::cell::OnceCell;
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
use crate::project::{self, Mode, Project, ViewId};
use crate::views;
use crate::voicing::{self as voicing_view, Voicing};

use commands::{CommandSource, Keymap};
use keyboard::Keyboard;
use layout::Layout;
use palette::Palette;
pub use screenshot::CaptureRegion;
use screenshot::Screenshot;
use session::Session;
use theme::{ThemeChoice, Tokens};

/// Per-view UI state (one per analysis view).
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
    /// The status line reports a failure (drawn in the error colour).
    status_error: bool,
    keyboard: Keyboard,
    views: Views,
    last_frame: Instant,
    /// The window's root NSView — the parent the plugin editor floats in.
    ns_view: *mut std::ffi::c_void,
    /// Where the Plugin view reserved space this frame (egui points).
    editor_rect: egui::Rect,
    screenshot: Option<Screenshot>,
    /// `--set` pairs, applied through the controller once the plugin is up.
    pending_sets: Vec<(String, f64)>,
    voicing: Voicing,
    /// The main plugin was dropped so a comparison could take the device;
    /// leaving the Voicing view brings it back.
    suspended_for_voicing: bool,
    layout: Layout,
    /// Where the layout is saved and what it was last saved as; none
    /// for a screenshot run (it neither reads nor writes the user's).
    layout_store: Option<(PathBuf, Layout)>,
    palette: Palette,
    keymap: Keymap,
    tokens: Tokens,
    /// The theme last applied (dark or not), to re-dress only on change.
    applied_dark: Option<bool>,
    /// The library's plugin scan (VST3 folders + catalog stages).
    plugins: OnceCell<Vec<PathBuf>>,
    library_query: String,
    /// The shared command registry and its current / last run.
    tools: tools::Tools,
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
    /// `--view`: open on this view.
    pub view: Option<ViewId>,
    /// `--theme`: this session's theme (saved only if changed in the app).
    pub theme: Option<ThemeChoice>,
    /// `--ui`: regions to open or close at start.
    pub ui: Vec<UiFlag>,
    /// `--window WxH`: the window's inner size in points.
    pub window: Option<[f32; 2]>,
    /// `--command ID[=ARG]`: commands to run once the window is up, as
    /// the palette would (`tool:bench`, `workbench.view=Scope`).
    pub commands: Vec<String>,
}

/// One `--ui` item: a region opened or closed, a panel tab, the
/// palette or focus mode (any state can be screenshotted).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UiFlag {
    Library(bool),
    Inspector(bool),
    Panel(bool),
    Keyboard,
    Samples,
    Palette,
    Focus,
}

impl UiFlag {
    pub fn parse(s: &str) -> Option<Self> {
        let (on, name) = s.strip_prefix("no-").map_or((true, s), |n| (false, n));
        Some(match (name, on) {
            ("library", _) => UiFlag::Library(on),
            ("inspector", _) => UiFlag::Inspector(on),
            ("panel", _) => UiFlag::Panel(on),
            ("keyboard", true) => UiFlag::Keyboard,
            ("samples", true) => UiFlag::Samples,
            ("palette", true) => UiFlag::Palette,
            ("focus", true) => UiFlag::Focus,
            _ => return None,
        })
    }
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
        let layout_store = launch.screenshot.is_none().then(|| {
            let path = layout::path();
            let saved = match Layout::load(&path) {
                Ok(l) => l,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Layout::default(),
                Err(e) => {
                    log.warn(format!("layout {}: {e} (using the defaults)", path.display()));
                    Layout::default()
                }
            };
            (path, saved)
        });
        let mut layout = layout_store.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
        if let Some(t) = launch.theme {
            layout.theme = t;
        }
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
            status_error: false,
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
            layout,
            layout_store,
            palette: Palette::default(),
            keymap: Keymap::default(),
            tokens: Tokens::dark(),
            applied_dark: None,
            plugins: OnceCell::new(),
            library_query: String::new(),
            tools: tools::Tools::default(),
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
            app.project.active_view = launch.view.unwrap_or_else(|| app.layout.view_for(Mode::Play));
            app.reload();
        }
        for flag in launch.ui {
            app.apply_flag(flag);
        }
        for c in launch.commands {
            let inv = match c.split_once('=') {
                Some((id, arg)) => commands::Invocation::with(id, arg),
                None => commands::Invocation::new(&c),
            };
            app.run(&inv);
        }
        app
    }

    fn apply_flag(&mut self, flag: UiFlag) {
        use commands::{id, Invocation};
        let l = &mut self.layout;
        match flag {
            UiFlag::Library(on) => l.library = on,
            UiFlag::Inspector(on) => l.inspector = on,
            UiFlag::Panel(on) => l.panel = on,
            UiFlag::Keyboard => self.run(&Invocation::new(id::KEYBOARD)),
            UiFlag::Samples => self.run(&Invocation::new(id::SAMPLES)),
            UiFlag::Palette => self.palette.show(),
            UiFlag::Focus => l.focus = true,
        }
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
                self.keyboard.tab = Keyboard::tab_for(s.inputs);
                self.session = Some(s);
                self.say("ready".into());
            }
            Err(e) => self.fail(format!("load failed: {}", crate::errors::with_causes(&e))),
        }
    }

    fn fail(&mut self, msg: String) {
        self.shared.log.error(msg.clone());
        self.status = msg;
        self.status_error = true;
    }

    /// A plain status message (clears an error).
    fn say(&mut self, msg: String) {
        self.status = msg;
        self.status_error = false;
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

    /// Re-dress the context when the resolved theme changes.
    fn apply_theme(&mut self, ctx: &egui::Context, system: Option<eframe::Theme>) {
        let dark = self.layout.theme.dark(system);
        if self.applied_dark != Some(dark) {
            self.tokens = theme::apply(ctx, dark);
            self.applied_dark = Some(dark);
        }
    }

    /// Save the layout when it changed (never for a screenshot run).
    fn persist_layout(&mut self) {
        let Some((path, saved)) = &mut self.layout_store else { return };
        let now = Layout { focus: false, ..self.layout.clone() };
        if now == *saved {
            return;
        }
        match now.save(path) {
            Ok(()) => *saved = now,
            Err(e) => {
                self.shared.log.warn(format!("layout {}: {e} (not saved)", path.display()));
                *saved = now; // don't retry every frame
            }
        }
    }

    /// Run whatever this frame's shortcuts asked for. Bare keys (Z/X)
    /// wait while a text field has focus or Listen owns the letters.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let bare_keys = !ctx.wants_keyboard_input() && self.project.active_view.mode() != Mode::Listen && !self.palette.open;
        for inv in self.keymap.fired(ctx, bare_keys) {
            if self.palette.open && inv.id != commands::id::PALETTE {
                self.palette.close();
            }
            self.palette.ran(&inv);
            self.run(&inv);
        }
    }
}

impl eframe::App for HostApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(0.001, 0.25);
        self.last_frame = now;
        self.apply_theme(ctx, frame.info().system_theme);
        self.apply_pending_sets();
        self.tick_screenshot(ctx);
        self.handle_shortcuts(ctx);

        let t = self.tokens;
        let mode = self.project.active_view.mode();
        let regions = self.layout.regions(mode);
        let frame_of = |fill| egui::Frame::none().fill(fill).inner_margin(egui::Margin::symmetric(8.0, 4.0));
        let hairline = egui::Stroke::new(1.0, t.line);

        // Declaration order is nesting order: the status bar spans the
        // window, the side bars sit above it, the panel and header take
        // only the editor column.
        egui::TopBottomPanel::bottom("status")
            .exact_height(shell::STATUS_HEIGHT)
            .frame(frame_of(t.status).inner_margin(egui::Margin::symmetric(8.0, 0.0)))
            .show(ctx, |ui| self.draw_statusbar(ui));
        if regions.activity {
            egui::SidePanel::left("activity")
                .exact_width(shell::ACTIVITY_WIDTH)
                .resizable(false)
                .frame(egui::Frame::none().fill(t.chrome).stroke(hairline))
                .show(ctx, |ui| self.draw_activity_bar(ui));
        }
        if regions.library {
            egui::SidePanel::left("library")
                .default_width(240.0)
                .width_range(180.0..=420.0)
                .frame(frame_of(t.chrome).inner_margin(egui::Margin::same(8.0)).stroke(hairline))
                .show(ctx, |ui| self.draw_library(ui));
        }
        if regions.inspector {
            egui::SidePanel::right("inspector")
                .default_width(280.0)
                .width_range(220.0..=480.0)
                .frame(frame_of(t.chrome).inner_margin(egui::Margin::same(8.0)).stroke(hairline))
                .show(ctx, |ui| self.draw_params(ui));
        }
        if regions.panel {
            egui::TopBottomPanel::bottom("panel")
                .resizable(false)
                .frame(frame_of(t.chrome).inner_margin(egui::Margin::same(8.0)).stroke(hairline))
                .show(ctx, |ui| self.draw_keyboard_panel(ui));
        }
        egui::TopBottomPanel::top("header")
            .exact_height(36.0)
            .frame(frame_of(t.canvas).inner_margin(egui::Margin::symmetric(12.0, 0.0)).stroke(hairline))
            .show(ctx, |ui| self.draw_header(ui));
        egui::CentralPanel::default()
            .frame(frame_of(t.canvas).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| self.draw_central(ui, dt));

        let was_busy = self.tools.busy();
        self.tools.poll();
        if was_busy && !self.tools.busy() {
            self.tool_finished();
        }
        // Commands are gathered only while the palette is open (they
        // read the live params and the plugin scan).
        let items = if self.palette.open { self.commands() } else { Vec::new() };
        if let Some(inv) = self.palette.ui(ctx, &items, &self.keymap, &t) {
            self.run(&inv);
        }

        if self.project.active_view.mode() == Mode::Listen || self.palette.open {
            self.release_held_notes();
        } else {
            self.sync_held_notes(ctx);
        }
        self.place_editor(ctx);
        self.persist_layout();
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_flags_parse_open_and_closed() {
        assert_eq!(UiFlag::parse("library"), Some(UiFlag::Library(true)));
        assert_eq!(UiFlag::parse("no-panel"), Some(UiFlag::Panel(false)));
        assert_eq!(UiFlag::parse("palette"), Some(UiFlag::Palette));
        assert_eq!(UiFlag::parse("no-palette"), None);
        assert_eq!(UiFlag::parse("sidebar"), None);
    }
}
