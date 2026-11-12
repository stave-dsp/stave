//! Switches: the bat toggle for two- and three-way choices, the
//! footswitch, and the indicator lamp.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Pt, Scene, TextAlign};

use crate::material::{self, Occluder};
use crate::part::{leaf, sprite_key, step_of, Bake, Bind, Control, FaceNode, Frame, Gesture, Hit, Part};
use crate::theme::Theme;

// ----------------------------------------------------------------- toggle

/// A bat-handle toggle for a 2- or 3-step parameter, its options
/// printed beside it (top to bottom = first to last step).
#[derive(Copy, Clone, Debug)]
pub struct Toggle {
    pub bind: Bind,
    pub label: Option<&'static str>,
}

const TOGGLE_W: f64 = 96.0;
const TOGGLE_H: f64 = 72.0;
const OPTION_PITCH: f64 = 13.0;

impl Toggle {
    pub fn new(bind: Bind) -> Self {
        Toggle { bind, label: None }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn node(self) -> FaceNode {
        leaf(self, TOGGLE_W, TOGGLE_H)
    }
    fn pivot(&self, r: Rect) -> Pt {
        pt(r.x + 28.0, r.y + 46.0)
    }
    fn option_y(&self, r: Rect, i: u32, n: u32) -> f64 {
        let c = self.pivot(r).y;
        c + (f64::from(i) - f64::from(n.saturating_sub(1)) / 2.0) * OPTION_PITCH
    }
    fn names(&self, host: &dyn crate::part::Host) -> Vec<String> {
        let n = host.steps(self.bind.id).clamp(2, 3);
        (0..n).map(|i| host.step_name(self.bind.id, i).unwrap_or_default()).collect()
    }
}

impl Part for Toggle {
    fn occluders(&self, r: Rect, _theme: &Theme, out: &mut Vec<Occluder>) {
        let p = self.pivot(r);
        out.push(Occluder::Disc {
            cx: p.x,
            cy: p.y,
            r: 8.0,
            height: 5.0,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let scale = cx.scale;
        let p = self.pivot(r);
        let nut = material::sprite(22.0, scale, |dx, dy| material::toggle_nut(dx, dy, 9.0, scale));
        cx.image(p.x - 11.0, p.y - 11.0, 22.0, 22.0, nut);
        let legend = self.label.unwrap_or(self.bind.name).to_uppercase();
        cx.scene.text_in(pt(p.x, r.y + 11.0), legend, ty.legend_size, th.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let ty = &th.type_;
        let names = self.names(f.host);
        let n = names.len() as u32;
        let cur = step_of(f.value(self.bind), n);
        // The lever: up for the first option, down for the last.
        let tilt = if n == 2 {
            if cur == 0 { -1.0 } else { 1.0 }
        } else {
            f64::from(cur) - 1.0
        };
        let p = self.pivot(r);
        let len = 12.0;
        f.sprite(s, p, 34.0, sprite_key(("lever", (tilt * 4.0) as i64)), |scale| {
            material::sprite(34.0, scale, |dx, dy| material::toggle_lever(dx, dy, len, tilt, scale))
        });
        let font = ty.caption.align(TextAlign::Left);
        for (i, name) in names.iter().enumerate() {
            let i = i as u32;
            let (color, w) = if i == cur { (th.accent, 750) } else { (th.ink_soft, 600) };
            let mut fnt = font;
            fnt.weight = w;
            s.text_in(pt(p.x + 18.0, self.option_y(r, i, n) + 3.0), name.as_str(), ty.caption_size, color, fnt);
        }
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        let p = self.pivot(r);
        let mut v = alloc::vec![Control {
            hit: Hit::Circle { c: p, r: 16.0 },
            gesture: Gesture::Stepped {
                bind: self.bind,
                pitch: 16.0,
            },
        }];
        // Each printed option is its own click target. The step count
        // is unknown here, so lay out three rows; a two-way toggle's
        // rows sit on the same pitch around the pivot.
        for (i, n) in [(0u32, 3u32), (1, 3), (2, 3)] {
            let y = self.option_y(r, i, n);
            v.push(Control {
                hit: Hit::Rect(Rect {
                    x: p.x + 16.0,
                    y: y - OPTION_PITCH / 2.0,
                    w: r.x + r.w - p.x - 16.0,
                    h: OPTION_PITCH,
                }),
                gesture: Gesture::Choose { bind: self.bind, step: i },
            });
        }
        v
    }
}

// ------------------------------------------------------------- footswitch

/// A stomp footswitch latching a parameter, with its lamp and legend.
#[derive(Copy, Clone, Debug)]
pub struct Footswitch {
    pub bind: Bind,
    pub label: Option<&'static str>,
    /// Light the lamp when the parameter is OFF (bypass: lit = engaged).
    pub invert: bool,
    /// The nut's radius: 21 for a stomp switch, 14 for a panel button.
    pub radius: f64,
}

impl Footswitch {
    pub fn new(bind: Bind) -> Self {
        Footswitch {
            bind,
            label: None,
            invert: false,
            radius: 21.0,
        }
    }
    /// A bypass switch: lit while the effect is engaged.
    pub fn bypass(bind: Bind) -> Self {
        Footswitch {
            bind,
            label: Some("BYPASS"),
            invert: true,
            radius: 21.0,
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    /// A smaller switch for a studio panel.
    pub fn compact(mut self) -> Self {
        self.radius = 14.0;
        self
    }
    pub fn node(self) -> FaceNode {
        let h = 50.0 + 2.0 * self.radius;
        leaf(self, (4.0 * self.radius).max(64.0), h)
    }
    fn lamp(&self, r: Rect) -> Pt {
        pt(r.x + r.w / 2.0, r.y + 10.0)
    }
    fn button(&self, r: Rect) -> Pt {
        pt(r.x + r.w / 2.0, r.y + r.h - self.radius - 3.0)
    }
}

impl Part for Footswitch {
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
        let ty = &th.type_;
        let scale = cx.scale;
        let b = self.button(r);
        let rad = self.radius;
        let size = 2.0 * rad + 2.0;
        let sw = material::sprite(size, scale, |dx, dy| material::footswitch(dx, dy, rad, scale));
        cx.image(b.x - size / 2.0, b.y - size / 2.0, size, size, sw);
        let l = self.lamp(r);
        let seat = material::sprite(14.0, scale, |dx, dy| material::lamp_seat(dx, dy, 6.0, scale));
        cx.image(l.x - 7.0, l.y - 7.0, 14.0, 14.0, seat);
        let legend = self.label.unwrap_or(self.bind.name).to_uppercase();
        cx.scene.text_in(pt(l.x, l.y + 21.0), legend, ty.legend_size, th.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let on = (f.value(self.bind) >= 0.5) != self.invert;
        if on {
            lamp(f, s, self.lamp(r));
        }
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        alloc::vec![Control {
            hit: Hit::Circle {
                c: self.button(r),
                r: self.radius + 2.0,
            },
            gesture: Gesture::Latch { bind: self.bind },
        }]
    }
}

/// Draw a lit LED (theme lamp colour) at `c`.
pub fn lamp(f: &Frame<'_>, s: &mut Scene, c: Pt) {
    let col = f.theme.lamp;
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
    /// A probe above a threshold.
    Probe(&'static str, f64),
}

/// An indicator LED with an optional legend under it.
#[derive(Copy, Clone, Debug)]
pub struct Lamp {
    pub source: LampSource,
    pub label: Option<&'static str>,
    /// A faceted jewel pilot lamp (amps) instead of an LED.
    pub jewel: bool,
}

const JEWEL_R: f64 = 10.0;

impl Lamp {
    pub fn param(b: Bind) -> Self {
        Lamp {
            source: LampSource::Param(b),
            label: None,
            jewel: false,
        }
    }
    pub fn probe(name: &'static str, threshold: f64) -> Self {
        Lamp {
            source: LampSource::Probe(name, threshold),
            label: None,
            jewel: false,
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    /// A jewel pilot lamp, lit whenever its source is on.
    pub fn jewel(mut self) -> Self {
        self.jewel = true;
        self
    }
    /// A pilot jewel that is always lit (the amp is on).
    pub fn pilot() -> Self {
        Lamp {
            source: LampSource::Probe("", f64::NEG_INFINITY),
            label: None,
            jewel: true,
        }
    }
    pub fn node(self) -> FaceNode {
        let top = if self.jewel { 2.0 * JEWEL_R + 6.0 } else { 18.0 };
        let h = top + if self.label.is_some() { 20.0 } else { 0.0 };
        leaf(self, 44.0, h)
    }
    fn centre(&self, r: Rect) -> Pt {
        let half = if self.jewel { JEWEL_R + 3.0 } else { 9.0 };
        pt(r.x + r.w / 2.0, r.y + half)
    }
}

impl Part for Lamp {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let scale = cx.scale;
        let c = self.centre(r);
        let below = if self.jewel {
            let col = th.lamp;
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
            let ty = &th.type_;
            cx.scene.text_in(pt(c.x, c.y + below), l, ty.caption_size, th.ink_soft, ty.caption);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let on = match self.source {
            LampSource::Param(b) => f.value(b) >= 0.5,
            LampSource::Probe("", _) => true,
            LampSource::Probe(name, t) => f.probe(name) > t,
        };
        if !on {
            return;
        }
        if self.jewel {
            let col = f.theme.lamp;
            let size = 2.0 * JEWEL_R * 3.0;
            let key = sprite_key(("jewel", col.map(f64::to_bits)));
            f.sprite(s, self.centre(r), size, key, |scale| {
                material::sprite(size, scale, |dx, dy| material::jewel(dx, dy, JEWEL_R, col, true, scale))
            });
        } else {
            lamp(f, s, self.centre(r));
        }
    }

    fn probes(&self) -> Vec<&'static str> {
        match self.source {
            LampSource::Probe("", _) => Vec::new(),
            LampSource::Probe(name, _) => alloc::vec![name],
            LampSource::Param(_) => Vec::new(),
        }
    }

    fn reads(&self) -> Vec<Bind> {
        match self.source {
            LampSource::Param(b) => alloc::vec![b],
            LampSource::Probe(..) => Vec::new(),
        }
    }
}
