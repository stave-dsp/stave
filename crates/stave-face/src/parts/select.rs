//! Choosing from a list: the rotary [`Selector`] (a pointer knob with the
//! options printed around it), the [`Stepper`] (a glass readout with
//! ◀ ▶ for long lists), the [`Presets`] readout, and the [`Pager`] that
//! flips editor-local pages.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Pt, Scene, Stroke, TextAlign};
use stave_ui::Dim;

use crate::material::{self, Occluder};
use crate::part::{leaf, step_of, Bake, Bind, Control, FaceNode, Frame, Gesture, Hit, Part};
use crate::parts::display::{bake_window, glass_over};
use crate::parts::switch::lamp;
use crate::theme::{CapStyle, Theme};

fn legend_of(label: Option<&'static str>, bind: Bind) -> String {
    label.unwrap_or(bind.name).to_uppercase()
}

// --------------------------------------------------------------- selector

/// A rotary switch for a stepped parameter (up to about eight options):
/// a small pointer knob with each option printed at its detent.
#[derive(Copy, Clone, Debug)]
pub struct Selector {
    pub bind: Bind,
    pub label: Option<&'static str>,
}

const SEL_R: f64 = 15.0;
const SEL_W: f64 = 132.0;
const SEL_H: f64 = 112.0;
/// Detents sweep this arc (degrees, y-down, centred on straight up).
const SEL_SWEEP: f64 = 240.0;

impl Selector {
    pub fn new(bind: Bind) -> Self {
        Selector { bind, label: None }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn node(self) -> FaceNode {
        leaf(self, SEL_W, SEL_H)
    }
    fn centre(r: Rect) -> Pt {
        pt(r.x + r.w / 2.0, r.y + 24.0 + 36.0)
    }
    fn angle(i: u32, n: u32) -> f64 {
        let t = if n <= 1 { 0.5 } else { f64::from(i) / f64::from(n - 1) };
        (-90.0 - SEL_SWEEP / 2.0 + SEL_SWEEP * t).to_radians()
    }
}

impl Part for Selector {
    fn occluders(&self, r: Rect, _theme: &Theme, out: &mut Vec<Occluder>) {
        let c = Self::centre(r);
        out.push(Occluder::Disc {
            cx: c.x,
            cy: c.y,
            r: SEL_R,
            height: 6.0,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let c = Self::centre(r);
        let (style, tint, scale) = (th.caps, th.cap, cx.scale);
        let size = 2.0 * SEL_R + 2.0;
        let cap = material::sprite(size, scale, |dx, dy| material::knob(style, tint, dx, dy, SEL_R, scale));
        cx.image(c.x - size / 2.0, c.y - size / 2.0, size, size, cap);
        cx.scene.text_in(pt(c.x, r.y + 11.0), legend_of(self.label, self.bind), ty.legend_size, th.ink, ty.legend);
        let n = cx.controller.steps(self.bind.id).max(2);
        for i in 0..n {
            let a = Self::angle(i, n);
            let p = pt(c.x + a.cos() * (SEL_R + 6.0), c.y + a.sin() * (SEL_R + 6.0));
            cx.scene.solid(Path::circle(p, 1.1), th.ink_soft);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let ty = &th.type_;
        let c = Self::centre(r);
        let n = f.controller.steps(self.bind.id).max(2);
        let cur = step_of(f.value(self.bind), n);
        let a = Self::angle(cur, n);
        if th.caps == CapStyle::Chicken {
            crate::parts::knob::chicken(f, s, c, SEL_R, a);
        } else {
            let tip = if th.caps == CapStyle::Aluminium { 0.66 } else { 0.8 };
            s.stroke(
                Path::new()
                    .move_to(pt(c.x + a.cos() * SEL_R * 0.2, c.y + a.sin() * SEL_R * 0.2))
                    .line_to(pt(c.x + a.cos() * SEL_R * tip, c.y + a.sin() * SEL_R * tip)),
                Stroke::round(th.pointer, 2.2),
            );
        }
        let lit = f.focus.active || f.focus.hover;
        for i in 0..n {
            let a = Self::angle(i, n);
            let name = f.controller.step_name(self.bind.id, i).unwrap_or_default();
            let rad = SEL_R + 15.0;
            let (x, y) = (c.x + a.cos() * rad, c.y + a.sin() * rad + ty.caption_size * 0.35);
            let align = if a.cos() < -0.3 {
                TextAlign::Right
            } else if a.cos() > 0.3 {
                TextAlign::Left
            } else {
                TextAlign::Center
            };
            let mut font = ty.caption.align(align);
            let color = if i == cur {
                font.weight = 750;
                th.accent
            } else {
                th.ink_soft
            };
            s.text_in(pt(x, y), name, ty.caption_size, color, font);
        }
        if lit {
            s.solid(Path::circle(pt(c.x + a.cos() * (SEL_R + 6.0), c.y + a.sin() * (SEL_R + 6.0)), 2.0), th.accent);
        }
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        alloc::vec![Control {
            hit: Hit::Circle {
                c: Self::centre(r),
                r: SEL_R + 10.0,
            },
            gesture: Gesture::Stepped {
                bind: self.bind,
                pitch: 18.0,
            },
        }]
    }
}

// ---------------------------------------------------------------- stepper

/// A glass readout of a stepped parameter's option, with ◀ ▶ either
/// side: for long lists (algorithms, modes, routing targets).
#[derive(Copy, Clone, Debug)]
pub struct Stepper {
    pub bind: Bind,
    pub label: Option<&'static str>,
    pub width: f64,
}

const READOUT_H: f64 = 24.0;

impl Stepper {
    pub fn new(bind: Bind) -> Self {
        Stepper {
            bind,
            label: None,
            width: 132.0,
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn width(mut self, w: f64) -> Self {
        self.width = w;
        self
    }
    pub fn node(self) -> FaceNode {
        leaf(self, self.width, 24.0 + READOUT_H + 8.0)
    }
}

/// The readout window inside a stepper-like cell, and its arrow hits.
fn readout(r: Rect) -> (Rect, Rect, Rect) {
    let win = Rect {
        x: r.x + 22.0,
        y: r.y + r.h - READOUT_H - 4.0,
        w: r.w - 44.0,
        h: READOUT_H,
    };
    let left = Rect { x: r.x, y: win.y - 4.0, w: 22.0, h: READOUT_H + 8.0 };
    let right = Rect { x: r.x + r.w - 22.0, y: win.y - 4.0, w: 22.0, h: READOUT_H + 8.0 };
    (win, left, right)
}

fn arrows(th: &Theme, s: &mut Scene, left: Rect, right: Rect, hot: bool) {
    let c = if hot { th.accent } else { th.ink_soft };
    let (ly, ry) = (left.y + left.h / 2.0, right.y + right.h / 2.0);
    let (lx, rx) = (left.x + 9.0, right.x + right.w - 9.0);
    s.solid(Path::new().move_to(pt(lx - 3.5, ly)).line_to(pt(lx + 3.0, ly - 4.5)).line_to(pt(lx + 3.0, ly + 4.5)).close(), c);
    s.solid(Path::new().move_to(pt(rx + 3.5, ry)).line_to(pt(rx - 3.0, ry - 4.5)).line_to(pt(rx - 3.0, ry + 4.5)).close(), c);
}

fn readout_text(th: &Theme, s: &mut Scene, win: Rect, text: &str) {
    let ty = &th.type_;
    s.text_in(
        pt(win.x + win.w / 2.0, win.y + win.h / 2.0 + ty.value_size * 0.36),
        text,
        ty.value_size,
        th.phosphor,
        ty.value.tracking(0.04),
    );
}

impl Part for Stepper {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let (win, _, _) = readout(r);
        bake_window(cx, win, 5.0, 3.0);
        cx.scene.text_in(pt(r.x + r.w / 2.0, r.y + 12.0), legend_of(self.label, self.bind), ty.legend_size, th.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let (win, left, right) = readout(r);
        readout_text(f.theme, s, win, &f.controller.value_text(self.bind.id));
        glass_over(f, s, win, 5.0);
        arrows(f.theme, s, left, right, f.focus.hover || f.focus.active);
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        let (win, left, right) = readout(r);
        alloc::vec![
            Control {
                hit: Hit::Rect(win),
                gesture: Gesture::Stepped { bind: self.bind, pitch: 18.0 },
            },
            Control {
                hit: Hit::Rect(left),
                gesture: Gesture::Nudge { bind: self.bind, delta: -1 },
            },
            Control {
                hit: Hit::Rect(right),
                gesture: Gesture::Nudge { bind: self.bind, delta: 1 },
            },
        ]
    }
}

// ---------------------------------------------------------------- presets

/// The factory-preset readout: the current preset's name with ◀ ▶.
#[derive(Copy, Clone, Debug)]
pub struct Presets {
    pub width: f64,
}

impl Default for Presets {
    fn default() -> Self {
        Presets { width: 200.0 }
    }
}

impl Presets {
    pub fn node(self) -> FaceNode {
        leaf(self, self.width, 24.0 + READOUT_H + 8.0)
    }
}

impl Part for Presets {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let (win, _, _) = readout(r);
        bake_window(cx, win, 5.0, 3.0);
        cx.scene.text_in(pt(r.x + r.w / 2.0, r.y + 12.0), "PRESET", ty.legend_size, th.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let (win, left, right) = readout(r);
        let presets = f.controller.presets();
        let text = match f.controller.preset() {
            Some(i) => presets.get(i).map(|p| String::from(p.name)).unwrap_or_default(),
            None if presets.is_empty() => String::from("—"),
            None => String::from("INIT"),
        };
        readout_text(f.theme, s, win, &text.to_uppercase());
        glass_over(f, s, win, 5.0);
        arrows(f.theme, s, left, right, f.focus.hover || f.focus.active);
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        let (win, left, right) = readout(r);
        alloc::vec![
            Control { hit: Hit::Rect(win), gesture: Gesture::Preset { delta: 1 } },
            Control { hit: Hit::Rect(left), gesture: Gesture::Preset { delta: -1 } },
            Control { hit: Hit::Rect(right), gesture: Gesture::Preset { delta: 1 } },
        ]
    }
}

// ------------------------------------------------------------------ pager

/// A row of lit buttons flipping an editor-local page set. Pair with
/// [`crate::part::on_page`] on the content of each page.
#[derive(Clone, Debug)]
pub struct Pager {
    pub key: u32,
    pub labels: Vec<&'static str>,
}

const PAGE_W: f64 = 72.0;
const PAGE_H: f64 = 40.0;

impl Pager {
    pub fn new(key: u32, labels: &[&'static str]) -> Self {
        Pager {
            key,
            labels: labels.to_vec(),
        }
    }
    pub fn node(self) -> FaceNode {
        let w = PAGE_W * self.labels.len() as f64;
        leaf(self, w, PAGE_H).width(Dim::Px(w))
    }
    fn cell(r: Rect, i: usize) -> Rect {
        Rect { x: r.x + PAGE_W * i as f64, y: r.y, w: PAGE_W, h: PAGE_H }
    }
}

impl Part for Pager {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let scale = cx.scale;
        let seat = material::sprite(14.0, scale, |dx, dy| material::lamp_seat(dx, dy, 6.0, scale));
        for (i, l) in self.labels.iter().enumerate() {
            let c = Self::cell(r, i);
            cx.image(c.x + c.w / 2.0 - 7.0, c.y + 3.0, 14.0, 14.0, seat.clone());
            cx.scene.text_in(pt(c.x + c.w / 2.0, c.y + 33.0), l.to_uppercase(), ty.caption_size, th.ink_soft, ty.caption);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let ty = &th.type_;
        let at = f.page(self.key) as usize;
        if let Some(l) = self.labels.get(at) {
            let c = Self::cell(r, at);
            lamp(f, s, pt(c.x + c.w / 2.0, c.y + 10.0));
            let mut font = ty.caption;
            font.weight = 750;
            s.text_in(pt(c.x + c.w / 2.0, c.y + 33.0), l.to_uppercase(), ty.caption_size, th.accent, font);
        }
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        (0..self.labels.len())
            .map(|i| Control {
                hit: Hit::Rect(Self::cell(r, i)),
                gesture: Gesture::Page { key: self.key, page: i as u32 },
            })
            .collect()
    }
}
