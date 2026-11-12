//! The rotary knob: a lit cap in the theme's style, an engraved scale,
//! a pointer, and a legend that shows the value while you touch it.

use std::vec::Vec;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Pt, Scene, Stroke};

use crate::material::{self, Occluder};
use stave_ui::Fonts;

use crate::part::{measured, Area, Bake, Bind, FaceNode, Frame, Gesture, Grab, Part, Reads};
use crate::theme::{CapStyle, Theme};

/// Knob sizes: the cap's radius in points.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum KnobSize {
    Small,
    Medium,
    Large,
    Huge,
}

impl KnobSize {
    pub fn radius(self) -> f64 {
        match self {
            KnobSize::Small => 15.0,
            KnobSize::Medium => 21.0,
            KnobSize::Large => 28.0,
            KnobSize::Huge => 38.0,
        }
    }
}

/// The scale printed around the knob.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Eleven ticks, the ends and middle long.
    Ticks,
    /// Ticks with the centre marked (for bipolar parameters).
    Centered,
    None,
}

#[derive(Copy, Clone, Debug)]
pub struct Knob {
    pub bind: Bind,
    pub size: KnobSize,
    pub scale: Scale,
    /// The legend; the parameter's name when None.
    pub label: Option<&'static str>,
}

/// Where the scale ring sits outside the cap.
const RING: f64 = 7.0;
/// The widest value a knob shows while touched, for its width: a sign,
/// three digits, a point, a digit and a unit.
const WIDEST_VALUE: &str = "-000.0 dB";

/// The angle (radians, y-down) of normalized value `v`: a 270° sweep
/// from lower-left through the top to lower-right.
pub fn knob_angle(v: f64) -> f64 {
    (135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians()
}

impl Knob {
    pub fn new(bind: Bind) -> Self {
        Knob {
            bind,
            size: KnobSize::Medium,
            scale: Scale::Ticks,
            label: None,
        }
    }
    pub fn size(mut self, s: KnobSize) -> Self {
        self.size = s;
        self
    }
    pub fn small(self) -> Self {
        self.size(KnobSize::Small)
    }
    pub fn large(self) -> Self {
        self.size(KnobSize::Large)
    }
    pub fn huge(self) -> Self {
        self.size(KnobSize::Huge)
    }
    pub fn centered(mut self) -> Self {
        self.scale = Scale::Centered;
        self
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
    /// The cap, its scale ring and the clearance around them.
    fn reach(&self, th: &Theme) -> f64 {
        self.size.radius() + RING + th.space(0) + 1.0
    }
    fn centre(&self, r: Rect, th: &Theme) -> Pt {
        pt(r.x + r.w / 2.0, r.y + self.reach(th))
    }
    fn legend(&self) -> std::string::String {
        self.label.unwrap_or(self.bind.name).to_uppercase()
    }
}

fn pointer_span(style: CapStyle, r: f64) -> (f64, f64, f64) {
    match style {
        CapStyle::Aluminium => (0.18 * r, 0.66 * r, 2.2),
        CapStyle::Dome => (0.28 * r, 0.7 * r, 2.4),
        CapStyle::Soft | CapStyle::Chicken => (0.3 * r, 0.8 * r, 2.6),
    }
}

impl Part for Knob {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let d = 2.0 * self.reach(th);
        let legend = fonts.width(&self.legend(), ty.legend_size, &ty.legend);
        let value = fonts.width(WIDEST_VALUE, ty.value_size, &ty.value);
        (d.max(legend.max(value) + 2.0 * th.space(0)), d + th.line(ty.legend_size))
    }

    fn occluders(&self, r: Rect, theme: &Theme, out: &mut Vec<Occluder>) {
        let c = self.centre(r, theme);
        let rad = self.size.radius();
        out.push(Occluder::Disc {
            cx: c.x,
            cy: c.y,
            r: rad,
            height: 4.0 + rad * 0.2,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let c = self.centre(r, th);
        let rad = self.size.radius();
        let (style, tint, scale) = (th.hardware.caps, th.hardware.cap, cx.scale);
        let size = 2.0 * rad + 2.0;
        let cap = material::sprite(size, scale, |dx, dy| material::knob(style, tint, dx, dy, rad, scale));
        cx.image(c.x - size / 2.0, c.y - size / 2.0, size, size, cap);
        if self.scale != Scale::None {
            let ring = rad + RING;
            for k in 0..=10 {
                let a = knob_angle(f64::from(k) / 10.0);
                let major = k == 0 || k == 10 || (k == 5 && self.scale == Scale::Centered);
                let dot = if major { 1.5 } else { 0.95 };
                cx.scene.solid(Path::circle(pt(c.x + a.cos() * ring, c.y + a.sin() * ring), dot), th.palette.ink_soft);
            }
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let c = self.centre(r, th);
        let rad = self.size.radius();
        let v = f.value(self.bind);
        let a = knob_angle(v);
        let (ca, sa) = (a.cos(), a.sin());
        if th.hardware.caps == CapStyle::Chicken {
            chicken(f, s, c, rad, a);
        } else {
            let (r0, r1, w) = pointer_span(th.hardware.caps, rad);
            let line = Path::new().move_to(pt(c.x + ca * r0, c.y + sa * r0)).line_to(pt(c.x + ca * r1, c.y + sa * r1));
            s.stroke(line, Stroke::round(th.hardware.pointer, w));
        }
        // The lit mark on the scale ring: where the knob points.
        let lit = f.focus.active || f.focus.hover;
        let ring = rad + RING;
        let accent = th.palette.accent;
        s.solid(Path::circle(pt(c.x + ca * ring, c.y + sa * ring), if lit { 2.4 } else { 1.9 }), accent);
        // Legend, or the value while touched.
        let ty = &th.typography;
        let base = pt(r.x + r.w / 2.0, r.y + r.h - ty.legend_size * 0.5);
        if lit {
            s.text_in(base, f.controller.value_text(self.bind.id), ty.value_size, accent, ty.value);
        } else {
            s.text_in(base, self.legend(), ty.legend_size, th.palette.ink, ty.legend);
        }
    }

    fn grabs(&self, r: Rect, f: &Frame<'_>, out: &mut Vec<Grab>) {
        out.push(Grab {
            area: Area::Circle {
                c: self.centre(r, f.theme),
                r: self.size.radius() + RING + 3.0,
            },
            gesture: Gesture::Rotary {
                bind: self.bind,
                travel: 220.0,
            },
        });
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
    }
}

/// Draw a chicken-head pointer of a knob of radius `r` at `c`, turned to
/// `angle`: a sprite cached per 2° step.
pub fn chicken(f: &Frame<'_>, s: &mut Scene, c: Pt, r: f64, angle: f64) {
    let step = (angle.to_degrees() / 2.0).round();
    let a = (step * 2.0).to_radians();
    let tint = f.theme.hardware.cap;
    let reach = r * 1.12;
    let size = 2.0 * reach + 4.0;
    let key = crate::part::sprite_key(("chicken", r.to_bits(), step as i64, tint.map(f64::to_bits)));
    f.sprite(s, c, size, key, |scale| {
        material::sprite(size, scale, |dx, dy| material::chicken_head(dx, dy, reach, a, tint, scale))
    });
}
