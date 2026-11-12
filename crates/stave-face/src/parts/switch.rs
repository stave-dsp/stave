//! Switches: the bat toggle for two- and three-way choices, the
//! footswitch, and the indicator lamp.

use std::string::{String, ToString};
use std::vec::Vec;

use stave_core::telemetry::TapId;
use stave_core::Controller;
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Pt, Scene, TextAlign};

use crate::material::{self, Occluder};
use stave_ui::Fonts;

use crate::part::{measured, sprite_key, step_of, Area, Bake, Bind, FaceNode, Frame, Gesture, Grab, Part, Reads};
use crate::theme::Theme;

// ----------------------------------------------------------------- toggle

/// A bat-handle toggle for a 2- or 3-step parameter, its options
/// printed beside it (top to bottom = first to last step).
#[derive(Copy, Clone, Debug)]
pub struct Toggle {
    pub bind: Bind,
    pub label: Option<&'static str>,
}

/// From the lever's pivot to its printed options.
const OPTION_GAP: f64 = 18.0;
/// The least room left of the pivot (the nut and the lever's swing).
const PIVOT_X: f64 = 28.0;

/// The rows the options print on.
fn option_pitch(th: &Theme) -> f64 {
    th.typography.caption_size * 1.75
}

/// The column the options print in: room for about eight caption letters.
fn options_w(th: &Theme) -> f64 {
    th.typography.caption_size * 6.5
}

impl Toggle {
    pub fn new(bind: Bind) -> Self {
        Toggle { bind, label: None }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
    /// The lever's pivot: the options column at the right edge, the
    /// legend's row and an option's pitch above.
    fn pivot(&self, r: Rect, th: &Theme) -> Pt {
        let x = r.x + r.w - options_w(th) - OPTION_GAP;
        pt(x, r.y + th.line(th.typography.legend_size) + 2.0 * option_pitch(th))
    }
    fn option_y(&self, r: Rect, th: &Theme, i: u32, n: u32) -> f64 {
        let c = self.pivot(r, th).y;
        c + (f64::from(i) - f64::from(n.saturating_sub(1)) / 2.0) * option_pitch(th)
    }
    fn legend(&self) -> String {
        self.label.unwrap_or(self.bind.name).to_uppercase()
    }
    fn names(&self, controller: &dyn Controller) -> Vec<String> {
        let n = controller.steps(self.bind.id).clamp(2, 3);
        (0..n).map(|i| controller.step_name(self.bind.id, i).unwrap_or_default().to_string()).collect()
    }
}

impl Part for Toggle {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        // The legend centres on the pivot: as much room left of it as half.
        let half = fonts.width(&self.legend(), ty.legend_size, &ty.legend) / 2.0 + 2.0;
        let pitch = option_pitch(th);
        (half.max(PIVOT_X) + OPTION_GAP + options_w(th), th.line(ty.legend_size) + 4.0 * pitch)
    }

    fn occluders(&self, r: Rect, theme: &Theme, out: &mut Vec<Occluder>) {
        let p = self.pivot(r, theme);
        out.push(Occluder::Disc {
            cx: p.x,
            cy: p.y,
            r: 8.0,
            height: 5.0,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let scale = cx.scale;
        let p = self.pivot(r, th);
        let nut = material::sprite(22.0, scale, |dx, dy| material::toggle_nut(dx, dy, 9.0, scale));
        cx.image(p.x - 11.0, p.y - 11.0, 22.0, 22.0, nut);
        let base = r.y + th.line(ty.legend_size) * 0.6;
        cx.scene.text_in(pt(p.x, base), self.legend(), ty.legend_size, th.palette.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let ty = &th.typography;
        let names = self.names(f.controller);
        let n = names.len() as u32;
        let cur = step_of(f.value(self.bind), n);
        // The lever: up for the first option, down for the last.
        let tilt = if n == 2 {
            if cur == 0 { -1.0 } else { 1.0 }
        } else {
            f64::from(cur) - 1.0
        };
        let p = self.pivot(r, th);
        let len = 12.0;
        f.sprite(s, p, 34.0, sprite_key(("lever", (tilt * 4.0) as i64)), |scale| {
            material::sprite(34.0, scale, |dx, dy| material::toggle_lever(dx, dy, len, tilt, scale))
        });
        let font = ty.caption.align(TextAlign::Left);
        for (i, name) in names.iter().enumerate() {
            let i = i as u32;
            let (color, w) = if i == cur { (th.palette.accent, 750) } else { (th.palette.ink_soft, 600) };
            let mut fnt = font;
            fnt.weight = w;
            let y = self.option_y(r, th, i, n) + ty.caption_size * 0.4;
            s.text_in(pt(p.x + OPTION_GAP, y), name.as_str(), ty.caption_size, color, fnt);
        }
    }

    fn grabs(&self, r: Rect, f: &Frame<'_>, out: &mut Vec<Grab>) {
        let th = f.theme;
        let p = self.pivot(r, th);
        let pitch = option_pitch(th);
        out.push(Grab {
            area: Area::Circle { c: p, r: 16.0 },
            gesture: Gesture::Stepped {
                bind: self.bind,
                pitch: 16.0,
            },
        });
        // Each printed option is its own click target, on the rows paint
        // prints them.
        let n = f.controller.steps(self.bind.id).clamp(2, 3);
        for i in 0..n {
            let y = self.option_y(r, th, i, n);
            out.push(Grab {
                area: Area::Rect(Rect {
                    x: p.x + 16.0,
                    y: y - pitch / 2.0,
                    w: r.x + r.w - p.x - 16.0,
                    h: pitch,
                }),
                gesture: Gesture::Choose { bind: self.bind, step: i },
            });
        }
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
    }
}

// ------------------------------------------------------------- footswitch

/// What pressing a footswitch does.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// Each press flips the parameter.
    Latch,
    /// The parameter is on while held, off on release (TAP, FORGET).
    Momentary,
}

/// When a footswitch's lamp is lit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LitWhen {
    /// While the parameter is on.
    On,
    /// While the parameter is off (a bypass: lit while engaged).
    Off,
}

/// A stomp footswitch on a parameter, with its lamp and legend.
#[derive(Copy, Clone, Debug)]
pub struct Footswitch {
    pub bind: Bind,
    pub label: Option<&'static str>,
    pub press: Press,
    pub lit: LitWhen,
    /// The nut's radius: 21 for a stomp switch, 14 for a panel button.
    pub radius: f64,
    /// The lamp's colour (the theme's lamp when None).
    pub color: Option<crate::photo::Rgb>,
}

impl Footswitch {
    pub fn new(bind: Bind) -> Self {
        Footswitch {
            bind,
            label: None,
            press: Press::Latch,
            lit: LitWhen::On,
            radius: 21.0,
            color: None,
        }
    }
    /// A bypass switch: lit while the effect is engaged.
    pub fn bypass(bind: Bind) -> Self {
        Footswitch {
            label: Some("BYPASS"),
            lit: LitWhen::Off,
            ..Footswitch::new(bind)
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    /// Held, not latched (a FORGET or TAP button).
    pub fn momentary(mut self) -> Self {
        self.press = Press::Momentary;
        self
    }
    /// When the lamp is lit.
    pub fn lit(mut self, when: LitWhen) -> Self {
        self.lit = when;
        self
    }
    /// The lamp's colour (linear RGB; may exceed 1 for a hot core).
    pub fn color(mut self, c: crate::photo::Rgb) -> Self {
        self.color = Some(c);
        self
    }
    /// A smaller switch for a studio panel.
    pub fn compact(mut self) -> Self {
        self.radius = 14.0;
        self
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
    fn legend(&self) -> String {
        self.label.unwrap_or(self.bind.name).to_uppercase()
    }
    fn lamp(&self, r: Rect, th: &Theme) -> Pt {
        pt(r.x + r.w / 2.0, r.y + th.space(1) + 2.0)
    }
    /// The legend's baseline, under the lamp's seat.
    fn legend_y(&self, r: Rect, th: &Theme) -> f64 {
        self.lamp(r, th).y + LAMP_SEAT + th.typography.legend_size * 1.5
    }
    fn button(&self, r: Rect) -> Pt {
        pt(r.x + r.w / 2.0, r.y + r.h - self.radius - 3.0)
    }
}

/// A lamp seat's radius.
const LAMP_SEAT: f64 = 7.0;

impl Part for Footswitch {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let legend = fonts.width(&self.legend(), ty.legend_size, &ty.legend);
        let w = (4.0 * self.radius).max(legend + 2.0 * th.space(1));
        (w, self.legend_y(Rect::default(), th) + th.space(3) + 2.0 * self.radius + 3.0)
    }

    fn occluders(&self, r: Rect, _theme: &Theme, out: &mut Vec<Occluder>) {
        let b = self.button(r);
        out.push(Occluder::Disc {
            cx: b.x,
            cy: b.y,
            r: self.radius - 1.0,
            height: self.radius * 0.38,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let scale = cx.scale;
        let b = self.button(r);
        let rad = self.radius;
        let size = 2.0 * rad + 2.0;
        let sw = material::sprite(size, scale, |dx, dy| material::footswitch(dx, dy, rad, scale));
        cx.image(b.x - size / 2.0, b.y - size / 2.0, size, size, sw);
        let l = self.lamp(r, th);
        let seat = material::sprite(14.0, scale, |dx, dy| material::lamp_seat(dx, dy, 6.0, scale));
        cx.image(l.x - 7.0, l.y - 7.0, 14.0, 14.0, seat);
        cx.scene.text_in(pt(l.x, self.legend_y(r, th)), self.legend(), ty.legend_size, th.palette.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let on = f.value(self.bind) >= 0.5;
        if on == (self.lit == LitWhen::On) {
            lamp_in(f, s, self.lamp(r, f.theme), self.color.unwrap_or(f.theme.hardware.lamp));
        }
    }

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        out.push(Grab {
            area: Area::Circle {
                c: self.button(r),
                r: self.radius + 2.0,
            },
            gesture: match self.press {
                Press::Latch => Gesture::Latch { bind: self.bind },
                Press::Momentary => Gesture::Momentary { bind: self.bind },
            },
        });
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
    }
}

/// Draw a lit LED (theme lamp colour) at `c`.
pub fn lamp(f: &Frame<'_>, s: &mut Scene, c: Pt) {
    lamp_in(f, s, c, f.theme.hardware.lamp);
}

/// Draw a lit LED of colour `col` (linear RGB) at `c`.
pub fn lamp_in(f: &Frame<'_>, s: &mut Scene, c: Pt, col: crate::photo::Rgb) {
    let key = sprite_key(("lamp", col.map(f64::to_bits)));
    f.sprite(s, c, 44.0, key, |scale| {
        material::sprite(44.0, scale, |dx, dy| material::lamp_glow(dx, dy, col, 4.2, scale))
    });
}

// ------------------------------------------------------------------- lamp

/// What lights an indicator lamp.
#[derive(Copy, Clone, Debug)]
pub enum LampSource {
    /// A parameter at or above half.
    Param(Bind),
    /// A tap above a threshold.
    Tap(TapId, f64),
    /// Always lit (a pilot lamp: the unit is on).
    Always,
}

/// What an indicator lamp looks like.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LampStyle {
    /// A small LED.
    Led,
    /// A faceted jewel pilot lamp (amps).
    Jewel,
}

/// An indicator lamp with an optional legend under it.
#[derive(Copy, Clone, Debug)]
pub struct Lamp {
    pub source: LampSource,
    pub label: Option<&'static str>,
    pub style: LampStyle,
    /// The lamp's colour (the theme's lamp when None).
    pub color: Option<crate::photo::Rgb>,
}

const JEWEL_R: f64 = 10.0;

impl Lamp {
    pub fn param(b: Bind) -> Self {
        Lamp {
            source: LampSource::Param(b),
            label: None,
            style: LampStyle::Led,
            color: None,
        }
    }
    pub fn tap(id: TapId, threshold: f64) -> Self {
        Lamp {
            source: LampSource::Tap(id, threshold),
            label: None,
            style: LampStyle::Led,
            color: None,
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    /// The lamp's colour (linear RGB; may exceed 1 for a hot core).
    pub fn color(mut self, c: crate::photo::Rgb) -> Self {
        self.color = Some(c);
        self
    }
    /// A jewel pilot lamp, lit whenever its source is on.
    pub fn jewel(mut self) -> Self {
        self.style = LampStyle::Jewel;
        self
    }
    /// A pilot jewel that is always lit (the amp is on).
    pub fn pilot() -> Self {
        Lamp {
            source: LampSource::Always,
            label: None,
            style: LampStyle::Jewel,
            color: None,
        }
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
    fn centre(&self, r: Rect) -> Pt {
        let half = if self.style == LampStyle::Jewel { JEWEL_R + 3.0 } else { 9.0 };
        pt(r.x + r.w / 2.0, r.y + half)
    }
}

/// A lamp's least width: room for its glow.
const LAMP_W: f64 = 44.0;

impl Part for Lamp {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let top = if self.style == LampStyle::Jewel { 2.0 * JEWEL_R + 6.0 } else { 18.0 };
        match self.label {
            Some(l) => {
                let w = fonts.width(l, ty.caption_size, &ty.caption) + 2.0 * th.space(0);
                (w.max(LAMP_W), top + th.line(ty.caption_size) + 1.5 * th.space(0))
            }
            None => (LAMP_W, top),
        }
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let scale = cx.scale;
        let c = self.centre(r);
        let below = if self.style == LampStyle::Jewel {
            let col = self.color.unwrap_or(th.hardware.lamp);
            let size = 2.0 * JEWEL_R + 2.0;
            let j = material::sprite(size, scale, |dx, dy| material::jewel(dx, dy, JEWEL_R, col, false, scale));
            cx.image(c.x - size / 2.0, c.y - size / 2.0, size, size, j);
            JEWEL_R + 14.0
        } else {
            let seat = material::sprite(14.0, scale, |dx, dy| material::lamp_seat(dx, dy, 6.0, scale));
            cx.image(c.x - 7.0, c.y - 7.0, 14.0, 14.0, seat);
            22.0
        };
        if let Some(l) = self.label {
            let ty = &th.typography;
            cx.scene.text_in(pt(c.x, c.y + below), l, ty.caption_size, th.palette.ink_soft, ty.caption);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let on = match self.source {
            LampSource::Param(b) => f.value(b) >= 0.5,
            LampSource::Tap(id, t) => f.tap(id) > t,
            LampSource::Always => true,
        };
        if !on {
            return;
        }
        let col = self.color.unwrap_or(f.theme.hardware.lamp);
        if self.style == LampStyle::Jewel {
            let size = 2.0 * JEWEL_R * 3.0;
            let key = sprite_key(("jewel", col.map(f64::to_bits)));
            f.sprite(s, self.centre(r), size, key, |scale| {
                material::sprite(size, scale, |dx, dy| material::jewel(dx, dy, JEWEL_R, col, true, scale))
            });
        } else {
            lamp_in(f, s, self.centre(r), col);
        }
    }

    fn reads(&self) -> Reads {
        match self.source {
            LampSource::Param(b) => Reads::params(&[b]),
            LampSource::Tap(id, _) => Reads::taps(&[id]),
            LampSource::Always => Reads::NONE,
        }
    }
}
