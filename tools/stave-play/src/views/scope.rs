//! Time-domain oscilloscope. Reads a window from the analysis ring per frame
//! and draws a per-channel waveform with optional rising zero-cross trigger
//! so periodic signals don't slide across the screen.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::analysis::AnalysisRing;

const SCOPE_CHANNEL_COLORS: [Color32; 2] = [
    Color32::from_rgb(120, 210, 255),
    Color32::from_rgb(255, 170, 110),
];

pub struct ScopeState {
    pub window: usize, // samples shown
    pub trigger: bool,
    pub gain_db: f32,
}

impl Default for ScopeState {
    fn default() -> Self {
        Self {
            window: 2048,
            trigger: true,
            gain_db: 0.0,
        }
    }
}

pub fn ui(ui: &mut egui::Ui, ring: &AnalysisRing, state: &mut ScopeState) {
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
        ui.checkbox(&mut state.trigger, "trigger");
        ui.separator();
        ui.label("gain");
        ui.add(egui::Slider::new(&mut state.gain_db, -24.0..=24.0).suffix(" dB"));
    });

    let channels = ring.channels();
    let n = state.window.min(crate::analysis::CAPACITY);
    let gain = 10f32.powf(state.gain_db / 20.0);

    let available = ui.available_rect_before_wrap();
    let height = available.height().max(120.0);
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(available.width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(14));

    // Center grid line.
    painter.line_segment(
        [
            Pos2::new(rect.min.x, rect.center().y),
            Pos2::new(rect.max.x, rect.center().y),
        ],
        Stroke::new(1.0, Color32::from_gray(40)),
    );
    // Top / bottom dashed lines at ±0.5.
    for y in [
        rect.min.y + rect.height() * 0.25,
        rect.min.y + rect.height() * 0.75,
    ] {
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(0.5, Color32::from_gray(30)),
        );
    }

    let mut buf = vec![0.0_f32; n];
    for ch in 0..channels {
        ring.snapshot(ch, &mut buf);
        let start = if state.trigger { find_trigger(&buf) } else { 0 };
        draw_trace(
            &painter,
            rect,
            &buf[start..],
            gain,
            SCOPE_CHANNEL_COLORS[ch.min(1)],
        );
    }

    // Stats footer.
    ui.horizontal(|ui| {
        ui.label(format!("samples shown: {}", n));
        ui.separator();
        ui.label(format!("channels: {}", channels));
        ui.separator();
        ui.label(format!("frames written: {}", ring.frames_written()));
    });
}

/// Find the first rising zero-crossing in the first half of `buf`. Falls
/// back to 0 if nothing found, so the trace still draws.
fn find_trigger(buf: &[f32]) -> usize {
    let half = buf.len() / 2;
    for i in 1..half {
        if buf[i - 1] <= 0.0 && buf[i] > 0.0 {
            return i;
        }
    }
    0
}

fn draw_trace(painter: &egui::Painter, rect: Rect, samples: &[f32], gain: f32, color: Color32) {
    if samples.len() < 2 {
        return;
    }
    let mid = rect.center().y;
    let scale = rect.height() * 0.5;
    let dx = rect.width() / (samples.len() - 1) as f32;
    let line: Vec<Pos2> = samples
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Pos2::new(
                rect.min.x + dx * i as f32,
                mid - (v * gain).clamp(-1.0, 1.0) * scale,
            )
        })
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.4, color)));
}
