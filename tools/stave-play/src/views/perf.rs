//! Perf view — the CPU inspector (Instruments-shaped): a filled
//! load track with the realtime budget drawn ON it and dropout
//! ticks where callbacks overran; the stats row (median/p95/p99/
//! max, session deadline misses); the plugin's own inner time (the
//! wrapper's `cpu` tap) beside the host's outer time so wrapper +
//! host overhead is visible as the gap between them; a duration
//! histogram; and the pprof flamegraph capture for drilling into
//! WHERE the time goes.

use stave_measure::stats::Summary;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Color32, Stroke};
use stave_core::runtime::WrapperTaps;
use stave_telemetry::Telemetry;

use crate::debug_log::DebugLog;

#[derive(Default)]
pub struct PerfState {
    /// Most recent flamegraph capture status, mirrored to UI label.
    pub status: Arc<Mutex<String>>,
    /// True while a capture is in flight so we disable the button.
    pub busy: Arc<std::sync::atomic::AtomicBool>,
    /// Session peak of the plugin's inner cpu probe (GUI-sampled).
    pub probe_peak: f64,
}

pub struct PerfInfo<'a> {
    pub log: &'a Arc<DebugLog>,
    pub sample_rate: f32,
    pub telemetry: Option<&'a Telemetry>,
    pub state: &'a mut PerfState,
}

pub fn ui(ui: &mut egui::Ui, info: PerfInfo<'_>) {
    let log = info.log;
    let snap = log.perf_snapshot();
    let st = CallbackStats::of(&snap, info.sample_rate);

    // The plugin's INNER time, when the wrapper publishes it: the
    // gap to the host's outer numbers is wrapper + host overhead.
    let inner = info.telemetry.and_then(|t| t.value(WrapperTaps::CPU));
    if let Some(v) = inner {
        if v > info.state.probe_peak {
            info.state.probe_peak = v;
        }
    }

    // ---- the load track --------------------------------------------
    ui.label(egui::RichText::new("callback load (% of realtime budget)").weak());
    load_track(ui, &snap, info.sample_rate);
    ui.add_space(8.0);

    // ---- stats -----------------------------------------------------
    let misses = log.deadline_misses.load(Ordering::Relaxed);
    let latency = log.plugin_latency.load(Ordering::Relaxed);
    ui.horizontal(|ui| {
        egui::Grid::new("perf_stats_a")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                row(ui, "last", &fmt_us(st.last));
                row(ui, "avg", &fmt_us(st.us.mean as u32));
                row(ui, "p95", &fmt_us(st.us.p95 as u32));
                row(ui, "p99", &fmt_us(st.us.p99 as u32));
                row(ui, "max", &fmt_us(st.us.max as u32));
                row(
                    ui,
                    "session max",
                    &fmt_us(log.max_cb_us.load(Ordering::Relaxed)),
                );
            });
        ui.separator();
        egui::Grid::new("perf_stats_b")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                row(ui, "CPU (avg)", &format!("{:.1} %", st.cpu_pct));
                ui.label("deadline misses");
                if misses > 0 {
                    ui.monospace(
                        egui::RichText::new(format!("{misses}"))
                            .color(Color32::from_rgb(235, 90, 80)),
                    );
                } else {
                    ui.monospace("0");
                }
                ui.end_row();
                match inner {
                    Some(v) => {
                        row(ui, "plugin inner (probe)", &format!("{:.1} %", v * 100.0));
                        row(
                            ui,
                            "inner peak (session)",
                            &format!("{:.1} %", info.state.probe_peak * 100.0),
                        );
                    }
                    None => row(ui, "plugin inner (probe)", "—"),
                }
                row(
                    ui,
                    "plugin PDC",
                    &format!(
                        "{} samples ({:.2} ms)",
                        latency,
                        latency as f32 * 1000.0 / info.sample_rate.max(1.0)
                    ),
                );
            });
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("callback duration histogram").weak());
    histogram(ui, &snap, info.sample_rate);

    ui.add_space(12.0);
    ui.separator();

    let busy = info.state.busy.load(Ordering::Relaxed);
    let status_text = info
        .state
        .status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    ui.horizontal(|ui| {
        let btn = egui::Button::new(if busy {
            "capturing…"
        } else {
            "capture flamegraph (5 s)"
        });
        if ui.add_enabled(!busy, btn).clicked() {
            spawn_capture(info.state, log.clone());
        }
        if !status_text.is_empty() {
            ui.monospace(status_text);
        }
    });

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "flamegraph samples the whole process (audio callback + GUI \
             thread). Open the SVG in a browser to drill in.",
        )
        .small()
        .weak(),
    );
}

/// The Instruments-style track: filled per-callback load, colored by
/// severity, the 100% budget line drawn ON it, red ticks at dropouts.
fn load_track(ui: &mut egui::Ui, snap: &[(u32, u32)], sr: f32) {
    let h = 140.0;
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, Color32::from_rgb(16, 18, 22));
    if snap.is_empty() {
        return;
    }
    // Per-callback load fraction; y-scale covers at least 130%.
    let loads: Vec<f32> = snap
        .iter()
        .map(|&(us, frames)| {
            if frames == 0 {
                0.0
            } else {
                us as f32 / (frames as f32 * 1e6 / sr)
            }
        })
        .collect();
    let peak = loads.iter().cloned().fold(0.0f32, f32::max);
    let scale = peak.max(1.3);
    let n = loads.len();
    let dx = rect.width() / n as f32;
    let y_of = |load: f32| rect.bottom() - (load / scale).min(1.0) * (h - 6.0);
    for (i, &load) in loads.iter().enumerate() {
        let x = rect.left() + i as f32 * dx;
        let color = if load >= 1.0 {
            Color32::from_rgb(235, 90, 80)
        } else if load >= 0.5 {
            Color32::from_rgb(235, 180, 70)
        } else {
            Color32::from_rgb(90, 200, 140)
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, y_of(load)),
                egui::pos2(x + dx.max(1.0), rect.bottom()),
            ),
            0.0,
            color.gamma_multiply(0.75),
        );
        if load >= 1.0 {
            // Dropout tick at the top of the track.
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, rect.top()),
                    egui::pos2(x + dx.max(2.0), rect.top() + 5.0),
                ),
                0.0,
                Color32::from_rgb(255, 60, 50),
            );
        }
    }
    // Budget (100%) and half-budget guides.
    let y100 = y_of(1.0);
    painter.line_segment(
        [
            egui::pos2(rect.left(), y100),
            egui::pos2(rect.right(), y100),
        ],
        Stroke::new(1.0, Color32::from_rgb(235, 90, 80)),
    );
    let y50 = y_of(0.5);
    painter.line_segment(
        [egui::pos2(rect.left(), y50), egui::pos2(rect.right(), y50)],
        Stroke::new(1.0, Color32::from_gray(70)),
    );
    painter.text(
        egui::pos2(rect.right() - 4.0, y100 - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        "budget",
        egui::FontId::monospace(9.0),
        Color32::from_rgb(235, 90, 80),
    );
}

/// Duration histogram, log-scaled counts — the shape of the block
/// cost distribution (a healthy engine is one tight mode; a second
/// bump is a periodic spike worth hunting).
fn histogram(ui: &mut egui::Ui, snap: &[(u32, u32)], sr: f32) {
    let h = 64.0;
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, Color32::from_rgb(16, 18, 22));
    if snap.is_empty() {
        return;
    }
    const BINS: usize = 48;
    let max_us = snap.iter().map(|&(u, _)| u).max().unwrap_or(1).max(1);
    let mut bins = [0u32; BINS];
    for &(us, _) in snap {
        let b = ((us as usize * (BINS - 1)) / max_us as usize).min(BINS - 1);
        bins[b] += 1;
    }
    let max_count = bins.iter().copied().max().unwrap_or(1) as f32;
    let dx = rect.width() / BINS as f32;
    // Budget marker (the most recent callback's frames as the rep).
    let frames = snap.last().map(|&(_, f)| f).unwrap_or(0);
    let budget_us = if frames > 0 {
        frames as f32 * 1e6 / sr
    } else {
        0.0
    };
    for (i, &c) in bins.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let frac = ((c as f32).ln_1p() / max_count.ln_1p()).clamp(0.05, 1.0);
        let x = rect.left() + i as f32 * dx;
        let bin_us = (i as f32 + 0.5) / BINS as f32 * max_us as f32;
        let color = if budget_us > 0.0 && bin_us > budget_us {
            Color32::from_rgb(235, 90, 80)
        } else {
            Color32::from_rgb(120, 200, 240)
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, rect.bottom() - frac * (h - 4.0)),
                egui::pos2(x + (dx - 1.0).max(1.0), rect.bottom()),
            ),
            0.0,
            color.gamma_multiply(0.8),
        );
    }
    if budget_us > 0.0 && budget_us < max_us as f32 {
        let x = rect.left() + budget_us / max_us as f32 * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgb(235, 90, 80)),
        );
    }
}

/// The callback history, summarized: µs per callback (shared
/// percentile definition with the bench) and average CPU load.
struct CallbackStats {
    last: u32,
    us: Summary,
    /// Total callback time over the realtime span it covered, %.
    cpu_pct: f64,
}

impl CallbackStats {
    /// `snap`: chronological (µs, frames) per callback.
    fn of(snap: &[(u32, u32)], rate: f32) -> Self {
        let us: Vec<f64> = snap.iter().map(|&(u, _)| f64::from(u)).collect();
        let total_us: f64 = us.iter().sum();
        let realtime_us =
            snap.iter().map(|&(_, f)| f64::from(f)).sum::<f64>() * 1e6 / f64::from(rate.max(1.0));
        Self {
            last: snap.last().map_or(0, |&(u, _)| u),
            us: Summary::of(&us).unwrap_or(Summary {
                count: 0,
                mean: 0.0,
                min: 0.0,
                median: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
            }),
            cpu_pct: if realtime_us > 0.0 {
                total_us / realtime_us * 100.0
            } else {
                0.0
            },
        }
    }
}

fn row(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(k);
    ui.monospace(v);
    ui.end_row();
}

fn fmt_us(us: u32) -> String {
    if us >= 1000 {
        format!("{:.2} ms", us as f32 / 1000.0)
    } else {
        format!("{} μs", us)
    }
}

/// pprof samples with signals: unix only.
#[cfg(not(unix))]
fn spawn_capture(state: &mut PerfState, log: Arc<DebugLog>) {
    log.error("flamegraph: sampling needs a unix (pprof)".to_string());
    if let Ok(mut g) = state.status.lock() {
        *g = "flamegraph capture is unix-only".to_string();
    }
}

#[cfg(unix)]
fn spawn_capture(state: &mut PerfState, log: Arc<DebugLog>) {
    state.busy.store(true, Ordering::Relaxed);
    let status = state.status.clone();
    let busy = state.busy.clone();
    std::thread::spawn(move || {
        if let Ok(mut g) = status.lock() {
            *g = "sampling… 5 s".to_string();
        }
        let guard = match pprof::ProfilerGuardBuilder::default()
            .frequency(500)
            .blocklist(&["libc", "libgcc", "pthread"])
            .build()
        {
            Ok(g) => g,
            Err(e) => {
                log.error(format!("flamegraph: ProfilerGuard failed: {}", e));
                if let Ok(mut g) = status.lock() {
                    *g = format!("error: {}", e);
                }
                busy.store(false, Ordering::Relaxed);
                return;
            }
        };
        std::thread::sleep(Duration::from_secs(5));
        let report = match guard.report().build() {
            Ok(r) => r,
            Err(e) => {
                log.error(format!("flamegraph: report failed: {}", e));
                if let Ok(mut g) = status.lock() {
                    *g = format!("error: {}", e);
                }
                busy.store(false, Ordering::Relaxed);
                return;
            }
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = format!("/tmp/stave-flame-{}.svg", ts);
        match std::fs::File::create(&path) {
            Ok(mut f) => {
                if let Err(e) = report.flamegraph(&mut f) {
                    log.error(format!("flamegraph: SVG write failed: {}", e));
                    if let Ok(mut g) = status.lock() {
                        *g = format!("error: {}", e);
                    }
                    busy.store(false, Ordering::Relaxed);
                    return;
                }
            }
            Err(e) => {
                log.error(format!("flamegraph: file create failed: {}", e));
                if let Ok(mut g) = status.lock() {
                    *g = format!("error: {}", e);
                }
                busy.store(false, Ordering::Relaxed);
                return;
            }
        }
        log.info(format!("flamegraph SVG: {}", &path));
        if let Ok(mut g) = status.lock() {
            *g = format!("wrote {}", &path);
        }
        busy.store(false, Ordering::Relaxed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_of_a_callback_history() {
        // 4 callbacks of 480 frames at 48 kHz (10 ms each), 1–4 ms spent.
        let snap = [(1000, 480), (2000, 480), (4000, 480), (3000, 480)];
        let st = CallbackStats::of(&snap, 48_000.0);
        assert_eq!((st.last, st.us.max, st.us.mean), (3000, 4000.0, 2500.0));
        assert!((st.cpu_pct - 25.0).abs() < 1e-9);
    }

    #[test]
    fn empty_history_is_zeroes() {
        let st = CallbackStats::of(&[], 48_000.0);
        assert_eq!((st.last, st.us.max, st.cpu_pct), (0, 0.0, 0.0));
    }
}
