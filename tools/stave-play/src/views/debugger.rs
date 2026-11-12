//! Debugger view. Renders live probe values, breakpoint controls, the
//! plugin's last captured output block, and a pause/resume control that
//! flips the bus's `paused` flag (the wrapper mutes audio while paused, but
//! the plugin keeps running so its internal state stays inspectable).

use std::sync::atomic::Ordering;

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, Vec2,
};
use stave_debug::DebugBus;

pub fn ui(ui: &mut egui::Ui, bus: Option<&DebugBus>) {
    let Some(bus) = bus else {
        ui.label(
            RichText::new(
                "(this plugin doesn't export stave_debug_bus_for — it wasn't built with the \
                 stave framework, or was built before debug support was added)",
            )
            .italics()
            .color(Color32::from_gray(160)),
        );
        return;
    };

    let paused = bus.is_paused();
    ui.horizontal(|ui| {
        let txt = if paused { "RESUME" } else { "PAUSE" };
        let bg = if paused {
            Color32::from_rgb(220, 90, 90)
        } else {
            Color32::from_rgb(80, 200, 120)
        };
        if ui
            .add(
                egui::Button::new(RichText::new(txt).strong().color(Color32::WHITE))
                    .fill(bg)
                    .min_size(Vec2::new(80.0, 28.0)),
            )
            .clicked()
        {
            bus.set_paused(!paused);
        }
        ui.separator();
        ui.label(format!(
            "block #{}",
            bus.block_counter.load(Ordering::Relaxed)
        ));
        ui.separator();
        if ui.button("reset probe min/max").clicked() {
            let used = bus.probes_used.load(Ordering::Relaxed) as usize;
            for i in 0..used.min(stave_debug::MAX_PROBES) {
                bus.probes[i]
                    .min
                    .store(0x7ff0_0000_0000_0000, Ordering::Relaxed);
                bus.probes[i]
                    .max
                    .store(0xfff0_0000_0000_0000, Ordering::Relaxed);
                bus.probes[i].hits.store(0, Ordering::Relaxed);
            }
        }
        if ui.button("reset breakpoint hits").clicked() {
            let used = bus.breakpoints_used.load(Ordering::Relaxed) as usize;
            for i in 0..used.min(stave_debug::MAX_BREAKPOINTS) {
                bus.breakpoints[i].hit_count.store(0, Ordering::Relaxed);
            }
        }
    });

    ui.add_space(6.0);

    egui::SidePanel::left("debug_probes")
        .resizable(true)
        .default_width(380.0)
        .show_inside(ui, |ui| {
            ui.heading("probes");
            ui.add_space(2.0);
            draw_probes(ui, bus);
            ui.add_space(8.0);
            ui.separator();
            ui.heading("breakpoints");
            ui.add_space(2.0);
            draw_breakpoints(ui, bus);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.heading("capture frame");
        ui.add_space(2.0);
        draw_capture(ui, bus);
    });
}

fn draw_probes(ui: &mut egui::Ui, bus: &DebugBus) {
    let used = bus.probes_used.load(Ordering::Acquire) as usize;
    if used == 0 {
        ui.label(RichText::new("(plugin emits no probes)").italics().weak());
        return;
    }
    ScrollArea::vertical()
        .id_source("probes_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("probes_grid")
                .num_columns(5)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("name").strong());
                    ui.label(RichText::new("value").strong());
                    ui.label(RichText::new("min").strong());
                    ui.label(RichText::new("max").strong());
                    ui.label(RichText::new("hits").strong());
                    ui.end_row();

                    for i in 0..used.min(stave_debug::MAX_PROBES) {
                        let p = &bus.probes[i];
                        let name = p.name_str();
                        let v = f64::from_bits(p.latest.load(Ordering::Relaxed));
                        let lo = f64::from_bits(p.min.load(Ordering::Relaxed));
                        let hi = f64::from_bits(p.max.load(Ordering::Relaxed));
                        let hits = p.hits.load(Ordering::Relaxed);
                        ui.label(RichText::new(name).monospace());
                        ui.monospace(format_value(v));
                        ui.monospace(if lo.is_finite() {
                            format_value(lo)
                        } else {
                            "—".into()
                        });
                        ui.monospace(if hi.is_finite() {
                            format_value(hi)
                        } else {
                            "—".into()
                        });
                        ui.monospace(format!("{}", hits));
                        ui.end_row();
                    }
                });
        });
}

fn draw_breakpoints(ui: &mut egui::Ui, bus: &DebugBus) {
    let used = bus.breakpoints_used.load(Ordering::Acquire) as usize;
    if used == 0 {
        ui.label(
            RichText::new("(plugin defines no breakpoints — call brk_if! to add)")
                .italics()
                .weak(),
        );
        return;
    }
    for i in 0..used.min(stave_debug::MAX_BREAKPOINTS) {
        let bp = &bus.breakpoints[i];
        let name = bp.name_str();
        let mut armed = bp.armed.load(Ordering::Acquire);
        let mut mute = bp.mute_on_hit.load(Ordering::Acquire);
        let hits = bp.hit_count.load(Ordering::Relaxed);
        let last_hit = bp.last_hit_block.load(Ordering::Relaxed);
        let last_cond = bp.last_condition_block.load(Ordering::Relaxed);
        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui.checkbox(&mut armed, "armed").changed() {
                    bp.armed.store(armed, Ordering::Release);
                }
                if ui.checkbox(&mut mute, "mute on hit").changed() {
                    bp.mute_on_hit.store(mute, Ordering::Release);
                }
                ui.label(RichText::new(name).strong().monospace());
            });
            ui.horizontal(|ui| {
                ui.monospace(format!("hits {}", hits));
                ui.separator();
                ui.monospace(format!("last hit block {}", last_hit));
                ui.separator();
                ui.monospace(format!("cond block {}", last_cond));
            });
        });
        ui.add_space(2.0);
    }
}

fn draw_capture(ui: &mut egui::Ui, bus: &DebugBus) {
    let channels = bus.capture.channels.load(Ordering::Acquire) as usize;
    let frames = bus.capture.frames.load(Ordering::Acquire) as usize;
    let block = bus.capture.block.load(Ordering::Acquire);
    ui.horizontal(|ui| {
        ui.monospace(format!(
            "block {} · {} channels · {} samples",
            block, channels, frames
        ));
    });
    if frames == 0 || channels == 0 {
        ui.label(RichText::new("(no capture yet)").italics().weak());
        return;
    }
    let available = ui.available_rect_before_wrap();
    let height = (available.height() - 16.0).max(160.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(available.width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_gray(14));

    // Split into rows per channel.
    let row_h = rect.height() / channels.max(1) as f32;
    let mut buf = vec![0.0_f32; frames.min(stave_debug::CAPTURE_LEN)];
    let colors = [
        Color32::from_rgb(120, 210, 255),
        Color32::from_rgb(255, 170, 110),
    ];
    for ch in 0..channels.min(stave_debug::CAPTURE_CHANNELS) {
        let (got, _) = bus.read_capture_channel(ch, &mut buf);
        let row_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.min.y + row_h * ch as f32),
            Vec2::new(rect.width(), row_h),
        );
        let mid = row_rect.center().y;
        painter.line_segment(
            [
                Pos2::new(row_rect.min.x, mid),
                Pos2::new(row_rect.max.x, mid),
            ],
            Stroke::new(0.5, Color32::from_gray(40)),
        );
        if got >= 2 {
            let scale = row_rect.height() * 0.45;
            let dx = row_rect.width() / (got as f32 - 1.0);
            let color = colors[ch.min(1)];
            let line: Vec<Pos2> = buf[..got as usize]
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Pos2::new(
                        row_rect.min.x + dx * i as f32,
                        mid - v.clamp(-1.0, 1.0) * scale,
                    )
                })
                .collect();
            painter.add(egui::Shape::line(line, Stroke::new(1.3, color)));
        }
        painter.text(
            Pos2::new(row_rect.min.x + 6.0, row_rect.min.y + 4.0),
            Align2::LEFT_TOP,
            format!("ch {}", ch),
            FontId::monospace(10.0),
            Color32::from_gray(180),
        );
    }
}

fn format_value(v: f64) -> String {
    if v.abs() >= 10_000.0 || (v.abs() < 0.01 && v != 0.0) {
        format!("{:.3e}", v)
    } else if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{:.4}", v)
    }
}
