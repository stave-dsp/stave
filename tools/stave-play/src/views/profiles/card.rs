//! Drawing one profile: a gallery card, or the expanded fullscreen plot
//! with a hover readout.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::Profile;
use crate::views::plot::{Density, FreqPlot, LogAxis};

pub enum Action {
    None,
    Remove,
    Expand,
}

/// Profiles span the audio band at the common rates.
fn axis() -> LogAxis {
    LogAxis::new(20.0, 20_000.0)
}

pub fn card(
    ui: &mut egui::Ui,
    p: &Profile,
    size: Vec2,
    color: Color32,
    (min_db, max_db): (f32, f32),
) -> Action {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    let hover = resp.hovered();
    painter.rect_filled(rect, 5.0, Color32::from_gray(if hover { 22 } else { 16 }));
    let border = Stroke::new(
        if hover { 1.2 } else { 1.0 },
        Color32::from_gray(if hover { 110 } else { 45 }),
    );
    painter.rect_stroke(rect, 5.0, border);
    let plot = FreqPlot {
        rect: Rect::from_min_max(
            rect.min + Vec2::new(38.0, 28.0),
            rect.max - Vec2::new(10.0, 22.0),
        ),
        x: axis(),
        min_db,
        max_db,
    };
    plot.grid(&painter, Density::Sparse);
    plot.curve(&painter, &p.points, Stroke::new(1.6, color));
    painter.text(
        rect.min + Vec2::new(8.0, 6.0),
        Align2::LEFT_TOP,
        &p.label,
        FontId::monospace(11.0),
        color,
    );

    let close = Rect::from_min_size(
        Pos2::new(rect.max.x - 22.0, rect.min.y + 4.0),
        Vec2::splat(18.0),
    );
    let on_close = resp.hover_pos().is_some_and(|pos| close.contains(pos));
    painter.rect_filled(
        close,
        2.0,
        if on_close {
            Color32::from_rgb(170, 60, 60)
        } else {
            Color32::from_gray(40)
        },
    );
    painter.text(
        close.center(),
        Align2::CENTER_CENTER,
        "×",
        FontId::proportional(13.0),
        Color32::from_gray(220),
    );
    if hover {
        painter.text(
            Pos2::new(rect.max.x - 30.0, rect.max.y - 6.0),
            Align2::RIGHT_BOTTOM,
            "click to expand ↗",
            FontId::proportional(10.0),
            Color32::from_gray(170),
        );
    }
    match (resp.clicked(), on_close) {
        (true, true) => Action::Remove,
        (true, false) => Action::Expand,
        _ => Action::None,
    }
}

/// Fullscreen plot; `true` when the user asks to go back.
pub fn expanded(
    ui: &mut egui::Ui,
    p: &Profile,
    color: Color32,
    (min_db, max_db): (f32, f32),
) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        back = ui.button("← back to gallery").clicked();
        ui.separator();
        ui.label(RichText::new(&p.label).strong().color(color).monospace());
    });
    ui.add_space(8.0);
    let avail = ui.available_rect_before_wrap();
    let size = Vec2::new(avail.width(), (avail.height() - 8.0).max(360.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::from_gray(14));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(45)));
    let plot = FreqPlot {
        rect: Rect::from_min_max(
            rect.min + Vec2::new(56.0, 12.0),
            rect.max - Vec2::new(16.0, 30.0),
        ),
        x: axis(),
        min_db,
        max_db,
    };
    plot.grid(&painter, Density::Dense);
    plot.curve(&painter, &p.points, Stroke::new(2.0, color));
    if let Some(pos) = resp.hover_pos() {
        plot.readout(&painter, &p.points, pos, color);
    }
    back
}
