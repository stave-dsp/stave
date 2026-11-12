//! Peak / RMS meters per channel + stereo correlation. Reads a recent window
//! from the analysis ring each frame; smooths the displayed RMS so the bars
//! don't strobe at GUI rate.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use stave_measure::level;

use super::spectral::FLOOR_DB;
use crate::analysis::AnalysisRing;

/// Linear amplitude → dBFS for display (floored).
fn to_db(amp: f64) -> f32 {
    (level::db(amp) as f32).max(FLOOR_DB)
}

pub struct MetersState {
    pub window: usize,
    pub min_db: f32,
    pub max_db: f32,
    /// Smoothed per-channel RMS in dB.
    smooth_rms_db: [f32; 2],
    /// Peak-hold per channel in dB.
    peak_hold_db: [f32; 2],
    /// When peak-hold was last touched (ms since app start, approximated).
    peak_age: [f32; 2],
    /// Smoothed correlation (-1..1).
    smooth_corr: f32,
    /// Clip latch.
    clipped: [bool; 2],
}

impl Default for MetersState {
    fn default() -> Self {
        Self {
            window: 4096,
            min_db: -60.0,
            max_db: 6.0,
            smooth_rms_db: [-120.0; 2],
            peak_hold_db: [-120.0; 2],
            peak_age: [0.0; 2],
            smooth_corr: 0.0,
            clipped: [false; 2],
        }
    }
}

pub fn ui(ui: &mut egui::Ui, ring: &AnalysisRing, state: &mut MetersState, dt: f32) {
    ui.horizontal(|ui| {
        ui.label("window");
        let mut w = state.window as f32;
        if ui
            .add(
                egui::Slider::new(&mut w, 256.0..=8192.0)
                    .logarithmic(true)
                    .integer(),
            )
            .changed()
        {
            state.window = (w as usize).max(64);
        }
        ui.separator();
        ui.label("range");
        ui.add(egui::Slider::new(&mut state.min_db, -90.0..=-12.0).suffix(" dB"));
        ui.add(egui::Slider::new(&mut state.max_db, 0.0..=12.0).suffix(" dB"));
        ui.separator();
        if ui.button("reset peak").clicked() {
            for p in state.peak_hold_db.iter_mut() {
                *p = -120.0;
            }
            for c in state.clipped.iter_mut() {
                *c = false;
            }
        }
    });

    let n = state.window.min(crate::analysis::CAPACITY);
    let channels = ring.channels();

    // Compute peak / RMS per channel.
    let mut buf_a = vec![0.0_f32; n];
    let mut buf_b = vec![0.0_f32; n];
    let mut peaks_db = [-120.0_f32; 2];
    let mut rms_db = [-120.0_f32; 2];
    for ch in 0..channels.min(2) {
        let dst = if ch == 0 { &mut buf_a } else { &mut buf_b };
        ring.snapshot(ch, dst);
        let p = level::peak(dst);
        if p >= 1.0 {
            state.clipped[ch] = true;
        }
        peaks_db[ch] = to_db(p);
        rms_db[ch] = to_db(level::rms(dst));
    }

    // Smoothing: exponential moving average, time-constant ≈ 120 ms for RMS.
    let alpha_rms = 1.0 - (-dt / 0.12).exp();
    for ch in 0..2 {
        let target = rms_db[ch];
        let cur = &mut state.smooth_rms_db[ch];
        *cur = *cur + alpha_rms * (target - *cur);
        // Peak hold: latch max, decay slowly (-12 dB / s).
        if peaks_db[ch] > state.peak_hold_db[ch] {
            state.peak_hold_db[ch] = peaks_db[ch];
            state.peak_age[ch] = 0.0;
        } else {
            state.peak_age[ch] += dt;
            if state.peak_age[ch] > 1.0 {
                state.peak_hold_db[ch] -= 12.0 * dt;
            }
        }
    }

    // Correlation.
    let raw_corr = if channels >= 2 {
        level::correlation(&buf_a, &buf_b) as f32
    } else {
        1.0
    };
    let alpha_corr = 1.0 - (-dt / 0.18).exp();
    state.smooth_corr += alpha_corr * (raw_corr - state.smooth_corr);

    // Layout: two vertical bars on the left, correlation meter on the right.
    let available = ui.available_rect_before_wrap();
    let height = available.height().max(220.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(available.width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(14));

    let pad = 14.0;
    let bar_area = Rect::from_min_size(
        Pos2::new(rect.min.x + pad, rect.min.y + pad),
        Vec2::new(rect.width() * 0.55 - pad * 2.0, rect.height() - pad * 2.0),
    );
    draw_level_bars(&painter, bar_area, &peaks_db, state, channels.min(2));

    let corr_area = Rect::from_min_size(
        Pos2::new(rect.min.x + rect.width() * 0.6, rect.min.y + pad),
        Vec2::new(rect.width() * 0.4 - pad, rect.height() - pad * 2.0),
    );
    draw_correlation(&painter, corr_area, state.smooth_corr);
}

fn draw_level_bars(
    painter: &egui::Painter,
    rect: Rect,
    peak_db: &[f32; 2],
    state: &MetersState,
    n_channels: usize,
) {
    let (rms_db, peak_hold_db, clipped) =
        (&state.smooth_rms_db, &state.peak_hold_db, &state.clipped);
    let (min_db, max_db) = (state.min_db, state.max_db);
    let bar_count = n_channels.max(1) as f32;
    let gap = 18.0;
    let bar_w = ((rect.width() - gap * (bar_count + 1.0)) / bar_count).max(20.0);
    let range = max_db - min_db;

    // dB scale ticks every 6 dB.
    let first = ((min_db / 6.0).ceil() * 6.0) as i32;
    let last = ((max_db / 6.0).floor() * 6.0) as i32;
    let mut db = first;
    while db <= last {
        let t = (db as f32 - min_db) / range;
        let y = rect.max.y - t * rect.height();
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(0.5, Color32::from_gray(35)),
        );
        painter.text(
            Pos2::new(rect.max.x - 2.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{}", db),
            FontId::monospace(9.0),
            Color32::from_gray(110),
        );
        db += 6;
    }

    for ch in 0..n_channels {
        let x0 = rect.min.x + gap + (bar_w + gap) * ch as f32;
        let bar_rect =
            Rect::from_min_size(Pos2::new(x0, rect.min.y), Vec2::new(bar_w, rect.height()));
        painter.rect_filled(bar_rect, 3.0, Color32::from_gray(22));

        let rms_t = ((rms_db[ch] - min_db) / range).clamp(0.0, 1.0);
        let rms_top = bar_rect.max.y - rms_t * bar_rect.height();
        let rms_fill = Rect::from_min_max(Pos2::new(bar_rect.min.x, rms_top), bar_rect.max);
        let rms_color = level_color(rms_db[ch]);
        painter.rect_filled(rms_fill, 3.0, rms_color);

        // Peak (thin overlay).
        let p_t = ((peak_db[ch] - min_db) / range).clamp(0.0, 1.0);
        let p_y = bar_rect.max.y - p_t * bar_rect.height();
        painter.line_segment(
            [
                Pos2::new(bar_rect.min.x, p_y),
                Pos2::new(bar_rect.max.x, p_y),
            ],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
        );

        // Peak hold tick.
        let ph_t = ((peak_hold_db[ch] - min_db) / range).clamp(0.0, 1.0);
        let ph_y = bar_rect.max.y - ph_t * bar_rect.height();
        painter.line_segment(
            [
                Pos2::new(bar_rect.min.x, ph_y),
                Pos2::new(bar_rect.max.x, ph_y),
            ],
            Stroke::new(2.0, Color32::from_rgb(240, 230, 100)),
        );

        // Clip LED.
        let led = Rect::from_min_size(
            Pos2::new(bar_rect.min.x, bar_rect.min.y - 8.0),
            Vec2::new(bar_w, 6.0),
        );
        let led_color = if clipped[ch] {
            Color32::from_rgb(230, 60, 60)
        } else {
            Color32::from_gray(40)
        };
        painter.rect_filled(led, 1.5, led_color);

        // Labels (top: dB peak; bottom: ch).
        painter.text(
            Pos2::new(bar_rect.center().x, bar_rect.min.y - 16.0),
            Align2::CENTER_BOTTOM,
            format!("{:>5.1}", peak_db[ch]),
            FontId::monospace(11.0),
            Color32::from_gray(220),
        );
        painter.text(
            Pos2::new(bar_rect.center().x, bar_rect.max.y + 4.0),
            Align2::CENTER_TOP,
            if ch == 0 { "L" } else { "R" },
            FontId::proportional(12.0),
            Color32::from_gray(180),
        );
    }
}

fn level_color(db: f32) -> Color32 {
    if db > -3.0 {
        Color32::from_rgb(220, 80, 70)
    } else if db > -12.0 {
        Color32::from_rgb(220, 200, 80)
    } else {
        Color32::from_rgb(80, 200, 120)
    }
}

fn draw_correlation(painter: &egui::Painter, rect: Rect, corr: f32) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(20));
    let mid_y = rect.center().y;
    // Scale: -1 left, 0 center, +1 right.
    let t = (corr + 1.0) * 0.5;
    let x = rect.min.x + t.clamp(0.0, 1.0) * rect.width();
    // Center reference.
    painter.line_segment(
        [
            Pos2::new(rect.center().x, rect.min.y + 6.0),
            Pos2::new(rect.center().x, rect.max.y - 6.0),
        ],
        Stroke::new(0.5, Color32::from_gray(60)),
    );
    let color = if corr < -0.2 {
        Color32::from_rgb(220, 90, 90)
    } else if corr < 0.2 {
        Color32::from_rgb(220, 200, 90)
    } else {
        Color32::from_rgb(120, 220, 130)
    };
    painter.circle_filled(Pos2::new(x, mid_y), 8.0, color);
    painter.text(
        Pos2::new(rect.center().x, rect.min.y + 8.0),
        Align2::CENTER_TOP,
        "correlation",
        FontId::proportional(11.0),
        Color32::from_gray(170),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.max.y - 8.0),
        Align2::CENTER_BOTTOM,
        format!("{:+.2}", corr),
        FontId::monospace(13.0),
        Color32::from_gray(220),
    );
    painter.text(
        Pos2::new(rect.min.x + 6.0, rect.max.y - 8.0),
        Align2::LEFT_BOTTOM,
        "−1  out of phase",
        FontId::proportional(10.0),
        Color32::from_gray(120),
    );
    painter.text(
        Pos2::new(rect.max.x - 6.0, rect.max.y - 8.0),
        Align2::RIGHT_BOTTOM,
        "in phase  +1",
        FontId::proportional(10.0),
        Color32::from_gray(120),
    );
}
