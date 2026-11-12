//! The frame around the views: top bar (rate, block, plugin), project
//! sidebar, controls panel, status bar.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use eframe::egui::{self, Color32, RichText};
use stave_host::config::{non_zero, HostConfig, SampleRate};
use stave_host::control::Control;

use super::{library, HostApp};
use crate::project::{self, Project};

const RATES: [SampleRate; 4] = [
    SampleRate::constant(44_100.0),
    SampleRate::constant(48_000.0),
    SampleRate::constant(88_200.0),
    SampleRate::constant(96_000.0),
];
const BLOCKS: [NonZeroUsize; 6] = [
    non_zero(64),
    non_zero(128),
    non_zero(256),
    non_zero(512),
    non_zero(1024),
    non_zero(2048),
];

impl HostApp {
    pub(super) fn draw_topbar(&mut self, ui: &mut egui::Ui) {
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.heading("stave-play");
            ui.separator();
            let dot = if self.dirty { "● " } else { "" };
            ui.label(RichText::new(format!("{dot}{}", self.project.name)).strong());
            ui.separator();
            if let Some(s) = &self.session {
                ui.label(format!("{} · {} ch out", s.name, s.outputs));
                ui.separator();
            }
            // Rate and block are LIVE: a change reloads the plugin (and
            // renegotiates the device, which writes the real rate back).
            egui::ComboBox::from_id_source("sr_picker")
                .selected_text(format!("{} Hz", self.project.sample_rate.hz()))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for sr in RATES {
                        let sel = self.project.sample_rate == sr;
                        if ui
                            .selectable_label(sel, format!("{} Hz", sr.hz()))
                            .clicked()
                            && !sel
                        {
                            self.project.sample_rate = sr;
                            reload = true;
                        }
                    }
                });
            egui::ComboBox::from_id_source("block_picker")
                .selected_text(format!("block {}", self.project.block_size))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for b in BLOCKS {
                        let sel = self.project.block_size == b;
                        if ui.selectable_label(sel, format!("block {b}")).clicked() && !sel {
                            self.project.block_size = b;
                            reload = true;
                        }
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reload |= ui.button("Reload").clicked();
                if ui.button("Browse…").clicked() {
                    self.browse();
                }
                self.plugin_picker(ui);
            });
        });
        if reload {
            self.mark_dirty();
            self.reload();
        }
    }

    fn plugin_picker(&mut self, ui: &mut egui::Ui) {
        let mut chosen: Option<PathBuf> = None;
        egui::ComboBox::from_id_source("plugin_picker")
            .selected_text(format!(
                "plugin: {}",
                library::display_name(&self.project.plugin_path)
            ))
            .width(260.0)
            .show_ui(ui, |ui| {
                for p in library::scan_vst3(&library::vst3_dirs())
                    .into_iter()
                    .chain(library::modules())
                {
                    let label = library::display_name(&p);
                    if ui
                        .selectable_label(p == self.project.plugin_path, label)
                        .clicked()
                    {
                        chosen = Some(p);
                    }
                }
            });
        if let Some(p) = chosen {
            self.adopt_plugin(p);
        }
    }

    /// `.vst3` bundles are directories, so pick a folder and check it.
    fn browse(&mut self) {
        let start = self.project.plugin_path.parent().map(PathBuf::from);
        let start = start.unwrap_or_else(|| library::vst3_dirs().remove(0));
        let Some(path) = rfd::FileDialog::new().set_directory(&start).pick_folder() else {
            return;
        };
        if library::is_vst3(&path) {
            self.adopt_plugin(path);
        } else {
            self.status = "not a .vst3 bundle".into();
            self.shared
                .log
                .warn(format!("ignored non-vst3 selection: {}", path.display()));
        }
    }

    fn adopt_plugin(&mut self, path: PathBuf) {
        self.project.name = library::display_name(&path);
        self.project.plugin_path = path;
        self.mark_dirty();
        self.reload();
    }

    pub(super) fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("projects");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("new").clicked() {
                self.project = Project::untitled(
                    library::default_plugin(),
                    HostConfig::default().sample_rate,
                    project::DEFAULT_BLOCK,
                );
                self.project_path = None;
                self.mark_dirty();
                self.reload();
            }
            if ui.button("save").clicked() {
                self.save_project();
            }
            if ui.button("refresh").clicked() {
                self.projects = project::list_projects(&self.project_dir);
            }
        });
        ui.separator();
        let mut open: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (p, path) in &self.projects {
                    let active = self.project.name == p.name;
                    let mut label = RichText::new(&p.name);
                    if active {
                        label = label.strong().color(Color32::from_rgb(140, 200, 255));
                    }
                    if ui.selectable_label(active, label).clicked() {
                        open = Some(path.clone());
                    }
                }
                if self.projects.is_empty() {
                    ui.label(RichText::new("(no saved projects)").italics().weak());
                }
            });
        if let Some(path) = open {
            self.open_project(path);
        }
    }

    fn save_project(&mut self) {
        match self.project.save(&self.project_dir) {
            Ok(p) => {
                self.shared
                    .log
                    .info(format!("saved project → {}", p.display()));
                self.project_path = Some(p);
                self.dirty = false;
                self.projects = project::list_projects(&self.project_dir);
            }
            Err(e) => self.fail(format!("save failed: {e}")),
        }
    }

    fn open_project(&mut self, path: PathBuf) {
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

    /// Live controls: stave's grouped panel when the plugin has a debug
    /// bus, the controller's generic sliders otherwise (third-party).
    pub(super) fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading(&self.project.name);
        ui.label(RichText::new(library::display_name(&self.project.plugin_path)).weak());
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
        let Some(s) = &self.session else {
            ui.label(RichText::new("(no plugin loaded)").italics().weak());
            return;
        };
        let log = &self.shared.log;
        let mut write = |name: &str, normalized: f64| {
            if let Err(e) = s.set_normalized(name, normalized) {
                log.warn(format!("set {name}: {e}"));
            }
        };
        let display = |name: &str, normalized: f64| s.display(name, normalized);
        let controls = s.controls();
        if controls.is_empty() {
            ui.label(
                RichText::new("(plugin exposes no parameters)")
                    .italics()
                    .weak(),
            );
        } else {
            generic_controls(ui, &controls, &mut write, &display);
        }
    }

    pub(super) fn draw_statusbar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let peak = f32::from_bits(self.shared.peak.load(Ordering::Relaxed)).clamp(0.0, 1.5);
            ui.label(format!("peak {peak:.2}"));
            ui.separator();
            ui.label(&self.device_name);
            ui.separator();
            ui.label(format!("{} Hz", self.project.sample_rate.hz()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&self.status);
            });
        });
    }
}

/// One slider per writable, visible control, under a heading per group.
/// Sliders move in the control's own normalized space (its taper) and
/// show its owner's text for the value.
fn generic_controls(
    ui: &mut egui::Ui,
    controls: &[Control],
    write: &mut dyn FnMut(&str, f64),
    display: &dyn Fn(&str, f64) -> String,
) {
    let shown: Vec<&Control> = controls
        .iter()
        .filter(|c| !c.flags.hidden && !c.flags.read_only)
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
            for group in groups {
                if !group.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(group.to_uppercase()).strong());
                }
                for c in shown.iter().filter(|c| c.group == group) {
                    let mut normalized = c.normalized;
                    let mut slider = egui::Slider::new(&mut normalized, 0.0..=1.0)
                        .text(c.leaf())
                        .custom_formatter(|n, _| display(&c.name, n));
                    if c.steps > 0 {
                        slider = slider.step_by(1.0 / f64::from(c.steps));
                    }
                    if ui.add(slider).changed() {
                        write(&c.name, normalized);
                    }
                }
            }
        });
}
