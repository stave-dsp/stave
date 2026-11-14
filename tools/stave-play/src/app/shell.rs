//! The workbench chrome: the activity bar (modes), the header (the
//! plugin, the mode's view tabs, the command search) and the status bar
//! (device, rate, block, latency, CPU, peak, octave, messages).

use std::sync::atomic::Ordering;

use eframe::egui::{self, Align2, Color32, RichText, Sense};

use super::commands::{id, Invocation};
use super::panels::{BLOCKS, RATES};
use super::HostApp;
use crate::debug_log::PERF_HISTORY;
use crate::project::{Mode, ViewId};
use crate::views;

pub(super) const ACTIVITY_WIDTH: f32 = 56.0;
pub(super) const STATUS_HEIGHT: f32 = 24.0;

/// The activity bar's glyph per mode (egui's bundled symbol fonts).
fn glyph(m: Mode) -> &'static str {
    match m {
        Mode::Play => "▶",
        Mode::Listen => "🎧",
        Mode::Measure => "📈",
        Mode::Validate => "✔",
        Mode::Debug => "🐛",
    }
}

impl HostApp {
    /// "Label (⌘B)": a tooltip that teaches the shortcut.
    pub(super) fn tip(&self, text: &str, command: &str, ctx: &egui::Context) -> String {
        self.tip_for(text, &Invocation::new(command), ctx)
    }

    fn tip_for(&self, text: &str, inv: &Invocation, ctx: &egui::Context) -> String {
        match self.keymap.hint(ctx, inv) {
            Some(h) => format!("{text}  ({h})"),
            None => text.to_string(),
        }
    }

    pub(super) fn draw_activity_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let active = self.project.active_view.mode();
        let mut run = None;
        ui.add_space(6.0);
        for m in Mode::ALL {
            let inv = Invocation::with(id::MODE, m.label());
            let tip = self.tip_for(&format!("{} — {}", m.label(), m.about()), &inv, ui.ctx());
            if rail_button(ui, &t, glyph(m), m.label(), m == active).on_hover_text(tip).clicked() {
                run = Some(inv);
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let tip = self.tip("Commands", id::PALETTE, ui.ctx());
            if rail_button(ui, &t, "🔍", "Search", self.palette.open).on_hover_text(tip).clicked() {
                run = Some(Invocation::new(id::PALETTE));
            }
            let tip = self.tip("Library — plugins and projects", id::LIBRARY, ui.ctx());
            if rail_button(ui, &t, "📚", "Library", self.layout.library).on_hover_text(tip).clicked() {
                run = Some(Invocation::new(id::LIBRARY));
            }
        });
        if let Some(inv) = run {
            self.run(&inv);
        }
    }

    /// The header over the editor area: what is loaded, the mode's
    /// views as tabs, and the command search.
    pub(super) fn draw_header(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let mode = self.project.active_view.mode();
        let mut run = None;
        ui.horizontal_centered(|ui| {
            // Listen compares its own candidates: the loaded plugin is
            // not what is playing, so the header names the mode.
            let name = match (&self.session, mode) {
                (_, Mode::Listen) => "Listen".to_string(),
                (Some(s), _) => s.name.clone(),
                (None, _) => self.project.name.clone(),
            };
            let dot = if self.dirty { "● " } else { "" };
            let title = ui
                .add(egui::Label::new(RichText::new(format!("{dot}{name}")).strong().color(t.ink)).sense(Sense::click()))
                .on_hover_text(self.tip("Change plugin (library)", id::LIBRARY, ui.ctx()));
            if title.clicked() {
                run = Some(Invocation::new(id::LIBRARY));
            }
            if let Some(s) = self.session.as_ref().filter(|_| mode != Mode::Listen) {
                let io = if s.inputs == 0 { format!("instrument · {} out", s.outputs) } else { format!("{} in · {} out", s.inputs, s.outputs) };
                ui.label(RichText::new(io).small().color(t.ink_soft));
            }
            if mode.views().len() > 1 {
                ui.add_space(12.0);
                for &v in mode.views() {
                    let selected = self.project.active_view == v;
                    if tab(ui, &t, v.label(), selected).clicked() && !selected {
                        run = Some(Invocation::with(id::VIEW, v.label()));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = self.keymap.hint(ui.ctx(), &Invocation::new(id::PALETTE)).unwrap_or_default();
                let search = egui::Button::new(RichText::new(format!("🔍  Commands   {hint}")).color(t.ink_soft))
                    .min_size(egui::vec2(180.0, 22.0));
                if ui.add(search).on_hover_text("Every action, by fuzzy search").clicked() {
                    run = Some(Invocation::new(id::PALETTE));
                }
            });
        });
        if let Some(inv) = run {
            self.run(&inv);
        }
    }

    pub(super) fn draw_statusbar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let mut run = None;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            // Status items read as text until hovered (VS Code's).
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.weak_bg_fill = Color32::TRANSPARENT;
            w.inactive.bg_fill = Color32::TRANSPARENT;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 1.0);
            let small = |s: String| RichText::new(s).small().color(t.ink_soft);
            if !self.device_name.is_empty() {
                ui.label(small(self.device_name.clone())).on_hover_text("Output device (the system default)");
            }
            let rate = self.project.sample_rate;
            ui.menu_button(small(format!("{} kHz", rate.hz() / 1000.0)), |ui| {
                for r in RATES {
                    if ui.selectable_label(r == rate, format!("{} Hz", r.hz())).clicked() {
                        run = Some(Invocation::with(id::SAMPLE_RATE, r.hz().to_string()));
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text("Sample rate (reloads the plugin)");
            let block = self.project.block_size;
            ui.menu_button(small(format!("block {block}")), |ui| {
                for b in BLOCKS {
                    if ui.selectable_label(b == block, format!("{b} samples")).clicked() {
                        run = Some(Invocation::with(id::BLOCK, b.to_string()));
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text("Block size (reloads the plugin)");
            let plugin_latency = self.shared.log.plugin_latency.load(Ordering::Relaxed);
            let latency_ms = (block.get() as f64 + f64::from(plugin_latency)) * 1000.0 / rate.hz();
            ui.label(small(format!("{latency_ms:.1} ms"))).on_hover_text(format!(
                "Latency: block {block} + plugin {plugin_latency} samples"
            ));
            if self.session.is_some() {
                let (load, misses) = self.cpu_load();
                let pct = load * 100.0;
                let text = RichText::new(if pct < 10.0 { format!("CPU {pct:.1}%") } else { format!("CPU {pct:.0}%") }).small();
                let text = if misses > 0 { text.color(t.bad) } else { text.color(t.ink_soft) };
                let cpu = ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_text(format!(
                    "Audio callback load against its realtime budget; {misses} overruns this session. Click: the CPU view."
                ));
                if cpu.clicked() {
                    run = Some(Invocation::with(id::VIEW, ViewId::Perf.label()));
                }
                peak_meter(ui, &t, f32::from_bits(self.shared.peak.load(Ordering::Relaxed)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(line) = self.tools.status_line() {
                    let job = ui.add(egui::Label::new(RichText::new(format!("● {line}")).small().color(t.accent)).sense(Sense::click()));
                    if job.on_hover_text("A command is running. Click: the Output panel").clicked() {
                        run = Some(Invocation::new(id::OUTPUT));
                    }
                }
                if self.project.active_view.mode() != Mode::Listen {
                    let o = self.project.octave;
                    let keys = format!(
                        "keys {}–{}",
                        views::piano::note_name(views::piano::midi_from_semi(o, 0)),
                        views::piano::note_name(views::piano::midi_from_semi(o, 16))
                    );
                    let k = ui.add(egui::Label::new(small(keys)).sense(Sense::click())).on_hover_text(self.tip(
                        "A–; play notes, Z/X shift the octave. Click: the keyboard panel",
                        id::PANEL,
                        ui.ctx(),
                    ));
                    if k.clicked() {
                        run = Some(Invocation::new(id::KEYBOARD));
                    }
                }
                let color = if self.status_error { t.bad } else { t.ink_soft };
                ui.add(egui::Label::new(RichText::new(&self.status).small().color(color)).truncate(true));
            });
        });
        if let Some(inv) = run {
            self.run(&inv);
        }
    }

    /// Mean callback load over the recent history (time spent / time
    /// the block lasts) and the session's overruns.
    fn cpu_load(&self) -> (f64, u64) {
        let log = &self.shared.log;
        let n = log.perf_idx.load(Ordering::Relaxed);
        let rate = self.project.sample_rate.hz();
        let take = n.min(64);
        let mut sum = 0.0;
        for k in 0..take {
            let i = ((n - 1 - k) as usize) & (PERF_HISTORY - 1);
            let us = f64::from(log.perf_us[i].load(Ordering::Relaxed));
            let frames = f64::from(log.perf_frames[i].load(Ordering::Relaxed));
            if frames > 0.0 {
                sum += us / (frames * 1e6 / rate);
            }
        }
        let load = if take == 0 { 0.0 } else { sum / take as f64 };
        (load, log.deadline_misses.load(Ordering::Relaxed))
    }
}

/// An activity-bar button: glyph over a small label, an accent bar on
/// the left edge when active.
fn rail_button(ui: &mut egui::Ui, t: &super::theme::Tokens, glyph: &str, label: &str, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ACTIVITY_WIDTH, 48.0), Sense::click());
    let p = ui.painter();
    if response.hovered() {
        p.rect_filled(rect.shrink(4.0), 6.0, t.ink.gamma_multiply(0.06));
    }
    if active {
        let bar = egui::Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 8.0), egui::vec2(2.0, rect.height() - 16.0));
        p.rect_filled(bar, 1.0, t.accent);
    }
    let ink = if active { t.ink } else { t.ink_soft };
    p.text(rect.center() - egui::vec2(0.0, 7.0), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(18.0), ink);
    p.text(rect.center() + egui::vec2(0.0, 13.0), Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0), ink);
    response
}

/// A header tab: text, underlined in the accent when selected.
fn tab(ui: &mut egui::Ui, t: &super::theme::Tokens, label: &str, selected: bool) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let size = egui::vec2(galley.size().x + 16.0, ui.available_height().max(28.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let ink = if selected || response.hovered() { t.ink } else { t.ink_soft };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    if selected {
        let line = egui::Rect::from_min_max(rect.left_bottom() - egui::vec2(-6.0, 2.0), rect.right_bottom() - egui::vec2(6.0, 0.0));
        ui.painter().rect_filled(line, 1.0, t.accent);
    }
    response
}

/// Peak in dBFS with a small bar: amber past -6, red at and above 0.
fn peak_meter(ui: &mut egui::Ui, t: &super::theme::Tokens, peak: f32) {
    let db = 20.0 * peak.max(1e-6).log10();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(48.0, 6.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, t.ink.gamma_multiply(0.12));
    let fill = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let color = if db >= 0.0 { t.bad } else if db > -6.0 { t.accent } else { t.good };
    p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fill, rect.height())), 2.0, color);
    let text = if db <= -100.0 { "−∞ dB".to_string() } else { format!("{db:.1} dB") };
    response.on_hover_text("Output peak (dBFS)");
    ui.label(RichText::new(text).small().monospace().color(if db >= 0.0 { t.bad } else { t.ink_soft }));
}
