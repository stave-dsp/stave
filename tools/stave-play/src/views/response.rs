//! Live transfer function: |H(f)| = |FFT(out)| / |FFT(in)| over the
//! newest window of the input and output taps, smoothed over frames.
//! Bins whose input is below a floor keep their last value instead of
//! smearing into noise when the stimulus goes quiet. (For a measured,
//! deterministic curve use the Profiles tab.)

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use stave_dsp::primitives::spectral::Complex;

use super::plot::{Density, FreqPlot, LogAxis};
use super::spectral::{Analyzer, AnalyzerConfig, FLOOR_DB};
use crate::analysis::AnalysisRing;

const SIZES: &[usize] = &[2048, 4096, 8192];
/// Input bins weaker than this (|X|, unscaled) are not updated.
const INPUT_FLOOR: f32 = 1e-3;
const LIVE: Color32 = Color32::from_rgb(140, 230, 180);
const SNAPSHOT_COLORS: [Color32; 5] = [
    Color32::from_rgb(255, 200, 110),
    Color32::from_rgb(220, 130, 220),
    Color32::from_rgb(110, 200, 255),
    Color32::from_rgb(240, 240, 240),
    Color32::from_rgb(255, 120, 120),
];

struct Snapshot {
    label: String,
    points: Vec<(f32, f32)>,
}

pub struct ResponseState {
    size: usize,
    min_db: f32,
    max_db: f32,
    smooth: f32,
    input: Analyzer,
    output: Analyzer,
    /// Smoothed |H| per bin, dB.
    avg_db: Vec<f32>,
    snapshots: Vec<Snapshot>,
}

impl Default for ResponseState {
    fn default() -> Self {
        Self {
            size: 4096,
            min_db: -36.0,
            max_db: 18.0,
            smooth: 0.85,
            input: Analyzer::new(AnalyzerConfig {
                size: 4096,
                ..AnalyzerConfig::default()
            }),
            output: Analyzer::new(AnalyzerConfig {
                size: 4096,
                ..AnalyzerConfig::default()
            }),
            avg_db: vec![0.0; 2048],
            snapshots: Vec::new(),
        }
    }
}

/// One frame of the estimate: per bin, |Y|/|X| in dB where the input
/// is strong enough, folded into `avg` with weight `alpha`.
fn update(avg: &mut [f32], x: &[Complex<f32>], y: &[Complex<f32>], alpha: f32) {
    for ((a, xi), yi) in avg.iter_mut().zip(x).zip(y).skip(1) {
        if xi.norm() < INPUT_FLOOR {
            continue;
        }
        let h = yi.norm() / xi.norm();
        let db = if h <= 1e-6 {
            FLOOR_DB
        } else {
            20.0 * h.log10()
        };
        *a += alpha * (db - *a);
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    input: &AnalysisRing,
    output: &AnalysisRing,
    rate: f32,
    state: &mut ResponseState,
) {
    toolbar(ui, rate, state);
    if state.input.size() != state.size {
        state.input.configure(AnalyzerConfig {
            size: state.size,
            ..state.input.config()
        });
        state.output.configure(AnalyzerConfig {
            size: state.size,
            ..state.output.config()
        });
        state.avg_db = vec![0.0; state.size / 2];
    }
    let x = state.input.ring_spectrum(input, 0).to_vec();
    let y = state.output.ring_spectrum(output, 0);
    update(
        &mut state.avg_db,
        &x,
        y,
        1.0 - state.smooth.clamp(0.0, 0.98),
    );

    let avail = ui.available_rect_before_wrap();
    let size = Vec2::new(avail.width(), (avail.height() - 12.0).max(180.0));
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
    for (i, s) in state.snapshots.iter().enumerate() {
        let color = SNAPSHOT_COLORS[i % SNAPSHOT_COLORS.len()];
        plot.curve(&painter, &s.points, Stroke::new(1.4, color));
        let y = rect.min.y + 18.0 + i as f32 * 14.0;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(rect.max.x - 110.0, y), Vec2::new(16.0, 8.0)),
            1.0,
            color,
        );
        painter.text(
            Pos2::new(rect.max.x - 88.0, y + 4.0),
            Align2::LEFT_CENTER,
            &s.label,
            FontId::monospace(10.0),
            color,
        );
    }
    plot.curve(
        &painter,
        &points(&state.avg_db, &state.input, rate),
        Stroke::new(1.6, LIVE),
    );
    ui.label("drive it with the log-sweep sample for a clean readout");
}

fn points(db: &[f32], a: &Analyzer, rate: f32) -> Vec<(f32, f32)> {
    db.iter()
        .enumerate()
        .skip(1)
        .map(|(i, &d)| (a.bin_hz(i, rate), d))
        .collect()
}

fn toolbar(ui: &mut egui::Ui, rate: f32, state: &mut ResponseState) {
    ui.horizontal(|ui| {
        ui.label("fft");
        egui::ComboBox::from_id_source("resp_fft")
            .selected_text(state.size.to_string())
            .show_ui(ui, |ui| {
                for &n in SIZES {
                    ui.selectable_value(&mut state.size, n, n.to_string());
                }
            });
        ui.separator();
        ui.label("smoothing");
        ui.add(egui::Slider::new(&mut state.smooth, 0.0..=0.98));
        ui.separator();
        ui.add(egui::Slider::new(&mut state.min_db, -60.0..=-6.0).suffix(" dB"));
        ui.add(egui::Slider::new(&mut state.max_db, 0.0..=24.0).suffix(" dB"));
        ui.separator();
        if ui
            .button("freeze")
            .on_hover_text("keep the current curve as an overlay")
            .clicked()
        {
            state.snapshots.push(Snapshot {
                label: format!("snap {}", state.snapshots.len() + 1),
                points: points(&state.avg_db, &state.input, rate),
            });
        }
        if ui.button("clear snapshots").clicked() {
            state.snapshots.clear();
        }
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_bins_converge_and_weak_bins_hold() {
        let x = [
            Complex::new(1.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(1e-5, 0.0),
        ];
        let y = [
            Complex::new(9.0, 0.0),
            Complex::new(0.5, 0.0),
            Complex::new(1.0, 0.0),
        ];
        let mut avg = [7.0, 0.0, 3.0];
        for _ in 0..200 {
            update(&mut avg, &x, &y, 0.1);
        }
        assert_eq!(avg[0], 7.0, "DC bin is skipped");
        assert!((avg[1] + 6.0206).abs() < 1e-3, "{}", avg[1]);
        assert_eq!(avg[2], 3.0, "input below the floor: held");
    }
}
