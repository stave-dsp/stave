//! Profile gallery: frequency-response curves of the loaded plugin AS
//! CURRENTLY SET, side by side. Run loads a fresh instance of the
//! bundle, copies the live controls onto it, and measures it offline
//! (`stave_host::response`: log sweep + H1) — deterministic, independent
//! of the audio device and the frame rate, and the live instance is
//! never disturbed.

mod card;

use std::path::Path;

use eframe::egui::{self, Color32, RichText, ScrollArea, Vec2};
use stave_host::config::HostConfig;
use stave_host::control::{Control, ControlError};
use stave_host::instance::Instance;
use stave_host::opened::{OpenError, Opened};
use stave_host::response::{self, ResponseConfig};

const MAX_PROFILES: usize = 24;
const COLORS: [Color32; 7] = [
    Color32::from_rgb(140, 230, 180),
    Color32::from_rgb(255, 200, 110),
    Color32::from_rgb(220, 130, 220),
    Color32::from_rgb(110, 200, 255),
    Color32::from_rgb(240, 240, 240),
    Color32::from_rgb(255, 120, 120),
    Color32::from_rgb(180, 250, 110),
];

/// One measured curve: (Hz, dB) points of the first output channel.
pub struct Profile {
    pub label: String,
    pub points: Vec<(f32, f32)>,
}

pub struct ProfilesState {
    profiles: Vec<Profile>,
    min_db: f32,
    max_db: f32,
    /// Fullscreen view of one profile.
    expanded: Option<usize>,
    error: Option<String>,
}

impl Default for ProfilesState {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            min_db: -48.0,
            max_db: 12.0,
            expanded: None,
            error: None,
        }
    }
}

/// What a run needs: the bundle, how to host it, the live controls.
pub struct Source<'a> {
    pub bundle: &'a Path,
    pub config: HostConfig,
    pub controls: Vec<Control>,
}

pub fn ui(ui: &mut egui::Ui, source: Source<'_>, state: &mut ProfilesState) {
    toolbar(ui, &source, state);
    ui.add_space(6.0);
    if let Some(e) = &state.error {
        ui.label(
            RichText::new(format!("run failed: {e}")).color(Color32::from_rgb(230, 110, 110)),
        );
    }
    if let Some(i) = state.expanded.filter(|&i| i < state.profiles.len()) {
        let p = &state.profiles[i];
        if card::expanded(
            ui,
            p,
            COLORS[i % COLORS.len()],
            (state.min_db, state.max_db),
        ) {
            state.expanded = None;
        }
        return;
    }
    state.expanded = None;
    if state.profiles.is_empty() {
        ui.label(
            RichText::new("(no profiles yet — set the controls, then Run profile)")
                .italics()
                .weak(),
        );
        return;
    }
    gallery(ui, state);
}

fn toolbar(ui: &mut egui::Ui, source: &Source<'_>, state: &mut ProfilesState) {
    ui.horizontal(|ui| {
        let run = egui::Button::new(RichText::new("⏺  Run profile").strong())
            .min_size(Vec2::new(160.0, 28.0))
            .fill(Color32::from_rgb(50, 110, 80));
        let hint = "measures a fresh instance with the current control values (2 s log sweep)";
        if ui.add(run).on_hover_text(hint).clicked() {
            match render_profile(source) {
                Ok(p) => {
                    state.profiles.push(p);
                    if state.profiles.len() > MAX_PROFILES {
                        state.profiles.remove(0);
                    }
                    state.error = None;
                }
                Err(e) => state.error = Some(crate::errors::with_causes(&e)),
            }
        }
        if ui.button("clear all").clicked() {
            state.profiles.clear();
        }
        if ui.button("remove last").clicked() {
            state.profiles.pop();
        }
        ui.separator();
        ui.add(egui::Slider::new(&mut state.min_db, -84.0..=-12.0).suffix(" dB"));
        ui.add(egui::Slider::new(&mut state.max_db, 0.0..=24.0).suffix(" dB"));
    });
}

fn gallery(ui: &mut egui::Ui, state: &mut ProfilesState) {
    let total_w = ui.available_width();
    let cols = if total_w > 1400.0 {
        3.0
    } else if total_w > 900.0 {
        2.0
    } else {
        1.0
    };
    let card_w = ((total_w - 24.0) / cols - 12.0).max(420.0);
    let size = Vec2::new(card_w, (card_w * 0.62).clamp(260.0, 420.0));
    let range = (state.min_db, state.max_db);
    let mut action = None;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, p) in state.profiles.iter().enumerate() {
                    match card::card(ui, p, size, COLORS[i % COLORS.len()], range) {
                        card::Action::None => {}
                        a => action = Some((i, a)),
                    }
                }
            });
        });
    match action {
        Some((i, card::Action::Remove)) => {
            state.profiles.remove(i);
        }
        Some((i, card::Action::Expand)) => state.expanded = Some(i),
        _ => {}
    }
}

#[derive(Debug, thiserror::Error)]
enum ProfileError {
    #[error(transparent)]
    Load(#[from] OpenError),
    #[error("could not copy a control onto the fresh instance")]
    Control(#[from] ControlError),
    #[error("plugin has no outputs")]
    NoOutputs,
}

fn render_profile(source: &Source<'_>) -> Result<Profile, ProfileError> {
    let mut inst = Opened::open(source.bundle, source.config)?.into_instance();
    let writable = source
        .controls
        .iter()
        .filter(|c| !c.flags.read_only && !c.flags.bypass);
    for c in writable {
        inst.set_control(&c.name, c.value)?;
    }
    let tf = response::measure(&mut inst, &ResponseConfig::default())
        .into_iter()
        .next()
        .ok_or(ProfileError::NoOutputs)?;
    let points = tf
        .bins
        .iter()
        .map(|b| (b.hz as f32, b.gain_db as f32))
        .collect();
    Ok(Profile {
        label: label(&source.controls),
        points,
    })
}

/// The settings that make this profile different: up to four controls
/// away from their defaults.
fn label(controls: &[Control]) -> String {
    let changed: Vec<String> = controls
        .iter()
        .filter(|c| !c.flags.hidden && (c.value - c.default).abs() > 1e-9)
        .take(4)
        .map(|c| format!("{}={}", c.leaf(), fmt_value(c.value)))
        .collect();
    if changed.is_empty() {
        "defaults".into()
    } else {
        changed.join("  ·  ")
    }
}

fn fmt_value(v: f64) -> String {
    match v.abs() {
        a if a >= 100.0 => format!("{v:.0}"),
        a if a >= 1.0 => format!("{v:.2}"),
        _ => format!("{v:.3}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stave_host::control::ControlFlags;

    fn control(name: &str, value: f64, default: f64) -> Control {
        Control {
            id: 0,
            name: name.into(),
            group: String::new(),
            unit: String::new(),
            min: -100.0,
            max: 100.0,
            default,
            value,
            normalized: 0.0,
            steps: 0,
            step_names: vec![],
            flags: ControlFlags::default(),
        }
    }

    #[test]
    fn label_names_only_changed_controls() {
        let cs = [
            control("Tone · tilt", 3.0, 0.0),
            control("output", 0.0, 0.0),
        ];
        assert_eq!(label(&cs), "tilt=3.00");
        assert_eq!(label(&cs[1..]), "defaults");
    }

    #[test]
    fn values_format_by_magnitude() {
        assert_eq!(
            (fmt_value(250.0), fmt_value(-3.5), fmt_value(0.25)),
            ("250".into(), "-3.50".into(), "0.250".into())
        );
    }
}
