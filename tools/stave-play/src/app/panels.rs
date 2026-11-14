//! The side bars: the library (plugins and projects, on demand) on the
//! left, the parameters inspector on the right; and the plugin and
//! project operations they run.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use eframe::egui::{self, RichText};
use stave_host::config::{non_zero, HostConfig, SampleRate};
use stave_host::param::Param;

use super::commands::{id, Invocation};
use super::theme::Tokens;
use super::{library, HostApp};
use crate::project::{self, Project};

pub(super) const RATES: [SampleRate; 4] = [
    SampleRate::constant(44_100.0),
    SampleRate::constant(48_000.0),
    SampleRate::constant(88_200.0),
    SampleRate::constant(96_000.0),
];
pub(super) const BLOCKS: [NonZeroUsize; 6] = [
    non_zero(64),
    non_zero(128),
    non_zero(256),
    non_zero(512),
    non_zero(1024),
    non_zero(2048),
];

/// A side bar's section head: tracked small caps, soft ink.
pub(super) fn section(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text.to_uppercase()).small().strong().color(t.ink_soft));
    ui.add_space(2.0);
}

impl HostApp {
    /// `.vst3` bundles are directories, so pick a folder and check it.
    pub(super) fn browse(&mut self) {
        let start = self.project.plugin_path.parent().map(PathBuf::from);
        let start = start.unwrap_or_else(|| library::vst3_dirs().remove(0));
        let Some(path) = rfd::FileDialog::new().set_directory(&start).pick_folder() else {
            return;
        };
        if library::is_vst3(&path) {
            self.adopt_plugin(path);
        } else {
            self.fail("not a .vst3 bundle".into());
            self.shared
                .log
                .warn(format!("ignored non-vst3 selection: {}", path.display()));
        }
    }

    pub(super) fn adopt_plugin(&mut self, path: PathBuf) {
        self.project.name = library::display_name(&path);
        self.project.plugin_path = path;
        self.mark_dirty();
        self.reload();
    }

    pub(super) fn new_project(&mut self) {
        self.project = Project::untitled(
            library::default_plugin(),
            HostConfig::default().sample_rate,
            project::DEFAULT_BLOCK,
        );
        self.project_path = None;
        self.mark_dirty();
        self.reload();
    }

    pub(super) fn save_project(&mut self) {
        match self.project.save(&self.project_dir) {
            Ok(p) => {
                self.shared
                    .log
                    .info(format!("saved project → {}", p.display()));
                self.say(format!("saved {}", self.project.name));
                self.project_path = Some(p);
                self.dirty = false;
                self.projects = project::list_projects(&self.project_dir);
            }
            Err(e) => self.fail(format!("save failed: {e}")),
        }
    }

    pub(super) fn open_project(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(p) => {
                self.shared.log.info(format!("opened project: {}", p.name));
                self.project = p;
                self.project_path = Some(path);
                self.dirty = false;
                self.reload();
            }
            Err(e) => self.fail(format!("load project failed: {e}")),
        }
    }

    /// The library side bar: find and open a plugin, open or save a
    /// project. Everything here is also in the palette.
    pub(super) fn draw_library(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let mut run: Option<Invocation> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("LIBRARY").small().strong().color(t.ink_soft));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.add(egui::Button::new("×").frame(false));
                if close.on_hover_text(self.tip("Close the library", id::LIBRARY, ui.ctx())).clicked() {
                    run = Some(Invocation::new(id::LIBRARY));
                }
                if ui.add(egui::Button::new("⟳").frame(false)).on_hover_text("Rescan plugins and projects").clicked() {
                    self.rescan_plugins();
                    self.projects = project::list_projects(&self.project_dir);
                }
            });
        });
        ui.horizontal(|ui| {
            let browse = ui.button("…").on_hover_text(self.tip("Browse for a .vst3", id::PLUGIN_BROWSE, ui.ctx()));
            if browse.clicked() {
                run = Some(Invocation::new(id::PLUGIN_BROWSE));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.library_query)
                    .hint_text("Filter plugins")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);
        let query = self.library_query.clone();
        let current = self.project.plugin_path.clone();
        let room = ui.available_height();
        egui::ScrollArea::vertical()
            .id_source("library-scroll")
            .max_height((room - 130.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let (stages, vst3): (Vec<&PathBuf>, Vec<&PathBuf>) = self
                    .plugin_list()
                    .iter()
                    .filter(|p| super::commands::fuzzy_score(&query, &library::display_name(p)).is_some())
                    .partition(|p| stave_host::opened::stage_name(p).is_some());
                for (title, list) in [("Plugins", vst3), ("Stages", stages)] {
                    section(ui, &t, &format!("{title} · {}", list.len()));
                    for p in list {
                        let name = library::display_name(p);
                        let name = name.strip_prefix(stave_host::stages::PREFIX).unwrap_or(&name).to_string();
                        if ui.selectable_label(*p == current, name).clicked() && *p != current {
                            run = Some(Invocation::with(id::PLUGIN_OPEN, p.display().to_string()));
                        }
                    }
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            section(ui, &t, "Projects");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Save").on_hover_text(self.tip("Save the project", id::PROJECT_SAVE, ui.ctx())).clicked() {
                    run = Some(Invocation::new(id::PROJECT_SAVE));
                }
                if ui.small_button("New").clicked() {
                    run = Some(Invocation::new(id::PROJECT_NEW));
                }
            });
        });
        egui::ScrollArea::vertical().id_source("projects-scroll").auto_shrink([false, false]).show(ui, |ui| {
            for (p, path) in &self.projects {
                let active = self.project_path.as_ref() == Some(path);
                if ui.selectable_label(active, &p.name).clicked() && !active {
                    run = Some(Invocation::with(id::PROJECT_OPEN, path.display().to_string()));
                }
            }
            if self.projects.is_empty() {
                ui.label(RichText::new("No saved projects yet. Save keeps the plugin, rate, block and view.").small().color(t.ink_soft));
            }
        });
        if let Some(inv) = run {
            self.run(&inv);
        }
    }

    /// The parameters inspector: stave's grouped params, or the
    /// controller's generic ones (third-party), as name · slider · value.
    pub(super) fn draw_params(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("PARAMETERS").small().strong().color(t.ink_soft));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = ui.add(egui::Button::new("×").frame(false));
                close = b.on_hover_text(self.tip("Hide the inspector", id::INSPECTOR, ui.ctx())).clicked();
            });
        });
        if close {
            return self.run(&Invocation::new(id::INSPECTOR));
        }
        let Some(s) = &self.session else {
            ui.label(RichText::new("No plugin loaded").color(t.ink_soft));
            return;
        };
        let log = &self.shared.log;
        let mut write = |name: &str, normalized: f64| {
            if let Err(e) = s.set_normalized(name, normalized) {
                log.warn(format!("set {name}: {e}"));
            }
        };
        let display = |name: &str, normalized: f64| s.display(name, normalized);
        let params = s.params();
        if params.iter().all(|c| c.flags.hidden || c.read_only) {
            ui.label(RichText::new("This plugin exposes no parameters").color(t.ink_soft));
        } else {
            generic_params(ui, &t, &params, &mut write, &display);
        }
    }
}

/// One row per writable, visible param, under a head per group. Sliders
/// move in the param's own normalized space (its taper) and show its
/// owner's text for the value.
fn generic_params(
    ui: &mut egui::Ui,
    t: &Tokens,
    params: &[Param],
    write: &mut dyn FnMut(&str, f64),
    display: &dyn Fn(&str, f64) -> String,
) {
    let shown: Vec<&Param> = params
        .iter()
        .filter(|c| !c.flags.hidden && !c.read_only)
        .collect();
    let mut groups: Vec<&str> = vec![];
    for c in &shown {
        if !groups.contains(&c.group.as_str()) {
            groups.push(&c.group);
        }
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let (name_w, value_w) = (88.0_f32.min(width * 0.32), 64.0);
            ui.spacing_mut().slider_width = (width - name_w - value_w - 16.0).max(40.0);
            for group in groups {
                if !group.is_empty() {
                    section(ui, t, group);
                }
                for c in shown.iter().filter(|c| c.group == group) {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(name_w, 20.0), egui::Sense::hover());
                        ui.painter().text(
                            r.left_center(),
                            egui::Align2::LEFT_CENTER,
                            c.leaf(),
                            egui::TextStyle::Body.resolve(ui.style()),
                            t.ink,
                        );
                        let mut normalized = c.normalized;
                        let mut slider = egui::Slider::new(&mut normalized, 0.0..=1.0).show_value(false);
                        if c.step_count > 1 {
                            slider = slider.step_by(1.0 / f64::from(c.step_count - 1));
                        }
                        let response = ui.add(slider).on_hover_text(&c.name);
                        if response.changed() {
                            write(&c.name, normalized);
                        }
                        ui.label(RichText::new(display(&c.name, normalized)).monospace().color(t.ink_soft));
                    });
                }
            }
        });
}
