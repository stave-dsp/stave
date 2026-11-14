//! The workbench's own commands: what it offers the palette, and what
//! each id does. Modes and views, layout toggles, theme, audio settings,
//! plugins and projects all run through [`HostApp::run`].

use std::path::PathBuf;

use stave_host::config::SampleRate;

use super::commands::{id, CommandInfo, CommandSource, Invocation};
use super::keyboard::Tab;
use super::panels::{BLOCKS, RATES};
use super::theme::ThemeChoice;
use super::{library, tools, HostApp};
use crate::project::{Mode, ViewId};

impl CommandSource for HostApp {
    fn commands(&self) -> Vec<CommandInfo> {
        let mut out = vec![];
        for m in Mode::ALL {
            out.push(CommandInfo::new("Go to", format!("{} — {}", m.label(), m.about()), Invocation::with(id::MODE, m.label())));
        }
        for m in Mode::ALL.into_iter().filter(|m| m.views().len() > 1) {
            for v in m.views() {
                out.push(CommandInfo::new(m.label(), v.label(), Invocation::with(id::VIEW, v.label())));
            }
        }
        let l = &self.layout;
        let toggle = |open: bool, what: &str| format!("{} {what}", if open { "Hide" } else { "Show" });
        out.extend([
            CommandInfo::new("Layout", toggle(l.library, "library (plugins, projects)"), Invocation::new(id::LIBRARY)),
            CommandInfo::new("Layout", toggle(l.inspector, "parameters inspector"), Invocation::new(id::INSPECTOR)),
            CommandInfo::new("Layout", toggle(l.panel, "keyboard panel"), Invocation::new(id::PANEL)),
            CommandInfo::new(
                "Layout",
                if l.focus { "Leave focus mode" } else { "Focus mode (editor only)" },
                Invocation::new(id::FOCUS),
            ),
            CommandInfo::new("Input", "Show the keyboard", Invocation::new(id::KEYBOARD)),
            CommandInfo::new("Input", "Show the sample pads", Invocation::new(id::SAMPLES)),
            CommandInfo::new("Layout", "Show command output", Invocation::new(id::OUTPUT)),
            CommandInfo::new("Input", "Octave up", Invocation::new(id::OCTAVE_UP)),
            CommandInfo::new("Input", "Octave down", Invocation::new(id::OCTAVE_DOWN)),
        ]);
        for t in ThemeChoice::ALL {
            out.push(CommandInfo::new("Theme", t.label(), Invocation::with(id::THEME, t.id())));
        }
        for r in RATES {
            out.push(CommandInfo::new("Audio", format!("Sample rate {} Hz", r.hz()), Invocation::with(id::SAMPLE_RATE, r.hz().to_string())));
        }
        for b in BLOCKS {
            out.push(CommandInfo::new("Audio", format!("Block size {b}"), Invocation::with(id::BLOCK, b.to_string())));
        }
        out.extend([
            CommandInfo::new("Plugin", "Reload", Invocation::new(id::PLUGIN_RELOAD)),
            CommandInfo::new("Plugin", "Browse for a .vst3…", Invocation::new(id::PLUGIN_BROWSE)),
        ]);
        for p in self.plugin_list() {
            out.push(CommandInfo::new(
                "Open plugin",
                library::display_name(p),
                Invocation::with(id::PLUGIN_OPEN, p.display().to_string()),
            ));
        }
        out.extend([
            CommandInfo::new("Project", "New", Invocation::new(id::PROJECT_NEW)),
            CommandInfo::new("Project", "Save", Invocation::new(id::PROJECT_SAVE)),
        ]);
        for (p, path) in &self.projects {
            out.push(CommandInfo::new("Open project", p.name.clone(), Invocation::with(id::PROJECT_OPEN, path.display().to_string())));
        }
        out.extend(self.tools.commands(self.tool_target().as_ref()));
        out
    }
}

impl HostApp {
    /// Run a command, from whichever surface it came. An id the
    /// workbench doesn't know is reported, not dropped silently.
    pub(super) fn run(&mut self, inv: &Invocation) {
        let arg = inv.arg.as_deref().unwrap_or_default();
        match inv.id.as_str() {
            id::PALETTE => self.palette.toggle(),
            id::MODE => {
                if let Some(m) = Mode::ALL.into_iter().find(|m| m.label() == arg) {
                    self.open_mode(m);
                }
            }
            id::VIEW => {
                if let Some(v) = ViewId::parse(arg) {
                    self.open_view(v);
                }
            }
            id::LIBRARY => {
                self.layout.library = !self.layout.library;
                self.layout.focus = false;
            }
            id::INSPECTOR => {
                self.layout.inspector = !self.layout.inspector;
                self.layout.focus = false;
            }
            id::PANEL => {
                self.layout.panel = !self.layout.panel;
                self.layout.focus = false;
            }
            id::FOCUS => self.layout.focus = !self.layout.focus,
            id::THEME => {
                if let Some(t) = ThemeChoice::parse(arg) {
                    self.layout.theme = t;
                }
            }
            id::KEYBOARD | id::SAMPLES | id::OUTPUT => {
                self.keyboard.tab = match inv.id.as_str() {
                    id::KEYBOARD => Tab::Midi,
                    id::SAMPLES => Tab::Samples,
                    _ => Tab::Output,
                };
                self.layout.panel = true;
                self.layout.focus = false;
            }
            id::OCTAVE_UP | id::OCTAVE_DOWN => {
                let step = if inv.id == id::OCTAVE_UP { 1 } else { -1 };
                self.set_octave(self.project.octave + step);
            }
            id::SAMPLE_RATE => {
                let rate = arg.parse::<f64>().ok().and_then(|hz| SampleRate::new(hz).ok());
                if let Some(r) = rate.filter(|r| *r != self.project.sample_rate) {
                    self.project.sample_rate = r;
                    self.mark_dirty();
                    self.reload();
                }
            }
            id::BLOCK => {
                let block = arg.parse::<usize>().ok().and_then(std::num::NonZeroUsize::new);
                if let Some(b) = block.filter(|b| *b != self.project.block_size) {
                    self.project.block_size = b;
                    self.mark_dirty();
                    self.reload();
                }
            }
            id::PLUGIN_OPEN => self.adopt_plugin(PathBuf::from(arg)),
            id::PLUGIN_BROWSE => self.browse(),
            id::PLUGIN_RELOAD => self.reload(),
            id::PROJECT_NEW => self.new_project(),
            id::PROJECT_SAVE => self.save_project(),
            id::PROJECT_OPEN => self.open_project(PathBuf::from(arg)),
            tool if tool.starts_with(tools::PREFIX) => {
                let target = self.tool_target();
                match self.tools.start(&tool[tools::PREFIX.len()..], target.as_ref()) {
                    Ok(()) => self.run(&Invocation::new(id::OUTPUT)),
                    Err(e) => self.fail(e),
                }
            }
            other => self.shared.log.warn(format!("unknown command {other}")),
        }
    }

    /// Show `view` (leaving Listen hands the device back first).
    pub(super) fn open_view(&mut self, view: ViewId) {
        let from = self.project.active_view;
        if from == view {
            return;
        }
        if from == ViewId::Voicing {
            self.leave_voicing();
        }
        self.project.active_view = view;
        self.layout.last_view.insert(view.mode(), view);
    }

    pub(super) fn open_mode(&mut self, mode: Mode) {
        self.open_view(self.layout.view_for(mode));
    }

    pub(super) fn set_octave(&mut self, octave: i32) {
        let octave = octave.clamp(0, 8);
        if octave != self.project.octave {
            self.project.octave = octave;
            self.mark_dirty();
        }
    }

    /// Every plugin the library knows (scanned once, refreshed on demand).
    pub(super) fn plugin_list(&self) -> &[PathBuf] {
        self.plugins.get_or_init(|| library::scan_vst3(&library::vst3_dirs()).into_iter().chain(library::stages()).collect())
    }

    /// The loaded plugin as a registry command's target, as currently
    /// set; none while nothing is loaded.
    pub(super) fn tool_target(&self) -> Option<tools::Target> {
        let s = self.session.as_ref()?;
        Some(tools::Target::new(
            &self.project.plugin_path,
            self.project.sample_rate.hz(),
            self.project.block_size.get(),
            &s.params(),
            s.telemetry().is_some(),
        ))
    }

    pub(super) fn rescan_plugins(&mut self) {
        self.plugins = std::cell::OnceCell::new();
    }
}
