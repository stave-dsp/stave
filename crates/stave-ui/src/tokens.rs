//! Design tokens — THE color/radius/elevation truth, in one
//! place. Platform-neutral so project compositions
//! and backends consume the same palette; `stave-vst3::theme` derives
//! its legacy tuples from these, never the reverse.

use crate::style::Color;

// ---------------------------------------------------------- editor canvas
// The logical design surface shared by plugin editors, previews, and
// screenshot baselines. Platform window chrome lives outside this canvas.
pub const EDITOR_CANVAS_WIDTH: f64 = 1024.0;
pub const EDITOR_CANVAS_HEIGHT: f64 = 720.0;
/// Primary shell chrome, including its one-point bottom divider.
pub const EDITOR_TOP_BAR_HEIGHT: f64 = 64.0;
/// Standard control height inside the primary shell bar.
pub const EDITOR_TOP_BAR_CONTROL_HEIGHT: f64 = 32.0;
/// Full preset navigator width: arrows, dropdown, and save action.
pub const EDITOR_PRESET_BAR_WIDTH: f64 = 384.0;

// ---------------------------------------------------------------- spacing
// The density scale. Layout paddings/gaps come
// from HERE — a literal spacing in widget or composition code is a
// drift hazard a grep check will grow to catch.
pub const SPACE_XS: f64 = 4.0;
pub const SPACE_SM: f64 = 8.0;
pub const SPACE_MD: f64 = 12.0;
pub const SPACE_LG: f64 = 18.0;
pub const SPACE_XL: f64 = 24.0;

// ---------------------------------------------------------------- motion
// Durations for animated affordances (meter falls, ring sweeps,
// page fades). Two speeds; if a third is ever needed, argue it here.
pub const DUR_FAST_MS: f64 = 120.0;
pub const DUR_SLOW_MS: f64 = 300.0;

// ------------------------------------------------------ interaction states
// State tints, applied by widgets over their base colors: hover
// lifts, drag lifts more, disabled fades. Roles, not per-widget
// choices.
pub const HOVER_LIFT: f64 = 0.06;
pub const DRAG_LIFT: f64 = 0.10;
pub const DISABLED_ALPHA: f64 = 0.4;

pub const BG_0: Color = Color::rgba(0.059, 0.067, 0.082, 1.0); // #0F1115
pub const BG_1: Color = Color::rgba(0.082, 0.094, 0.114, 1.0); // #15181D
pub const BG_2: Color = Color::rgba(0.106, 0.122, 0.149, 1.0); // #1B1F26
pub const LINE: Color = Color::rgba(0.180, 0.196, 0.224, 1.0); // #2E3239
pub const INK_MUTED: Color = Color::rgba(0.46, 0.50, 0.58, 1.0);
/// High-contrast ink for compact interactive shell controls.
pub const SHELL_CONTROL_INK: Color = Color::rgba(0.92, 0.94, 0.98, 1.0);
/// Structural edge for compact interactive shell controls.
pub const SHELL_CONTROL_LINE: Color = Color::rgba(0.38, 0.41, 0.48, 1.0);
/// Plot backdrop (transfer curves, scopes).
pub const PLOT_BG: Color = Color::rgba(0.07, 0.07, 0.09, 1.0);

/// Band identity palette (Low, Mid, High).
pub const BAND: [(f64, f64, f64); 3] = [(0.45, 0.68, 0.92), (0.55, 0.82, 0.66), (0.92, 0.68, 0.42)];

pub fn band_color(i: usize, alpha: f64) -> Color {
    let (r, g, b) = BAND[i.min(2)];
    Color::rgba(r, g, b, alpha)
}

pub const RADIUS_XS: f64 = 2.0;
pub const RADIUS_SM: f64 = 4.0;
pub const RADIUS_MD: f64 = 8.0;

/// Card elevation: (shadow dy, blur radius, opacity).
pub const ELEV_1: (f64, f64, f64) = (2.0, 8.0, 0.35);

/// Typography presets — the type scale as data. Names match intent,
/// not size; the platform maps `TypeRole` to concrete fonts.
pub mod text {
    use super::INK_MUTED;
    use crate::style::{Color, TextStyle, TypeRole};

    pub const WORDMARK: TextStyle = TextStyle {
        role: TypeRole::Display,
        size: 15.0,
        kern: 1.5,
        color: Color::rgba(0.94, 0.96, 1.00, 1.0),
    };
    pub const SECTION: TextStyle = TextStyle {
        role: TypeRole::Label,
        size: 13.0,
        kern: 2.4,
        color: Color::rgba(0.62, 0.66, 0.74, 1.0),
    };
    pub const TAGLINE: TextStyle = TextStyle {
        role: TypeRole::Label,
        size: 9.0,
        kern: 1.6,
        color: INK_MUTED,
    };
    pub const CAPTION: TextStyle = TextStyle {
        role: TypeRole::Tick,
        size: 9.0,
        kern: 1.6,
        color: INK_MUTED,
    };
    pub const PLOT_CAPTION: TextStyle = TextStyle {
        role: TypeRole::Tick,
        size: 9.0,
        kern: 1.2,
        color: INK_MUTED,
    };
    /// Card titles take the band accent as color at use site.
    pub const CARD_TITLE: TextStyle = TextStyle {
        role: TypeRole::Overlay,
        size: 11.0,
        kern: 2.0,
        color: Color::rgba(1.0, 1.0, 1.0, 1.0),
    };
}
