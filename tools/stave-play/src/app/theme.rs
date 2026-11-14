//! The workbench's look, from the stave design system's own tokens
//! (`stave_face::theme`): graphite for dark, cream for light, the
//! accent that means "active", the 4-pt spacing scale, and type sizes
//! on a small scale. Plots keep a dark screen in both, as the faces do.

use eframe::egui::{self, Color32, FontFamily, FontId, Rgba, Stroke, TextStyle};
use serde::{Deserialize, Serialize};
use stave_face::theme::Theme;
use stave_ui::style::Color;

/// What the user picked.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    /// Dark or light, as the OS is.
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Dark, ThemeChoice::Light];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Follow system",
            ThemeChoice::Dark => "Graphite (dark)",
            ThemeChoice::Light => "Cream (light)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "system" => Some(ThemeChoice::System),
            "dark" | "graphite" => Some(ThemeChoice::Dark),
            "light" | "cream" => Some(ThemeChoice::Light),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Light => "light",
        }
    }

    /// Dark or not, given what the OS says (none: dark).
    pub fn dark(self, system: Option<eframe::Theme>) -> bool {
        match self {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => system != Some(eframe::Theme::Light),
        }
    }
}

/// The colour roles the shell paints with (beyond egui's visuals).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tokens {
    /// The editor area.
    pub canvas: Color32,
    /// Side bars, the activity bar, the panel.
    pub chrome: Color32,
    /// The status bar.
    pub status: Color32,
    /// Raised controls.
    pub raised: Color32,
    /// Hairlines between regions.
    pub line: Color32,
    pub ink: Color32,
    pub ink_soft: Color32,
    pub accent: Color32,
    /// Text on the accent.
    pub on_accent: Color32,
    pub bad: Color32,
    pub good: Color32,
}

/// The spacing scale (points), from the design system: xs .. xxl.
pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

fn print(c: Color) -> Color32 {
    let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(c.r), byte(c.g), byte(c.b), byte(c.a))
}

fn paint(linear: [f64; 3]) -> Color32 {
    Color32::from(Rgba::from_rgb(linear[0] as f32, linear[1] as f32, linear[2] as f32))
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

impl Tokens {
    pub fn dark() -> Self {
        let t = Theme::graphite();
        let ink = print(t.palette.ink);
        Tokens {
            canvas: paint(t.finish.desk),
            chrome: paint(t.finish.plate),
            status: mix(paint(t.finish.plate), Color32::BLACK, 0.25),
            raised: paint(t.finish.surface),
            line: mix(paint(t.finish.plate), ink, 0.12),
            ink,
            ink_soft: mix(paint(t.finish.plate), ink, 0.6),
            accent: print(t.palette.accent),
            on_accent: Color32::from_rgb(0x1A, 0x14, 0x0A),
            bad: Color32::from_rgb(0xF2, 0x6D, 0x5B),
            good: Color32::from_rgb(0x7F, 0xC9, 0x7F),
        }
    }

    pub fn light() -> Self {
        let t = Theme::cream();
        let ink = print(t.palette.ink);
        let surface = paint(t.finish.surface);
        Tokens {
            canvas: mix(surface, Color32::WHITE, 0.55),
            chrome: mix(surface, Color32::WHITE, 0.2),
            status: paint(t.finish.plate),
            raised: mix(surface, Color32::WHITE, 0.75),
            line: mix(surface, ink, 0.18),
            ink,
            ink_soft: mix(surface, ink, 0.62),
            accent: print(t.palette.accent),
            on_accent: Color32::WHITE,
            bad: Color32::from_rgb(0xB0, 0x30, 0x24),
            good: Color32::from_rgb(0x2E, 0x7D, 0x32),
        }
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Dress `ctx` in the tokens (call when the resolved theme changes).
pub fn apply(ctx: &egui::Context, dark: bool) -> Tokens {
    let t = Tokens::for_dark(dark);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.chrome;
    v.window_fill = t.chrome;
    v.faint_bg_color = mix(t.chrome, t.ink, 0.04);
    v.extreme_bg_color = if dark { mix(t.canvas, Color32::BLACK, 0.35) } else { Color32::from_rgb(0xFC, 0xFA, 0xF5) };
    v.code_bg_color = v.extreme_bg_color;
    v.override_text_color = None;
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent.gamma_multiply(if dark { 0.35 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.window_stroke = Stroke::new(1.0, t.line);
    v.window_rounding = egui::Rounding::same(6.0);
    v.menu_rounding = egui::Rounding::same(6.0);
    v.error_fg_color = t.bad;
    v.warn_fg_color = t.accent;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.chrome;
    w.noninteractive.weak_bg_fill = t.chrome;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.ink);
    for (state, lift) in [(&mut w.inactive, 0.0), (&mut w.hovered, 0.08), (&mut w.active, 0.14)] {
        let fill = mix(t.raised, t.ink, lift);
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.fg_stroke = Stroke::new(1.0, t.ink);
        state.rounding = egui::Rounding::same(4.0);
    }
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.bg_stroke = Stroke::new(1.0, t.line);
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    w.open = w.active;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    let [xs, s, m, ..] = SPACE;
    style.spacing.item_spacing = egui::vec2(s, xs);
    style.spacing.button_padding = egui::vec2(s, 3.0);
    style.spacing.window_margin = egui::Margin::same(m);
    style.spacing.menu_margin = egui::Margin::same(xs);
    style.spacing.indent = m;
    style.spacing.interact_size.y = 22.0;
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_is_the_design_systems() {
        assert_eq!(Tokens::dark().accent, Color32::from_rgb(0xFF, 0xB2, 0x3F));
        assert_eq!(Tokens::light().accent, Color32::from_rgb(0xA9, 0x54, 0x2E));
    }

    #[test]
    fn ink_reads_against_its_ground() {
        // A crude luminance gap: ink and canvas sit at opposite ends.
        let lum = |c: Color32| 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
        for t in [Tokens::dark(), Tokens::light()] {
            assert!((lum(t.ink) - lum(t.canvas)).abs() > 140.0, "{t:?}");
            assert!((lum(t.ink_soft) - lum(t.chrome)).abs() > 60.0, "{t:?}");
        }
    }

    #[test]
    fn choices_resolve_against_the_system() {
        assert!(ThemeChoice::System.dark(None));
        assert!(!ThemeChoice::System.dark(Some(eframe::Theme::Light)));
        assert!(ThemeChoice::Dark.dark(Some(eframe::Theme::Light)));
        assert_eq!(ThemeChoice::parse("cream"), Some(ThemeChoice::Light));
    }
}
