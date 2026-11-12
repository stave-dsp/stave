//! Choosing from a list: the rotary [`Selector`] (a pointer knob with the
//! options printed around it), the [`Stepper`] (a glass readout with
//! ◀ ▶ for long lists), the [`Presets`] readout, the [`Tabs`] that pick
//! a stepped parameter's option, and the [`Pager`] that flips
//! editor-local pages (the same lit row, choosing the face's page).

use std::string::String;
use std::vec::Vec;

use stave_core::params::{EnumParam, ParamEnum};
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Pt, Scene, Stroke, TextAlign};
use stave_ui::Fonts;

use crate::material::{self, Occluder};
use crate::part::{measured, step_of, Area, Bake, Bind, FaceNode, Frame, Gesture, Grab, Part, PartId, Reads};
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
/// The least width: the options print around the knob, out to here.
const SEL_W: f64 = 132.0;
/// From the knob's centre to the bottom edge (the lowest options).
const SEL_BELOW: f64 = 52.0;

/// A legend's row at the top of a part, and the room under it.
fn head(th: &Theme) -> f64 {
    th.line(th.typography.legend_size) + th.space(0) + 2.0
}

/// The legend's baseline in a part's head.
fn legend_y(r: Rect, th: &Theme) -> f64 {
    r.y + th.line(th.typography.legend_size) * 0.65
}

/// A head's legend is never wider than its part.
fn legend_w(th: &Theme, fonts: &dyn Fonts, legend: &str) -> f64 {
    let ty = &th.typography;
    fonts.width(legend, ty.legend_size, &ty.legend) + 2.0 * th.space(1)
}
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
        measured(self)
    }
    /// The knob's centre: under the legend's row, with the options'
    /// arc above it.
    fn centre(r: Rect, th: &Theme) -> Pt {
        pt(r.x + r.w / 2.0, r.y + head(th) + 36.0)
    }
    fn angle(i: u32, n: u32) -> f64 {
        let t = if n <= 1 { 0.5 } else { f64::from(i) / f64::from(n - 1) };
        (-90.0 - SEL_SWEEP / 2.0 + SEL_SWEEP * t).to_radians()
    }
}

impl Part for Selector {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        (SEL_W.max(legend_w(th, fonts, &legend_of(self.label, self.bind))), head(th) + 36.0 + SEL_BELOW)
    }

    fn occluders(&self, r: Rect, theme: &Theme, out: &mut Vec<Occluder>) {
        let c = Self::centre(r, theme);
        out.push(Occluder::Disc {
            cx: c.x,
            cy: c.y,
            r: SEL_R,
            height: 6.0,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let c = Self::centre(r, th);
        let (style, tint, scale) = (th.hardware.caps, th.hardware.cap, cx.scale);
        let size = 2.0 * SEL_R + 2.0;
        let cap = material::sprite(size, scale, |dx, dy| material::knob(style, tint, dx, dy, SEL_R, scale));
        cx.image(c.x - size / 2.0, c.y - size / 2.0, size, size, cap);
        cx.scene.text_in(pt(c.x, legend_y(r, th) - 1.0), legend_of(self.label, self.bind), ty.legend_size, th.palette.ink, ty.legend);
        let n = cx.controller.steps(self.bind.id).max(2);
        for i in 0..n {
            let a = Self::angle(i, n);
            let p = pt(c.x + a.cos() * (SEL_R + 6.0), c.y + a.sin() * (SEL_R + 6.0));
            cx.scene.solid(Path::circle(p, 1.1), th.palette.ink_soft);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let ty = &th.typography;
        let c = Self::centre(r, th);
        let n = f.controller.steps(self.bind.id).max(2);
        let cur = step_of(f.value(self.bind), n);
        let a = Self::angle(cur, n);
        if th.hardware.caps == CapStyle::Chicken {
            crate::parts::knob::chicken(f, s, c, SEL_R, a);
        } else {
            let tip = if th.hardware.caps == CapStyle::Aluminium { 0.66 } else { 0.8 };
            s.stroke(
                Path::new()
                    .move_to(pt(c.x + a.cos() * SEL_R * 0.2, c.y + a.sin() * SEL_R * 0.2))
                    .line_to(pt(c.x + a.cos() * SEL_R * tip, c.y + a.sin() * SEL_R * tip)),
                Stroke::round(th.hardware.pointer, 2.2),
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
                th.palette.accent
            } else {
                th.palette.ink_soft
            };
            s.text_in(pt(x, y), name, ty.caption_size, color, font);
        }
        if lit {
            s.solid(Path::circle(pt(c.x + a.cos() * (SEL_R + 6.0), c.y + a.sin() * (SEL_R + 6.0)), 2.0), th.palette.accent);
        }
    }

    fn grabs(&self, r: Rect, f: &Frame<'_>, out: &mut Vec<Grab>) {
        out.push(Grab {
            area: Area::Circle {
                c: Self::centre(r, f.theme),
                r: SEL_R + 10.0,
            },
            gesture: Gesture::Stepped {
                bind: self.bind,
                pitch: 18.0,
            },
        });
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
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
        measured(self)
    }
}

/// A readout cell's size: at least `width`, as wide as its legend, a
/// head over the window.
fn readout_size(th: &Theme, fonts: &dyn Fonts, width: f64, legend: &str) -> (f64, f64) {
    (width.max(legend_w(th, fonts, legend)), head(th) + READOUT_H + 8.0)
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
    let c = if hot { th.palette.accent } else { th.palette.ink_soft };
    let (ly, ry) = (left.y + left.h / 2.0, right.y + right.h / 2.0);
    let (lx, rx) = (left.x + 9.0, right.x + right.w - 9.0);
    s.solid(Path::new().move_to(pt(lx - 3.5, ly)).line_to(pt(lx + 3.0, ly - 4.5)).line_to(pt(lx + 3.0, ly + 4.5)).close(), c);
    s.solid(Path::new().move_to(pt(rx + 3.5, ry)).line_to(pt(rx - 3.0, ry - 4.5)).line_to(pt(rx - 3.0, ry + 4.5)).close(), c);
}

fn readout_text(th: &Theme, s: &mut Scene, win: Rect, text: &str) {
    let ty = &th.typography;
    s.text_in(
        pt(win.x + win.w / 2.0, win.y + win.h / 2.0 + ty.value_size * 0.36),
        text,
        ty.value_size,
        th.palette.phosphor,
        ty.value.tracking(0.04),
    );
}

impl Part for Stepper {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        readout_size(th, fonts, self.width, &legend_of(self.label, self.bind))
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let (win, _, _) = readout(r);
        bake_window(cx, win, 5.0, 3.0);
        cx.scene.text_in(pt(r.x + r.w / 2.0, legend_y(r, th)), legend_of(self.label, self.bind), ty.legend_size, th.palette.ink, ty.legend);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let (win, left, right) = readout(r);
        readout_text(f.theme, s, win, &f.controller.value_text(self.bind.id));
        glass_over(f, s, win, 5.0);
        arrows(f.theme, s, left, right, f.focus.hover || f.focus.active);
    }

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        let (win, left, right) = readout(r);
        out.extend([
            Grab {
                area: Area::Rect(win),
                gesture: Gesture::Stepped { bind: self.bind, pitch: 18.0 },
            },
            Grab {
                area: Area::Rect(left),
                gesture: Gesture::Nudge { bind: self.bind, delta: -1 },
            },
            Grab {
                area: Area::Rect(right),
                gesture: Gesture::Nudge { bind: self.bind, delta: 1 },
            },
        ]);
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
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
        measured(self)
    }
}

impl Part for Presets {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        readout_size(th, fonts, self.width, "PRESET")
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let (win, _, _) = readout(r);
        bake_window(cx, win, 5.0, 3.0);
        cx.scene.text_in(pt(r.x + r.w / 2.0, legend_y(r, th)), "PRESET", ty.legend_size, th.palette.ink, ty.legend);
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

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        let (win, left, right) = readout(r);
        out.extend([
            Grab { area: Area::Rect(win), gesture: Gesture::Preset { delta: 1 } },
            Grab { area: Area::Rect(left), gesture: Gesture::Preset { delta: -1 } },
            Grab { area: Area::Rect(right), gesture: Gesture::Preset { delta: 1 } },
        ]);
    }

    fn id(&self) -> Option<PartId> {
        Some(PartId::Name("presets"))
    }
}

// ------------------------------------------------------------- lit rows

/// The least width of one button of a lit row ([`Pager`], [`Tabs`]).
const PAGE_W: f64 = 72.0;
/// A lit row button's lamp seat, from the top.
const PAGE_SEAT: f64 = 20.0;

/// A row of lit buttons, one per label, split evenly: what [`Pager`]
/// and [`Tabs`] share. The lit one is the choice.
#[derive(Clone, Debug)]
struct LitRow {
    labels: Vec<&'static str>,
}

impl LitRow {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let widest = self.labels.iter().map(|l| fonts.width(&l.to_uppercase(), ty.caption_size, &ty.caption)).fold(0.0, f64::max);
        let cell = PAGE_W.max(widest + 2.0 * th.space(1));
        (cell * self.labels.len() as f64, PAGE_SEAT + ty.caption_size * 2.75)
    }

    /// Button `i`: the part split evenly.
    fn cell(&self, r: Rect, i: usize) -> Rect {
        let w = r.w / self.labels.len().max(1) as f64;
        Rect { x: r.x + w * i as f64, y: r.y, w, h: r.h }
    }

    fn label_y(c: Rect, th: &Theme) -> f64 {
        c.y + PAGE_SEAT + th.typography.caption_size * 1.75
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let scale = cx.scale;
        let seat = material::sprite(14.0, scale, |dx, dy| material::lamp_seat(dx, dy, 6.0, scale));
        for (i, l) in self.labels.iter().enumerate() {
            let c = self.cell(r, i);
            cx.image(c.x + c.w / 2.0 - 7.0, c.y + 3.0, 14.0, 14.0, seat.clone());
            cx.scene.text_in(pt(c.x + c.w / 2.0, Self::label_y(c, th)), l.to_uppercase(), ty.caption_size, th.palette.ink_soft, ty.caption);
        }
    }

    /// Light button `at`.
    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene, at: usize) {
        let th = f.theme;
        let ty = &th.typography;
        if let Some(l) = self.labels.get(at) {
            let c = self.cell(r, at);
            lamp(f, s, pt(c.x + c.w / 2.0, c.y + 10.0));
            let mut font = ty.caption;
            font.weight = 750;
            s.text_in(pt(c.x + c.w / 2.0, Self::label_y(c, th)), l.to_uppercase(), ty.caption_size, th.palette.accent, font);
        }
    }

    fn grabs(&self, r: Rect, out: &mut Vec<Grab>, gesture: impl Fn(u32) -> Gesture) {
        out.extend((0..self.labels.len()).map(|i| Grab {
            area: Area::Rect(self.cell(r, i)),
            gesture: gesture(i as u32),
        }));
    }
}

// ------------------------------------------------------------------- tabs

/// A row of lit tabs choosing a stepped parameter's option, one tab per
/// step: the [`Pager`]'s look, but the choice is the plugin's — a tab
/// automates and saves like a knob. Pair with [`crate::part::on_step`]
/// to show only the chosen option's controls under it.
///
/// ```ignore
/// column(vec![Tabs::of(&p.algo).node(), follow_algo])
/// ```
#[derive(Clone, Debug)]
pub struct Tabs {
    pub bind: Bind,
    row: LitRow,
}

impl Tabs {
    /// Tabs for `bind`, labelled in step order (one label per step).
    pub fn new(bind: Bind, labels: &[&'static str]) -> Self {
        Tabs { bind, row: LitRow { labels: labels.to_vec() } }
    }

    /// Tabs for an enum parameter, labelled with its options' names.
    pub fn of<T: ParamEnum>(param: &EnumParam<T>) -> Self {
        let labels: Vec<&'static str> = (0..T::COUNT).map(T::name).collect();
        Self::new(crate::part::BindParam::bind(param), &labels)
    }

    pub fn node(self) -> FaceNode {
        measured(self)
    }
}

impl Part for Tabs {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        self.row.size(th, fonts)
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        self.row.bake(r, cx);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let at = step_of(f.value(self.bind), self.row.labels.len() as u32);
        self.row.paint(r, f, s, at as usize);
    }

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        let bind = self.bind;
        self.row.grabs(r, out, |step| Gesture::Choose { bind, step });
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.bind])
    }
}

// ------------------------------------------------------------------ pager

/// A row of lit buttons flipping an editor-local page set. Pair with
/// [`crate::part::on_page`] on the content of each page.
#[derive(Clone, Debug)]
pub struct Pager {
    pub key: u32,
    row: LitRow,
}

impl Pager {
    pub fn new(key: u32, labels: &[&'static str]) -> Self {
        Pager {
            key,
            row: LitRow { labels: labels.to_vec() },
        }
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
}

impl Part for Pager {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        self.row.size(th, fonts)
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        self.row.bake(r, cx);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        self.row.paint(r, f, s, f.page(self.key) as usize);
    }

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        let key = self.key;
        self.row.grabs(r, out, |page| Gesture::Page { key, page });
    }
}
