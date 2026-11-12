//! Debug view. Draws the captured log lines, audio-thread stats, and
//! current device/plugin lifecycle info. Read-only; "clear log" is the only
//! action.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, ScrollArea};

use crate::analysis::AnalysisRing;
use crate::debug_log::{DebugLog, LogLevel};

pub struct DebugInfo<'a> {
    pub log: &'a Arc<DebugLog>,
    pub ring: &'a AnalysisRing,
    pub plugin_path: String,
    pub device_name: String,
    pub sample_rate: f32,
    pub block_size: usize,
    pub output_channels: usize,
    pub event_queue_len: usize,
    pub event_queue_cap: usize,
}

pub fn ui(ui: &mut egui::Ui, info: DebugInfo<'_>) {
    egui::Grid::new("debug_stats")
        .num_columns(2)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            let log = info.log;
            row(ui, "device", &info.device_name);
            row(ui, "sample rate", &format!("{:.0} Hz", info.sample_rate));
            row(ui, "block size", &format!("{}", info.block_size));
            row(ui, "output channels", &format!("{}", info.output_channels));
            row(ui, "plugin", &info.plugin_path);
            ui.label("audio callbacks");
            ui.monospace(format!(
                "{}  ({} frames)",
                log.callback_count.load(Ordering::Relaxed),
                log.frames_processed.load(Ordering::Relaxed),
            ));
            ui.end_row();
            ui.label("callback time");
            ui.monospace(format!(
                "last {} µs  ·  max {} µs",
                log.last_cb_us.load(Ordering::Relaxed),
                log.max_cb_us.load(Ordering::Relaxed),
            ));
            ui.end_row();
            ui.label("events");
            ui.monospace(format!(
                "consumed {}  ·  dropped {}  ·  queue {}/{}",
                log.consumed_events.load(Ordering::Relaxed),
                log.dropped_events.load(Ordering::Relaxed),
                info.event_queue_len,
                info.event_queue_cap,
            ));
            ui.end_row();
            ui.label("analysis tap");
            ui.monospace(format!(
                "blocks {}  ·  max-block {} samples  ·  total frames {}",
                info.ring.blocks_written.load(Ordering::Relaxed),
                info.ring.max_block_len.load(Ordering::Relaxed),
                info.ring.frames_written(),
            ));
            ui.end_row();
        });

    ui.separator();
    ui.horizontal(|ui| {
        ui.heading("log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("clear").clicked() {
                info.log.clear();
            }
        });
    });

    let lines = info.log.snapshot();
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in lines {
                let color = match line.level {
                    LogLevel::Info => Color32::from_gray(200),
                    LogLevel::Warn => Color32::from_rgb(230, 200, 90),
                    LogLevel::Error => Color32::from_rgb(230, 110, 110),
                };
                let secs = line.when_ms as f32 / 1000.0;
                ui.label(
                    RichText::new(format!("[{:>8.3}]  {}", secs, line.text))
                        .monospace()
                        .color(color),
                );
            }
        });
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    ui.monospace(value);
    ui.end_row();
}
