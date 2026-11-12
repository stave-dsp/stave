//! Themes: everything a face's look is made of, as data. A part never
//! mints a colour or a font; it asks the theme for a role. Swapping the
//! theme re-skins a face without touching its layout or its parts.

use crate::photo::{hex, Rgb};
use stave_ui::scene::{Family, Font};
use stave_ui::style::Color;

/// How the enclosure's paint or metal is finished. Each is a lit
/// material in [`crate::material`], not a texture.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Coat {
    /// Anodized aluminium with a fine brushed grain.
    Anodized,
    /// Smooth powder coat with a faint orange-peel.
    Powder,
    /// Metallic paint dimpled by hammer dents.
    Hammertone,
    /// Wrinkle paint: a tight, irregular crinkle.
    Crinkle,
    /// Glossy vitreous enamel over a mottled ground.
    Enamel,
}

/// The knob family a theme fits its controls with.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CapStyle {
    /// Spun-aluminium cap on a knurled black skirt.
    Aluminium,
    /// Glossy black dome on a knurled chrome skirt.
    Dome,
    /// Soft-touch cap the colour of `Hardware::cap`, with a printed line.
    Soft,
    /// The amp knob: a bakelite skirt under a chicken-head pointer in
    /// `Hardware::cap`, the whole head turning with the value.
    Chicken,
}

/// Text roles. Sizes are in points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Typography {
    /// The product name on the face.
    pub title: Font,
    pub title_size: f64,
    /// The line under the title.
    pub tagline: Font,
    pub tagline_size: f64,
    /// Control legends (TIME, MIX).
    pub legend: Font,
    pub legend_size: f64,
    /// Small print: option names, scale numbers, section heads.
    pub caption: Font,
    pub caption_size: f64,
    /// Values on readouts and while a control moves.
    pub value: Font,
    pub value_size: f64,
}

/// The enclosure: its paint or metal, the plates set into it, and the
/// bench it sits on.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Finish {
    pub coat: Coat,
    /// Base colour (linear RGB).
    pub surface: Rgb,
    /// A recessed plate (a control group).
    pub plate: Rgb,
    /// The bench the enclosure sits on.
    pub desk: Rgb,
    /// Corner radius of the enclosure.
    pub radius: f64,
}

/// The printed and lit colours.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Palette {
    /// Printed legends and engraving.
    pub ink: Color,
    /// Secondary print (taglines, captions, idle option names).
    pub ink_soft: Color,
    /// The colour that means "selected / active" on print and pointers.
    pub accent: Color,
    /// Display glass: the screen's ground and its phosphor.
    pub screen: Color,
    pub phosphor: Color,
}

/// The fitted hardware: knobs and lamps.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Hardware {
    /// The knob family.
    pub caps: CapStyle,
    /// Cap tint for [`CapStyle::Soft`]; pointer colour for the others.
    pub cap: Rgb,
    pub pointer: Color,
    /// LED colour (linear RGB, may exceed 1 for a hot core).
    pub lamp: Rgb,
}

/// Amp and cabinet coverings (linear RGB): the tolex, the piping cord
/// around a faceplate, the grille cloth, and the badge metal.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Coverings {
    pub tolex: Rgb,
    pub piping: Rgb,
    pub grille: Rgb,
    pub badge: Rgb,
}

/// A face's whole look, grouped by what it dresses.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub finish: Finish,
    pub palette: Palette,
    pub hardware: Hardware,
    pub typography: Typography,
    /// The spacing scale (4-pt grid): xs, s, m, l, xl, xxl.
    pub spacing: [f64; 6],
    pub coverings: Coverings,
}

impl Theme {
    pub fn space(&self, step: usize) -> f64 {
        self.spacing[step.min(self.spacing.len() - 1)]
    }

    /// The height a printed line of `size` points takes, leading
    /// included (a legend row, a caption row).
    pub fn line(&self, size: f64) -> f64 {
        size * 1.9
    }
}

const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::rgba(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0)
}

const fn rgba8(r: u8, g: u8, b: u8, a: f64) -> Color {
    Color::rgba(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, a)
}

/// The house typography: SF Pro set tight for titles, tracked caps for
/// legends, SF Mono for values.
pub const TYPE: Typography = Typography {
    title: Font::new(Family::Sans, 700).tracking(0.16),
    title_size: 22.0,
    tagline: Font::new(Family::Sans, 500).tracking(0.06),
    tagline_size: 9.0,
    legend: Font::new(Family::Sans, 650).tracking(0.12),
    legend_size: 9.5,
    caption: Font::new(Family::Sans, 600).tracking(0.1),
    caption_size: 7.5,
    value: Font::new(Family::Mono, 500),
    value_size: 9.5,
};

const SPACE: [f64; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

impl Theme {
    /// The house default: graphite anodized aluminium, warm white
    /// print, amber light, spun-aluminium knobs.
    pub fn graphite() -> Self {
        Theme {
            name: "graphite",
            finish: Finish {
                coat: Coat::Anodized,
                surface: hex(0x2E3034),
                plate: hex(0x232528),
                desk: hex(0x1E1F21),
                radius: 14.0,
            },
            palette: Palette {
                ink: rgb8(0xEC, 0xE6, 0xDA),
                ink_soft: rgba8(0xEC, 0xE6, 0xDA, 0.55),
                accent: rgb8(0xFF, 0xB2, 0x3F),
                screen: rgb8(0x0B, 0x0D, 0x0E),
                phosphor: rgb8(0xFF, 0xB8, 0x52),
            },
            hardware: Hardware {
                caps: CapStyle::Aluminium,
                cap: hex(0x18191B),
                pointer: rgb8(0x1A, 0x1A, 0x1C),
                lamp: [1.6, 0.62, 0.12],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Cream powder coat, rust print, aluminium knobs (heirloom).
    pub fn cream() -> Self {
        Theme {
            name: "cream",
            finish: Finish {
                coat: Coat::Powder,
                surface: hex(0xEADFC6),
                plate: hex(0xDFD2B5),
                desk: hex(0x232427),
                radius: 12.0,
            },
            palette: Palette {
                ink: rgb8(0x3A, 0x2E, 0x26),
                ink_soft: rgba8(0x3A, 0x2E, 0x26, 0.6),
                accent: rgb8(0xA9, 0x54, 0x2E),
                screen: rgb8(0x14, 0x12, 0x10),
                phosphor: rgb8(0xFF, 0xA2, 0x5A),
            },
            hardware: Hardware {
                caps: CapStyle::Aluminium,
                cap: hex(0x1A1A1C),
                pointer: rgb8(0x1A, 0x1A, 0x1C),
                lamp: [1.8, 0.25, 0.12],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Blue-grey hammertone, black dome knobs (revenant).
    pub fn hammertone() -> Self {
        Theme {
            name: "hammertone",
            finish: Finish {
                coat: Coat::Hammertone,
                surface: hex(0x46525E),
                plate: hex(0x3B4651),
                desk: hex(0x232427),
                radius: 14.0,
            },
            palette: Palette {
                ink: rgb8(0xE6, 0xEA, 0xEE),
                ink_soft: rgba8(0xE6, 0xEA, 0xEE, 0.55),
                accent: rgb8(0x6F, 0xE6, 0xF0),
                screen: rgb8(0x05, 0x0D, 0x10),
                phosphor: rgb8(0x6F, 0xE6, 0xF0),
            },
            hardware: Hardware {
                caps: CapStyle::Dome,
                cap: hex(0x0E0E10),
                pointer: rgb8(0xF2, 0xF2, 0xF2),
                lamp: [1.8, 0.2, 0.12],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Oxblood crinkle, cream print (ribbon).
    pub fn oxblood() -> Self {
        Theme {
            name: "oxblood",
            finish: Finish {
                coat: Coat::Crinkle,
                surface: hex(0x5E1A1F),
                plate: hex(0x4C1418),
                desk: hex(0x232427),
                radius: 14.0,
            },
            palette: Palette {
                ink: rgb8(0xF1, 0xE6, 0xCF),
                ink_soft: rgba8(0xF1, 0xE6, 0xCF, 0.6),
                accent: rgb8(0xF6, 0xC9, 0x6A),
                screen: rgb8(0x12, 0x0A, 0x0A),
                phosphor: rgb8(0xF6, 0xC9, 0x6A),
            },
            hardware: Hardware {
                caps: CapStyle::Soft,
                cap: hex(0xEDE3C8),
                pointer: rgb8(0xA3, 0x1F, 0x24),
                lamp: [1.8, 0.3, 0.1],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Verdigris enamel, brass and ivory (aeolian).
    pub fn verdigris() -> Self {
        Theme {
            name: "verdigris",
            finish: Finish {
                coat: Coat::Enamel,
                surface: hex(0x2F6A61),
                plate: hex(0x285A52),
                desk: hex(0x232427),
                radius: 14.0,
            },
            palette: Palette {
                ink: rgb8(0xED, 0xE3, 0xC8),
                ink_soft: rgba8(0xED, 0xE3, 0xC8, 0.6),
                accent: rgb8(0xE2, 0xB8, 0x5C),
                screen: rgb8(0x06, 0x12, 0x10),
                phosphor: rgb8(0xA8, 0xF0, 0xD2),
            },
            hardware: Hardware {
                caps: CapStyle::Soft,
                cap: hex(0xEDE3C8),
                pointer: rgb8(0x2F, 0x6A, 0x61),
                lamp: [1.4, 1.1, 0.35],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Violet anodized, phosphor-green screens (engram).
    pub fn indigo() -> Self {
        Theme {
            name: "indigo",
            finish: Finish {
                coat: Coat::Anodized,
                surface: hex(0x1C1752),
                plate: hex(0x151140),
                desk: hex(0x1E1F21),
                radius: 14.0,
            },
            palette: Palette {
                ink: rgb8(0xE4, 0xE2, 0xF6),
                ink_soft: rgba8(0xE4, 0xE2, 0xF6, 0.55),
                accent: rgb8(0x6B, 0xFF, 0xDB),
                screen: rgb8(0x04, 0x08, 0x0A),
                phosphor: rgb8(0x6B, 0xFF, 0xDB),
            },
            hardware: Hardware {
                caps: CapStyle::Dome,
                cap: hex(0x0E0E12),
                pointer: rgb8(0xF0, 0xF0, 0xF6),
                lamp: [0.5, 1.6, 1.2],
            },
            typography: TYPE,
            spacing: SPACE,
            coverings: Coverings {
                tolex: hex(0x161616),
                piping: hex(0xD8D2C2),
                grille: hex(0x3A3530),
                badge: [0.86, 0.86, 0.9],
            },
        }
    }

    /// Every built-in theme (the storybook pages through these).
    pub fn all() -> [Theme; 6] {
        [Self::graphite(), Self::cream(), Self::hammertone(), Self::oxblood(), Self::verdigris(), Self::indigo()]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::graphite()
    }
}
