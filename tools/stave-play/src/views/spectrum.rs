//! Magnitude spectrum of the plugin output: log frequency × dBFS, per
//! channel, with an optional slowly decaying peak hold.

use eframe::egui::{self, Color32, Rect, Sense, Stroke, Vec2};

use super::plot::{Density, FreqPlot, LogAxis};
use stave_measure::window::Window;

use super::spectral::{Analyzer, AnalyzerConfig, FLOOR_DB};
use crate::analysis::AnalysisRing;

const FFT_SIZES: &[usize] = &[1024, 2048, 4096, 8192];
const COLORS: [Color32; 2] = [
    Color32::from_rgb(120, 210, 255),
    Color32::from_rgb(255, 170, 110),
];
/// Peak-hold decay per frame.
const DECAY_DB: f32 = 0.6;

pub struct SpectrumState {
    analysis: AnalyzerConfig,
    peak_hold: bool,
    min_db: f32,
    max_db: f32,
    analyzer: Analyzer,
    /// Per-channel held peak (dB).
    peak: [Vec<f32>; 2],
}

impl Default for SpectrumState {
    fn default() -> Self {
        Self {
            analysis: AnalyzerConfig::default(),
            peak_hold: true,
            min_db: -90.0,
            max_db: 0.0,
            analyzer: Analyzer::new(AnalyzerConfig::default()),
            peak: [vec![FLOOR_DB; 1024], vec![FLOOR_DB; 1024]],
        }
    }
}

impl SpectrumState {
    fn reset_peak(&mut self) {
        for p in &mut self.peak {
            p.clear();
            p.resize(self.analysis.size / 2, FLOOR_DB);
        }
    }
}

/// Hold the max; otherwise fall `DECAY_DB` per frame (floored).
fn hold(peak: &mut [f32], now: &[f32], floor: f32) {
    for (p, &d) in peak.iter_mut().zip(now) {
        *p = if d > *p {
            d
        } else {
            (*p - DECAY_DB).max(floor)
        };
    }
}

pub fn ui(ui: &mut egui::Ui, ring: &AnalysisRing, rate: f32, state: &mut SpectrumState) {
    ui.horizontal(|ui| {
        ui.label("size");
        egui::ComboBox::from_id_source("fft_size")
            .selected_text(state.analysis.size.to_string())
            .show_ui(ui, |ui| {
                for &s in FFT_SIZES {
                    ui.selectable_value(&mut state.analysis.size, s, s.to_string());
                }
            });
        ui.label("window");
        egui::ComboBox::from_id_source("spectrum_window")
            .selected_text(state.analysis.window.name())
            .show_ui(ui, |ui| {
                for w in Window::ALL {
                    ui.selectable_value(&mut state.analysis.window, w, w.name());
                }
            });
        ui.separator();
        ui.checkbox(&mut state.peak_hold, "peak hold");
        if ui.button("reset").clicked() {
            state.reset_peak();
        }
        ui.separator();
        ui.label("range");
        ui.add(egui::Slider::new(&mut state.min_db, -120.0..=-20.0).suffix(" dB"));
        ui.add(egui::Slider::new(&mut state.max_db, -10.0..=24.0).suffix(" dB"));
    });
    if state.analyzer.config() != state.analysis {
        state.analyzer.configure(state.analysis);
        state.reset_peak();
    }
    let channels = ring.channels().min(2);
    if channels == 0 {
        return;
    }

    let avail = ui.available_rect_before_wrap();
    let size = Vec2::new(avail.width(), (avail.height() - 24.0).max(160.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(14));
    let plot = FreqPlot {
        rect: Rect::from_min_max(
            rect.min + Vec2::new(34.0, 6.0),
            rect.max - Vec2::new(6.0, 18.0),
        ),
        x: LogAxis::audio(rate),
        min_db: state.min_db,
        max_db: state.max_db,
    };
    plot.grid(&painter, Density::Sparse);
    for (ch, &color) in COLORS.iter().enumerate().take(channels) {
        let a = &mut state.analyzer;
        let db: Vec<f32> = a
            .ring_spectrum(ring, ch)
            .to_vec()
            .into_iter()
            .map(|b| a.amp_db(b))
            .collect();
        let points = |v: &[f32]| -> Vec<(f32, f32)> {
            v.iter()
                .enumerate()
                .skip(1)
                .map(|(i, &d)| (a.bin_hz(i, rate), d))
                .collect()
        };
        plot.curve(&painter, &points(&db), Stroke::new(1.3, color));
        if state.peak_hold {
            hold(&mut state.peak[ch], &db, state.min_db);
            let faint = Stroke::new(1.3, color.linear_multiply(0.45));
            plot.curve(&painter, &points(&state.peak[ch]), faint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_holds_then_decays_to_the_floor() {
        let mut p = vec![-10.0, -50.0];
        hold(&mut p, &[-20.0, -40.0], -60.0);
        assert_eq!(p, [-10.0 - DECAY_DB, -40.0]);
        for _ in 0..1000 {
            hold(&mut p, &[-120.0, -120.0], -60.0);
        }
        assert_eq!(p, [-60.0, -60.0]);
    }
}
