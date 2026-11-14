//! The Output panel (VS Code's): what a registry command is doing, and
//! what the last one returned — its verdict, its text as the CLI prints
//! it, the files it wrote, and its JSON (the same `data` the CLI's
//! `--json` and MCP return).

use eframe::egui::{self, RichText};

use super::tools::{self, Finished};
use super::HostApp;

/// The Output tab's body height: room for a report, still a panel.
const BODY_HEIGHT: f32 = 220.0;

impl HostApp {
    /// A run just ended: say so on the status line.
    pub(super) fn tool_finished(&mut self) {
        let Some(f) = &self.tools.last else { return };
        let msg = match (&f.result, tools::passed(f)) {
            (Err(e), _) => format!("{}: {e}", f.title),
            (Ok(_), Some(true)) => format!("{}: passed ({:.1} s)", f.title, f.seconds),
            (Ok(_), _) => format!("{}: FAILED ({:.1} s) — see Output", f.title, f.seconds),
        };
        if tools::passed(f) == Some(true) {
            self.say(msg);
        } else {
            self.fail(msg);
        }
    }

    /// The tab row's tools: cancel a run, copy the last result's JSON.
    pub(super) fn output_toolbar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        if let Some(r) = &self.tools.running {
            let fraction = r.progress.as_ref().and_then(|p| p.total.map(|total| (p.done / total.max(1e-9)) as f32));
            let bar = egui::ProgressBar::new(fraction.unwrap_or(0.0)).desired_width(160.0).animate(fraction.is_none());
            ui.add(bar);
            let message = r.progress.as_ref().map_or_else(|| format!("{}…", r.title), |p| p.message.clone());
            ui.label(RichText::new(message).small().monospace().color(t.ink_soft));
            if ui.small_button("Cancel").clicked() {
                self.tools.cancel();
            }
            return;
        }
        if let Some(title) = self.tools.waiting_title() {
            ui.label(RichText::new(format!("{title}: running on the main thread (another vendor's plugin)…")).small().color(t.ink_soft));
            return;
        }
        let Some(f) = &self.tools.last else {
            ui.label(RichText::new("Run a command from the palette (Run: …) to see its result here").small().color(t.ink_soft));
            return;
        };
        let (verdict, color) = match tools::passed(f) {
            Some(true) => ("passed", t.good),
            Some(false) => ("failed", t.bad),
            None => ("error", t.bad),
        };
        ui.label(RichText::new(&f.title).small().strong());
        ui.label(RichText::new(verdict).small().strong().color(color));
        ui.label(RichText::new(format!("{:.1} s · stave {}", f.seconds, f.id.replace('.', " "))).small().color(t.ink_soft))
            .on_hover_text("The same command on the command line");
        if let Ok(o) = &f.result {
            if ui.small_button("Copy JSON").on_hover_text("The result's data, as `--json` prints it").clicked() {
                let json = serde_json::to_string_pretty(&o.data).unwrap_or_default();
                ui.ctx().copy_text(json);
            }
        }
    }

    pub(super) fn draw_output(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), BODY_HEIGHT));
        let mut body = ui.child_ui(rect, egui::Layout::top_down(egui::Align::Min));
        let Some(f) = &self.tools.last else {
            body.label(RichText::new("Nothing has run yet.").color(t.ink_soft));
            return;
        };
        egui::ScrollArea::vertical().id_source("output-scroll").auto_shrink([false, false]).show(&mut body, |ui| {
            for (path, role) in tools::artefacts(f) {
                let link = ui.link(format!("{role}: {}", path.display()));
                if link.on_hover_text("Open").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{}", path.display())));
                }
            }
            ui.add(egui::Label::new(RichText::new(text_of(f)).monospace()).wrap(true));
        });
    }
}

fn text_of(f: &Finished) -> String {
    match &f.result {
        Ok(o) if !o.text.is_empty() => o.text.clone(),
        Ok(o) => serde_json::to_string_pretty(&o.data).unwrap_or_default(),
        Err(e) => e.clone(),
    }
}
