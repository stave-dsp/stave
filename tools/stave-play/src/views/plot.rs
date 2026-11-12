//! Shared plotting for frequency-domain views: a log-frequency axis, a
//! dB axis, their grids, and a curve with a hover readout. Every view
//! that plots against frequency maps through [`LogAxis`] — one
//! definition of "where does 1 kHz go".

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke};

/// Frequency ↔ position on a log scale over `[lo, hi]` Hz.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LogAxis {
    log_lo: f32,
    log_hi: f32,
}

impl LogAxis {
    pub fn new(lo_hz: f32, hi_hz: f32) -> Self {
        Self {
            log_lo: lo_hz.max(1e-3).log10(),
            log_hi: hi_hz.max(lo_hz * 1.0001).log10(),
        }
    }

    /// The audible band's floor up to Nyquist.
    pub fn audio(rate: f32) -> Self {
        Self::new(stave_measure::AUDIBLE_MIN_HZ as f32, rate * 0.5)
    }

    /// Position of `hz` in [0, 1] (unclamped outside the band).
    pub fn frac(&self, hz: f32) -> f32 {
        (hz.max(1e-3).log10() - self.log_lo) / (self.log_hi - self.log_lo)
    }

    /// The frequency at position `t` ∈ [0, 1].
    pub fn hz(&self, t: f32) -> f32 {
        10f32.powf(self.log_lo + t * (self.log_hi - self.log_lo))
    }

    pub fn contains(&self, hz: f32) -> bool {
        (0.0..=1.0).contains(&self.frac(hz))
    }
}

/// "1k", "200", "20k".
pub fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.0}k", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

/// A frequency (x, log) × dB (y, linear) plot area.
#[derive(Copy, Clone, Debug)]
pub struct FreqPlot {
    pub rect: Rect,
    pub x: LogAxis,
    pub min_db: f32,
    pub max_db: f32,
}

/// How much grid to draw.
#[derive(Copy, Clone, PartialEq)]
pub enum Density {
    Sparse,
    Dense,
}

impl FreqPlot {
    pub fn pos(&self, hz: f32, db: f32) -> Pos2 {
        let tx = self.x.frac(hz).clamp(0.0, 1.0);
        let ty = ((db - self.min_db) / (self.max_db - self.min_db)).clamp(0.0, 1.0);
        Pos2::new(
            self.rect.min.x + tx * self.rect.width(),
            self.rect.max.y - ty * self.rect.height(),
        )
    }

    /// The frequency under screen x.
    pub fn hz_at(&self, x: f32) -> f32 {
        self.x.hz((x - self.rect.min.x) / self.rect.width())
    }

    pub fn grid(&self, painter: &Painter, density: Density) {
        let (freqs, step, font): (&[f32], i32, f32) = match density {
            Density::Sparse => (&[100.0, 1_000.0, 10_000.0], 6, 9.0),
            Density::Dense => (
                &[20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 1e4, 2e4],
                3,
                11.0,
            ),
        };
        let label = Color32::from_gray(160);
        for &hz in freqs.iter().filter(|&&hz| self.x.contains(hz)) {
            let x = self.pos(hz, self.min_db).x;
            painter.line_segment(
                [Pos2::new(x, self.rect.min.y), Pos2::new(x, self.rect.max.y)],
                Stroke::new(0.4, Color32::from_gray(38)),
            );
            painter.text(
                Pos2::new(x, self.rect.max.y + 3.0),
                Align2::CENTER_TOP,
                hz_label(hz),
                FontId::monospace(font),
                label,
            );
        }
        let first = (self.min_db / step as f32).ceil() as i32 * step;
        for db in (first..=self.max_db as i32).step_by(step as usize) {
            let y = self.pos(1_000.0, db as f32).y;
            let zero = db == 0;
            painter.line_segment(
                [Pos2::new(self.rect.min.x, y), Pos2::new(self.rect.max.x, y)],
                Stroke::new(
                    if zero { 0.9 } else { 0.3 },
                    Color32::from_gray(if zero { 95 } else { 30 }),
                ),
            );
            if db % 6 == 0 {
                painter.text(
                    Pos2::new(self.rect.min.x - 5.0, y),
                    Align2::RIGHT_CENTER,
                    db.to_string(),
                    FontId::monospace(font),
                    label,
                );
            }
        }
    }

    /// A polyline through `(hz, dB)` points (out-of-band points skipped).
    pub fn curve(&self, painter: &Painter, points: &[(f32, f32)], stroke: Stroke) {
        let line: Vec<Pos2> = points
            .iter()
            .filter(|(hz, _)| self.x.contains(*hz))
            .map(|&(hz, db)| self.pos(hz, db))
            .collect();
        painter.add(eframe::egui::Shape::line(line, stroke));
    }

    /// Crosshair + "1.20 kHz  −3.1 dB" at the point nearest `hover`.
    pub fn readout(&self, painter: &Painter, points: &[(f32, f32)], hover: Pos2, color: Color32) {
        if !self.rect.contains(hover) {
            return;
        }
        let target = self.x.frac(self.hz_at(hover.x));
        let nearest = points
            .iter()
            .filter(|(hz, _)| self.x.contains(*hz))
            .min_by(|a, b| {
                (self.x.frac(a.0) - target)
                    .abs()
                    .total_cmp(&(self.x.frac(b.0) - target).abs())
            });
        let Some(&(hz, db)) = nearest else { return };
        let p = self.pos(hz, db);
        painter.line_segment(
            [
                Pos2::new(p.x, self.rect.min.y),
                Pos2::new(p.x, self.rect.max.y),
            ],
            Stroke::new(0.5, Color32::from_rgba_unmultiplied(255, 255, 255, 80)),
        );
        painter.circle_filled(p, 4.0, color);
        let text = if hz >= 1000.0 {
            format!("{:.2} kHz   {db:.1} dB", hz / 1000.0)
        } else {
            format!("{hz:.0} Hz   {db:.1} dB")
        };
        let at = Pos2::new(
            (p.x + 8.0).min(self.rect.max.x - 120.0),
            (p.y - 18.0).max(self.rect.min.y + 4.0),
        );
        painter.text(
            at,
            Align2::LEFT_TOP,
            text,
            FontId::monospace(12.0),
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_axis_round_trips_and_spaces_decades_evenly() {
        let a = LogAxis::new(20.0, 20_000.0);
        assert_eq!((a.frac(20.0), a.frac(20_000.0)), (0.0, 1.0));
        assert!((a.frac(200.0) - 1.0 / 3.0).abs() < 1e-6);
        assert!((a.frac(2_000.0) - a.frac(200.0) - 1.0 / 3.0).abs() < 1e-6);
        assert!((a.hz(a.frac(1_234.0)) - 1_234.0).abs() < 0.05);
        assert!(!a.contains(10.0) && a.contains(1_000.0));
    }

    #[test]
    fn plot_maps_corners() {
        let p = FreqPlot {
            rect: Rect::from_min_max(Pos2::new(10.0, 0.0), Pos2::new(110.0, 60.0)),
            x: LogAxis::new(20.0, 20_000.0),
            min_db: -60.0,
            max_db: 0.0,
        };
        assert_eq!(p.pos(20.0, -60.0), Pos2::new(10.0, 60.0));
        assert_eq!(p.pos(20_000.0, 0.0), Pos2::new(110.0, 0.0));
        assert_eq!(p.pos(1e6, 99.0), Pos2::new(110.0, 0.0), "clamped");
        assert!((p.hz_at(10.0) - 20.0).abs() < 1e-3);
    }

    #[test]
    fn labels() {
        assert_eq!(
            (hz_label(200.0), hz_label(2_000.0)),
            ("200".into(), "2k".into())
        );
    }
}
