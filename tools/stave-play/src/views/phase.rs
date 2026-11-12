//! Phase: a goniometer (L/R rotated 45°: mono vertical, anti-phase
//! horizontal) and per-bin phase vs log frequency for channel 0.

use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Vec2};

use super::plot::LogAxis;
use super::spectral::{Analyzer, AnalyzerConfig};
use crate::analysis::AnalysisRing;

const FFT_SIZES: &[usize] = &[1024, 2048, 4096];
/// Bins quieter than this have noise for a phase; they are not drawn.
const MIN_MAG: f32 = 1e-4;

pub struct PhaseState {
    fft_size: usize,
    gonio_gain_db: f32,
    analyzer: Analyzer,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Default for PhaseState {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            gonio_gain_db: 0.0,
            analyzer: Analyzer::new(AnalyzerConfig {
                size: 2048,
                ..AnalyzerConfig::default()
            }),
            left: vec![0.0; 2048],
            right: vec![0.0; 2048],
        }
    }
}

pub fn ui(ui: &mut egui::Ui, ring: &AnalysisRing, rate: f32, state: &mut PhaseState) {
    ui.horizontal(|ui| {
        ui.label("fft");
        egui::ComboBox::from_id_source("phase_fft_size")
            .selected_text(state.fft_size.to_string())
            .show_ui(ui, |ui| {
                for &s in FFT_SIZES {
                    ui.selectable_value(&mut state.fft_size, s, s.to_string());
                }
            });
        ui.separator();
        ui.label("gonio gain");
        ui.add(egui::Slider::new(&mut state.gonio_gain_db, -12.0..=24.0).suffix(" dB"));
    });
    state.analyzer.configure(AnalyzerConfig {
        size: state.fft_size,
        ..state.analyzer.config()
    });
    let avail = ui.available_rect_before_wrap();
    let size = Vec2::new(avail.width(), (avail.height() - 24.0).max(220.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(14));
    let (gonio, phase) = rect.split_left_right_at_fraction(0.5);
    goniometer(&painter, gonio.shrink(12.0), ring, state);
    phase_panel(
        &painter,
        phase.shrink(12.0),
        ring,
        rate,
        &mut state.analyzer,
    );
}

/// L/R → (side, mid): mono traces the vertical, anti-phase the horizontal.
fn rotate(l: f32, r: f32) -> (f32, f32) {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    ((l - r) * k, (l + r) * k)
}

fn goniometer(painter: &Painter, inner: Rect, ring: &AnalysisRing, state: &mut PhaseState) {
    let dim = Stroke::new(0.5, Color32::from_gray(40));
    painter.rect_stroke(inner, 3.0, Stroke::new(1.0, Color32::from_gray(50)));
    painter.line_segment([inner.left_top(), inner.right_bottom()], dim);
    painter.line_segment([inner.right_top(), inner.left_bottom()], dim);
    let c = inner.center();
    for (text, at, align) in [
        ("M", Pos2::new(c.x, inner.min.y + 4.0), Align2::CENTER_TOP),
        (
            "S",
            Pos2::new(c.x, inner.max.y - 4.0),
            Align2::CENTER_BOTTOM,
        ),
        ("L", Pos2::new(inner.min.x + 4.0, c.y), Align2::LEFT_CENTER),
        ("R", Pos2::new(inner.max.x - 4.0, c.y), Align2::RIGHT_CENTER),
    ] {
        painter.text(
            at,
            align,
            text,
            FontId::monospace(11.0),
            Color32::from_gray(120),
        );
    }
    if ring.channels() < 2 {
        let msg = "(mono — needs 2 channels)";
        painter.text(
            c,
            Align2::CENTER_CENTER,
            msg,
            FontId::proportional(12.0),
            Color32::from_gray(140),
        );
        return;
    }
    ring.snapshot(0, &mut state.left);
    ring.snapshot(1, &mut state.right);
    let half = inner.width().min(inner.height()) * 0.5;
    let gain = 10f32.powf(state.gonio_gain_db / 20.0);
    let dot = Color32::from_rgba_unmultiplied(120, 210, 255, 80);
    for (&l, &r) in state.left.iter().zip(&state.right) {
        let (x, y) = rotate((l * gain).clamp(-1.0, 1.0), (r * gain).clamp(-1.0, 1.0));
        painter.circle_filled(Pos2::new(c.x + x * half, c.y - y * half), 1.0, dot);
    }
}

fn phase_panel(painter: &Painter, inner: Rect, ring: &AnalysisRing, rate: f32, a: &mut Analyzer) {
    painter.rect_stroke(inner, 3.0, Stroke::new(1.0, Color32::from_gray(50)));
    let label = |at, align, text| {
        painter.text(
            at,
            align,
            text,
            FontId::monospace(10.0),
            Color32::from_gray(120),
        )
    };
    label(
        inner.min + Vec2::new(4.0, 4.0),
        Align2::LEFT_TOP,
        "phase vs freq (ch 0)",
    );
    label(
        Pos2::new(inner.max.x - 2.0, inner.min.y + 14.0),
        Align2::RIGHT_TOP,
        "+π",
    );
    label(
        Pos2::new(inner.max.x - 2.0, inner.max.y - 14.0),
        Align2::RIGHT_BOTTOM,
        "−π",
    );
    let mid = inner.center().y;
    painter.line_segment(
        [Pos2::new(inner.min.x, mid), Pos2::new(inner.max.x, mid)],
        Stroke::new(0.5, Color32::from_gray(40)),
    );
    let axis = LogAxis::audio(rate);
    let spec = a.ring_spectrum(ring, 0).to_vec();
    let color = Color32::from_rgb(180, 160, 240);
    for (i, bin) in spec.iter().enumerate().skip(1) {
        let hz = a.bin_hz(i, rate);
        if !axis.contains(hz) || bin.norm() < MIN_MAG {
            continue;
        }
        let ty = 1.0 - (bin.arg() + std::f32::consts::PI) / std::f32::consts::TAU;
        let p = Pos2::new(
            inner.min.x + axis.frac(hz) * inner.width(),
            inner.min.y + ty * inner.height(),
        );
        painter.circle_filled(p, 1.4, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_vertical_and_anti_phase_horizontal() {
        let (x, y) = rotate(0.5, 0.5);
        assert!(x.abs() < 1e-7 && y > 0.7);
        let (x, y) = rotate(0.5, -0.5);
        assert!(y.abs() < 1e-7 && x > 0.7);
    }
}
