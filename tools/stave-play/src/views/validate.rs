//! One check per frame on the UI thread: main-thread-affine plugins
//! (JUCE) are created where they expect, and the tab fills in as
//! checks land.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText, ScrollArea};
use stave_host::config::HostConfig;
use stave_host::loaded::Loaded;
use stave_host::validate::{CheckResult, Status, Suite, SuiteReport};

#[derive(Default)]
pub struct ValidateState {
    run: Option<Run>,
}

struct Run {
    suite: Suite,
    bundle: PathBuf,
    config: HostConfig,
    results: Vec<CheckResult>,
}

impl Run {
    fn done(&self) -> bool {
        self.results.len() == self.suite.checks.len()
    }

    fn step(&mut self) {
        let Some(check) = self.suite.checks.get(self.results.len()) else {
            return;
        };
        let (bundle, config) = (self.bundle.clone(), self.config);
        let mut fresh = move || Loaded::open(&bundle, config).map(Loaded::into_instance);
        let result = self.suite.run_check(&mut fresh, check);
        self.results.push(result);
    }
}

pub fn ui(ui: &mut egui::Ui, bundle: &Path, config: HostConfig, state: &mut ValidateState) {
    let running = state.run.as_ref().is_some_and(|r| !r.done());
    ui.horizontal(|ui| {
        let label = if state.run.is_some() {
            "Re-run suite"
        } else {
            "Run suite"
        };
        let button = egui::Button::new(RichText::new(label).strong())
            .min_size(egui::vec2(150.0, 28.0))
            .fill(Color32::from_rgb(50, 90, 120));
        let clicked = ui
            .add_enabled(!running, button)
            .on_hover_text("loads fresh instances of the bundle; the live plugin is untouched")
            .clicked();
        if clicked {
            state.run = Some(Run {
                suite: Suite::default(),
                bundle: bundle.to_path_buf(),
                config,
                results: Vec::new(),
            });
        }
        if let Some(run) = &state.run {
            summary(ui, run);
        }
    });
    ui.add_space(6.0);

    let Some(run) = state.run.as_mut() else {
        ui.label(
            RichText::new("(run the suite to certify the loaded plugin)")
                .italics()
                .weak(),
        );
        return;
    };
    ui.label(
        RichText::new(run.bundle.display().to_string())
            .weak()
            .monospace(),
    );
    ui.add_space(4.0);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in &run.results {
                row(ui, r);
            }
            if let Some(next) = run.suite.checks.get(run.results.len()) {
                ui.label(
                    RichText::new(format!("running {} …", next.name))
                        .weak()
                        .monospace(),
                );
            }
        });
    if !run.done() {
        run.step();
        ui.ctx().request_repaint();
    }
}

fn summary(ui: &mut egui::Ui, run: &Run) {
    if !run.done() {
        ui.label(format!("{}/{}", run.results.len(), run.suite.checks.len()));
        return;
    }
    let report = SuiteReport::from_results(run.results.clone());
    let total: f64 = report.checks.iter().map(|c| c.duration_ms).sum();
    let text = if report.passed() {
        RichText::new(format!("✓ no failures ({:.0} ms)", total)).color(PASS)
    } else {
        RichText::new(format!("✗ {} failed ({:.0} ms)", report.failures, total)).color(FAIL)
    };
    ui.label(text.strong());
}

const PASS: Color32 = Color32::from_rgb(120, 220, 130);
const INFO: Color32 = Color32::from_rgb(140, 180, 230);
const FAIL: Color32 = Color32::from_rgb(230, 110, 110);
const NOT_APPLICABLE: Color32 = Color32::from_rgb(140, 140, 140);

fn row(ui: &mut egui::Ui, r: &CheckResult) {
    let (mark, color) = match r.status {
        Status::Pass => ("✓", PASS),
        Status::Info => ("i", INFO),
        Status::NotApplicable => ("–", NOT_APPLICABLE),
        Status::Fail => ("✗", FAIL),
    };
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(mark).strong().color(color).size(16.0));
            ui.label(RichText::new(r.check).monospace().strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{:.1} ms", r.duration_ms)).weak());
            });
        });
        if r.status != Status::Pass || !r.details.is_null() {
            ui.label(
                RichText::new(r.details.to_string())
                    .monospace()
                    .small()
                    .color(color),
            );
        }
    });
    ui.add_space(2.0);
}
