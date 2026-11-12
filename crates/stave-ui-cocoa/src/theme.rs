//! Centralised design tokens for the Stave editor UI.
//!
//! Pure-data layer: no AppKit, no objc — just sRGB tuples, f64 sizes,
//! and f64 weights. The editor wraps these with `ns_color` / `ns_font_w`
//! at call sites. Keeping it ABI-free means tests and the host-cli can
//! consume the same palette for spectrum/curve renderings.
//!
//! Influences (synthesized from Baby Audio + Klevgrand survey):
//! - Dark surface with a single bright accent (BA-1, TAIP, Smooth Op).
//! - Per-domain accents only where domain identity matters — for the
//!   multiband, the three bands need to stay color-coded so the eye
//!   can lock on to LOW/MID/HIGH at a glance.
//! - Uppercase tracked LABELS, mixed-case VALUES — the "data sheet"
//!   pattern from Crystalline / R0verb.
//! - Generous whitespace, soft elevation (Klevgrand surfaces).
//! - SF Mono tabular digits everywhere a number changes in real time.

/// sRGB (red, green, blue, alpha) in 0..=1 — what NSColor's
/// `colorWithSRGBRed:green:blue:alpha:` consumes directly.
pub type Rgba = (f64, f64, f64, f64);

/// Colors that ALSO exist as stave-ui tokens derive from them —
/// tokens are the single truth; this file only adds cocoa-side
/// extras (font roles, elevation shorthands, ink shades).
const fn from_token(c: stave_ui::Color) -> Rgba {
    (c.r, c.g, c.b, c.a)
}

/// sRGB triple without alpha — for cases where the call site supplies
/// its own alpha (e.g. band-accent tints at varying opacity).
pub type Rgb = (f64, f64, f64);

// ------------------------------------------------------------------
// PALETTE — neutrals
// ------------------------------------------------------------------

/// Deepest background. The "outside" of every panel.
pub const BG_0: Rgba = from_token(stave_ui::tokens::BG_0); // #0F1115

/// Root background gradient — a soft vertical slate wash (lifted at the top,
/// deeper at the bottom) instead of a flat fill, for modern depth.
pub const BG_GRAD_TOP: Rgba = (0.107, 0.128, 0.166, 1.0); // ~#1B2129 slate
pub const BG_GRAD_BOTTOM: Rgba = (0.043, 0.051, 0.067, 1.0); // ~#0B0D11 deep

/// Edge vignette — black layered toward the corners at this alpha to give the
/// surface a soft, recessed "shadow" feel.
pub const VIGNETTE: Rgba = (0.0, 0.0, 0.0, 0.22);

/// Primary surface — what most cards / panels sit on.
pub const BG_1: Rgba = from_token(stave_ui::tokens::BG_1); // #15181D

/// Raised surface — for cards/elevated controls that should pop one
/// step above the surface they're on.
pub const BG_2: Rgba = from_token(stave_ui::tokens::BG_2); // #1B1F26

/// Subtle hairline divider tone.
pub const LINE: Rgba = from_token(stave_ui::tokens::LINE); // #2E3239

/// Visualisation gridline / axis tone — slightly brighter than LINE so
/// it reads inside a recessed BG_0 panel without disappearing.
pub const AXIS: Rgba = (0.275, 0.290, 0.330, 1.0); // #464A55

// ------------------------------------------------------------------
// PALETTE — ink (text on dark surfaces)
// ------------------------------------------------------------------

/// Headline-grade text. Use sparingly — the eye locks here first.
pub const INK_HI: Rgba = (0.94, 0.96, 1.00, 1.0);

/// Body / value text.
pub const INK_MD: Rgba = (0.82, 0.85, 0.92, 1.0);

/// De-emphasised text (units, hints, captions).
pub const INK_LO: Rgba = (0.62, 0.67, 0.76, 1.0);

/// "Almost off" — disabled labels, ghost values, axis ticks.
pub const INK_MUTED: Rgba = from_token(stave_ui::tokens::INK_MUTED);

/// High-contrast ink for compact shell controls. Brighter than body copy,
/// but deliberately below headline white so the product wordmark still leads.
pub const SHELL_CONTROL_INK: Rgba = from_token(stave_ui::tokens::SHELL_CONTROL_INK);

/// Structural edge for compact shell controls. Ordinary panel hairlines are
/// intentionally quieter; interactive chrome needs a more legible boundary.
pub const SHELL_CONTROL_LINE: Rgba = from_token(stave_ui::tokens::SHELL_CONTROL_LINE);

// ------------------------------------------------------------------
// PALETTE — accents
// ------------------------------------------------------------------

/// Global brand accent — wordmark, focus rings, primary CTAs.
pub const ACCENT: Rgba = (0.55, 0.78, 0.95, 1.0);

/// Secondary accent — secondary CTAs, link states, "on" indicators.
pub const ACCENT_2: Rgba = (0.72, 0.80, 0.92, 1.0);

/// Warning / clip / bypassed states.
pub const WARN: Rgba = (0.92, 0.46, 0.42, 1.0);

/// Soft warning (just-too-hot).
pub const WARN_SOFT: Rgba = (0.90, 0.66, 0.40, 1.0);

// ------------------------------------------------------------------
// PALETTE — band identity (multiband)
//
// Three perceptually-distinct hues. They're the only multi-color
// dimension in the editor; everything else hews to the neutral ramp.
// ------------------------------------------------------------------

pub const BAND_LOW: Rgb = (
    stave_ui::tokens::BAND[0].0,
    stave_ui::tokens::BAND[0].1,
    stave_ui::tokens::BAND[0].2,
); // blue
pub const BAND_MID: Rgb = (
    stave_ui::tokens::BAND[1].0,
    stave_ui::tokens::BAND[1].1,
    stave_ui::tokens::BAND[1].2,
); // green
pub const BAND_HIGH: Rgb = (
    stave_ui::tokens::BAND[2].0,
    stave_ui::tokens::BAND[2].1,
    stave_ui::tokens::BAND[2].2,
); // orange

/// All three band tints in a fixed [low, mid, high] order so callers
/// can iterate in band-index order without rebuilding the tuple.
pub const BAND_PALETTE: [Rgb; 3] = [BAND_LOW, BAND_MID, BAND_HIGH];

/// Promote a band Rgb to a full Rgba.
#[inline]
pub const fn band_rgba(rgb: Rgb, a: f64) -> Rgba {
    (rgb.0, rgb.1, rgb.2, a)
}

/// Convert a band tint to a u8 RGB tuple for raw pixel-buffer rendering
/// (spectrum bars, transfer curves). `idx` ∈ 0..=2 picks low / mid / high.
#[inline]
pub fn band_u8(idx: usize) -> (u8, u8, u8) {
    let c = BAND_PALETTE[idx.min(2)];
    (
        (c.0 * 255.0) as u8,
        (c.1 * 255.0) as u8,
        (c.2 * 255.0) as u8,
    )
}

/// Brightened "hot" variant of a band tint — used when a spectrum bar
/// crosses its threshold so the over-threshold portion pops. Lifts each
/// channel toward 1.0 by 65%, capped at 255.
#[inline]
pub fn band_hot_u8(idx: usize) -> (u8, u8, u8) {
    let c = BAND_PALETTE[idx.min(2)];
    let lift = |k: f64| -> u8 {
        let v = k + (1.0 - k) * 0.65;
        (v.clamp(0.0, 1.0) * 255.0) as u8
    };
    (lift(c.0), lift(c.1), lift(c.2))
}

/// Convert any `Rgba` token to (u8, u8, u8, u8) for raw pixel-buffer use.
#[inline]
pub fn rgba_u8(c: Rgba) -> (u8, u8, u8, u8) {
    (
        (c.0 * 255.0) as u8,
        (c.1 * 255.0) as u8,
        (c.2 * 255.0) as u8,
        (c.3 * 255.0) as u8,
    )
}

// ------------------------------------------------------------------
// TYPE RAMP — sizes (pt)
// ------------------------------------------------------------------

/// Wordmark / hero numeric. Use for the brand mark and the single
/// biggest readout on screen.
pub const SIZE_DISPLAY: f64 = 17.0;

/// Section heading (e.g. "DETECTION", "OUT").
pub const SIZE_HEADING: f64 = 14.0;

/// Small label / caption — the uppercase tracked tier.
pub const SIZE_LABEL: f64 = 9.0;

/// Card / group title.
pub const SIZE_GROUP: f64 = 10.0;

/// Live value readout under sliders.
pub const SIZE_VALUE: f64 = 10.5;

/// In-curve / in-spectrum annotations (e.g. live GR overlay).
pub const SIZE_OVERLAY: f64 = 12.0;

/// Axis tick / inline tag.
pub const SIZE_TICK: f64 = 8.5;

// ------------------------------------------------------------------
// TYPE RAMP — weights (NSFontWeight CGFloat values)
// ------------------------------------------------------------------

pub const W_ULTRALIGHT: f64 = -0.8;
pub const W_THIN: f64 = -0.6;
pub const W_LIGHT: f64 = -0.4;
pub const W_REGULAR: f64 = 0.0;
pub const W_MEDIUM: f64 = 0.23;
pub const W_SEMIBOLD: f64 = 0.3;
pub const W_BOLD: f64 = 0.4;
pub const W_HEAVY: f64 = 0.56;
pub const W_BLACK: f64 = 0.62;

// ------------------------------------------------------------------
// TYPE RAMP — semantic role helpers
//
// (size, weight) tuples for the canonical roles. Call sites pass
// `theme::role_wordmark()` to a font helper instead of restating sizes.
// ------------------------------------------------------------------

#[inline]
pub const fn role_wordmark() -> (f64, f64) {
    (SIZE_DISPLAY, W_HEAVY)
}
#[inline]
pub const fn role_section() -> (f64, f64) {
    (SIZE_HEADING, W_LIGHT)
}
#[inline]
pub const fn role_label() -> (f64, f64) {
    (SIZE_LABEL, W_SEMIBOLD)
}
#[inline]
pub const fn role_label_heavy() -> (f64, f64) {
    (SIZE_LABEL, W_HEAVY)
}
#[inline]
pub const fn role_group() -> (f64, f64) {
    (SIZE_GROUP, W_HEAVY)
}
#[inline]
pub const fn role_value() -> (f64, f64) {
    (SIZE_VALUE, W_MEDIUM)
}
#[inline]
pub const fn role_overlay() -> (f64, f64) {
    (SIZE_OVERLAY, W_HEAVY)
}
#[inline]
pub const fn role_tick() -> (f64, f64) {
    (SIZE_TICK, W_SEMIBOLD)
}

/// Tracking (NSKern) for the canonical label tier — uppercase tracked.
/// Positive values widen letter spacing; +0.8 reads as a confident
/// "spec sheet" label without crossing into novelty.
pub const TRACK_LABEL: f64 = 0.8;

/// Tracking for the wordmark — slightly negative to tighten the heavy
/// glyphs into a single mark.
pub const TRACK_WORDMARK: f64 = -0.4;

// ------------------------------------------------------------------
// SPACING — 4 / 8 / 12 / 16 / 24 / 32 / 48
// ------------------------------------------------------------------

pub const SPACE_XXS: f64 = 4.0;
pub const SPACE_XS: f64 = 8.0;
pub const SPACE_SM: f64 = 12.0;
pub const SPACE_MD: f64 = 16.0;
pub const SPACE_LG: f64 = 24.0;
pub const SPACE_XL: f64 = 32.0;
pub const SPACE_XXL: f64 = 48.0;

// ------------------------------------------------------------------
// RADII — corner-radius scale
// ------------------------------------------------------------------

pub const RADIUS_XS: f64 = 2.0;
pub const RADIUS_SM: f64 = 4.0;
pub const RADIUS_MD: f64 = 8.0;
pub const RADIUS_LG: f64 = 14.0;

// ------------------------------------------------------------------
// ELEVATION — soft shadows + focus ring (CALayer params)
//
// Spec-only constants; the editor reads them when configuring CALayer
// shadow/border properties.
// ------------------------------------------------------------------

/// Standard card elevation: y=2, blur=8, opacity=0.35.
pub const ELEV_1: (f64, f64, f64) = (2.0, 8.0, 0.35);

/// Lifted / hover elevation: y=6, blur=18, opacity=0.45.
pub const ELEV_2: (f64, f64, f64) = (6.0, 18.0, 0.45);

/// Focus-ring stroke width (px).
pub const FOCUS_RING_WIDTH: f64 = 1.5;

// ------------------------------------------------------------------
// HELPERS — small color math used at multiple sites
// ------------------------------------------------------------------

/// Replace the alpha channel of an Rgba while leaving RGB intact.
#[inline]
pub const fn with_alpha(c: Rgba, a: f64) -> Rgba {
    (c.0, c.1, c.2, a)
}

/// Multiply an Rgb's brightness — useful for derived shades like the
/// dim ghost of a band accent on the spectrum trail.
#[inline]
pub fn scale_rgb(c: Rgb, k: f64) -> Rgb {
    (
        (c.0 * k).clamp(0.0, 1.0),
        (c.1 * k).clamp(0.0, 1.0),
        (c.2 * k).clamp(0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_alpha_is_unit() {
        // Every base color in the palette should land at full alpha by
        // default — alpha is something we layer on intentionally with
        // `with_alpha`, not something that sneaks into the base palette.
        for c in [
            BG_0, BG_1, BG_2, LINE, INK_HI, INK_MD, INK_LO, INK_MUTED, ACCENT, ACCENT_2, WARN,
            WARN_SOFT,
        ] {
            assert_eq!(c.3, 1.0);
        }
    }

    #[test]
    fn spacing_scale_strictly_increases() {
        let xs = [
            SPACE_XXS, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL, SPACE_XXL,
        ];
        for w in xs.windows(2) {
            assert!(
                w[1] > w[0],
                "spacing scale not strictly increasing: {:?}",
                w
            );
        }
    }

    #[test]
    fn band_palette_is_low_mid_high() {
        assert_eq!(BAND_PALETTE[0], BAND_LOW);
        assert_eq!(BAND_PALETTE[1], BAND_MID);
        assert_eq!(BAND_PALETTE[2], BAND_HIGH);
    }
}
