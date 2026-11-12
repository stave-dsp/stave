//! Spectrogram: time × frequency × level (colour) of the plugin output.
//! One column per frame; the oldest scrolls off the left. **Hold**
//! freezes the buffer for static study of a finished run.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use super::plot::{hz_label, LogAxis};
use super::spectral::{Analyzer, AnalyzerConfig, FLOOR_DB};
use crate::analysis::AnalysisRing;

const SIZES: &[usize] = &[1024, 2048, 4096];

pub struct SpectrogramState {
    fft_size: usize,
    history: usize,
    min_db: f32,
    max_db: f32,
    hold: bool,
    log_freq: bool,
    analyzer: Analyzer,
    columns: Columns,
}

impl Default for SpectrogramState {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            history: 256,
            min_db: -84.0,
            max_db: 0.0,
            hold: false,
            log_freq: true,
            analyzer: Analyzer::new(AnalyzerConfig {
                size: 2048,
                ..AnalyzerConfig::default()
            }),
            columns: Columns::new(256, 1024),
        }
    }
}

/// A ring of spectrum columns (dB per bin), oldest first on read.
struct Columns {
    cols: Vec<Vec<f32>>,
    next: usize,
}

impl Columns {
    fn new(history: usize, bins: usize) -> Self {
        Self {
            cols: vec![vec![FLOOR_DB; bins]; history],
            next: 0,
        }
    }

    fn fits(&self, history: usize, bins: usize) -> bool {
        self.cols.len() == history && self.cols[0].len() == bins
    }

    /// The slot to overwrite with the newest column.
    fn push(&mut self) -> &mut Vec<f32> {
        let i = self.next;
        self.next = (self.next + 1) % self.cols.len();
        &mut self.cols[i]
    }

    /// Column `k` counting from the oldest.
    fn get(&self, k: usize) -> &[f32] {
        &self.cols[(self.next + k) % self.cols.len()]
    }
}

pub fn ui(ui: &mut egui::Ui, ring: &AnalysisRing, rate: f32, state: &mut SpectrogramState) {
    toolbar(ui, state);
    state.analyzer.configure(AnalyzerConfig {
        size: state.fft_size,
        ..state.analyzer.config()
    });
    let bins = state.fft_size / 2;
    if !state.columns.fits(state.history, bins) {
        state.columns = Columns::new(state.history, bins);
    }
    if !state.hold {
        let a = &mut state.analyzer;
        let spec = a.ring_spectrum(ring, 0).to_vec();
        let col = state.columns.push();
        for (c, b) in col.iter_mut().zip(spec) {
            *c = a.amp_db(b);
        }
    }

    let avail = ui.available_rect_before_wrap();
    let size = Vec2::new(avail.width(), (avail.height() - 12.0).max(220.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(8));
    let axis = LogAxis::audio(rate);
    let rows = rect.height().min(bins as f32) as usize;
    let (dx, dy) = (
        rect.width() / state.history as f32,
        rect.height() / rows as f32,
    );
    // Each pixel row's bin, computed once per frame, not per column.
    let row_bins: Vec<usize> = (0..rows)
        .map(|yi| {
            let t = 1.0 - (yi as f32 + 1.0) / rows as f32;
            let hz = if state.log_freq {
                axis.hz(t)
            } else {
                t * rate * 0.5
            };
            ((hz * state.fft_size as f32 / rate) as usize).clamp(1, bins - 1)
        })
        .collect();
    let range = state.max_db - state.min_db;
    for k in 0..state.history {
        let col = state.columns.get(k);
        let x = rect.min.x + k as f32 * dx;
        for (yi, &bin) in row_bins.iter().enumerate() {
            let color = colormap((col[bin] - state.min_db) / range);
            let cell = Rect::from_min_size(
                Pos2::new(x, rect.min.y + yi as f32 * dy),
                Vec2::new(dx + 1.0, dy + 1.0),
            );
            painter.rect_filled(cell, 0.0, color);
        }
    }
    for hz in [100.0_f32, 1_000.0, 10_000.0]
        .into_iter()
        .filter(|&hz| hz < rate * 0.5)
    {
        let t = if state.log_freq {
            axis.frac(hz)
        } else {
            hz / (rate * 0.5)
        };
        let y = rect.max.y - t * rect.height();
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(0.5, Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
        );
        painter.text(
            Pos2::new(rect.max.x - 3.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            hz_label(hz),
            FontId::monospace(10.0),
            Color32::from_gray(220),
        );
    }
    ui.label(if state.hold {
        "HELD — buffer static"
    } else {
        "running · one column per frame"
    });
}

fn toolbar(ui: &mut egui::Ui, state: &mut SpectrogramState) {
    ui.horizontal(|ui| {
        ui.label("fft");
        egui::ComboBox::from_id_source("spectro_fft")
            .selected_text(state.fft_size.to_string())
            .show_ui(ui, |ui| {
                for &n in SIZES {
                    ui.selectable_value(&mut state.fft_size, n, n.to_string());
                }
            });
        ui.separator();
        ui.label("history");
        ui.add(egui::Slider::new(&mut state.history, 64..=512));
        ui.separator();
        ui.checkbox(&mut state.hold, "hold")
            .on_hover_text("freeze the buffer for static inspection");
        if ui.button("clear").clicked() {
            state.columns = Columns::new(state.history, state.fft_size / 2);
        }
        ui.separator();
        ui.checkbox(&mut state.log_freq, "log freq");
        ui.add(egui::Slider::new(&mut state.min_db, -120.0..=-30.0).suffix(" dB"));
        ui.add(egui::Slider::new(&mut state.max_db, -12.0..=12.0).suffix(" dB"));
    });
    ui.add_space(4.0);
}

/// Sequential dark → indigo → teal → yellow ramp, `t` ∈ [0, 1].
fn colormap(t: f32) -> Color32 {
    const STOPS: [(f32, [u8; 3]); 5] = [
        (0.00, [12, 8, 30]),
        (0.25, [55, 22, 105]),
        (0.50, [35, 130, 155]),
        (0.75, [180, 200, 80]),
        (1.00, [255, 240, 130]),
    ];
    let t = t.clamp(0.0, 1.0);
    let i = STOPS
        .iter()
        .rposition(|(s, _)| *s <= t)
        .unwrap_or(0)
        .min(STOPS.len() - 2);
    let ((t0, c0), (t1, c1)) = (STOPS[i], STOPS[i + 1]);
    let f = (t - t0) / (t1 - t0);
    let mix =
        |k: usize| (f32::from(c0[k]) + f * (f32::from(c1[k]) - f32::from(c0[k]))).round() as u8;
    Color32::from_rgb(mix(0), mix(1), mix(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colormap_hits_its_stops_and_clamps() {
        assert_eq!(colormap(0.0), Color32::from_rgb(12, 8, 30));
        assert_eq!(colormap(0.5), Color32::from_rgb(35, 130, 155));
        assert_eq!(colormap(7.0), Color32::from_rgb(255, 240, 130));
        assert_eq!(colormap(-1.0), colormap(0.0));
    }

    #[test]
    fn columns_read_oldest_first() {
        let mut c = Columns::new(3, 1);
        for v in [1.0, 2.0, 3.0, 4.0] {
            c.push()[0] = v;
        }
        assert_eq!([c.get(0)[0], c.get(1)[0], c.get(2)[0]], [2.0, 3.0, 4.0]);
    }
}
