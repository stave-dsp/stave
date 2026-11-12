//! Instruments: the segmented level [`Meter`] (and the gain-reduction
//! meter that hangs from the top), the moving [`Trace`] of a tap over
//! time (bipolar about a centre line, for a modulation source), and the
//! XY [`Pad`].

use std::collections::VecDeque;
use std::vec::Vec;
use std::sync::Mutex;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use stave_core::telemetry::TapId;

use stave_ui::Fonts;

use crate::part::{leaf, measured, Area, Bake, Bind, FaceNode, Frame, Gesture, Grab, Part, Reads};
use crate::parts::display::{bake_window, glass_over};
use crate::screens;
use crate::theme::Theme;

fn with_alpha(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a * a)
}

// ------------------------------------------------------------------ meter

/// How a meter's tap reads.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Reading {
    /// The tap is in dB already.
    Db,
    /// The tap is a linear amplitude (1 = 0 dBFS).
    Linear,
    /// The tap is 0..1 (a depth, a position).
    Unit,
    /// The tap is gain reduction in dB (its size: −6 and 6 both read as
    /// 6 dB of reduction), over `range`'s (0, most).
    Reduction,
}

/// Which end of a meter's bar its reading grows from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Hang {
    /// A level: lit from the bottom up.
    Bottom,
    /// Gain reduction: lit from the top down.
    Top,
}

/// A vertical LED bar meter reading a tap, with an optional legend.
#[derive(Copy, Clone, Debug)]
pub struct Meter {
    pub tap: TapId,
    pub reading: Reading,
    /// dB at the bottom and top of the scale (Db/Linear readings).
    pub range: (f64, f64),
    pub label: Option<&'static str>,
    pub height: f64,
    /// The end the bar grows from.
    pub hang: Hang,
}

const SEGMENTS: usize = 24;
/// The bar's window width.
const METER_W: f64 = 14.0;

impl Meter {
    pub fn db(tap: TapId) -> Self {
        Meter {
            tap,
            reading: Reading::Db,
            range: (-60.0, 6.0),
            label: None,
            height: 150.0,
            hang: Hang::Bottom,
        }
    }
    /// A gain-reduction meter: the tap's reduction in dB, 0 to 24 dB,
    /// hanging from the top.
    pub fn reduction(tap: TapId) -> Self {
        Meter {
            reading: Reading::Reduction,
            range: (0.0, 24.0),
            hang: Hang::Top,
            ..Self::db(tap)
        }
    }
    /// The dB at the bottom and top of the scale (for a reduction
    /// meter: none and the most it shows).
    pub fn range(mut self, lo: f64, hi: f64) -> Self {
        self.range = (lo, hi);
        self
    }
    pub fn linear(tap: TapId) -> Self {
        Meter {
            reading: Reading::Linear,
            ..Self::db(tap)
        }
    }
    pub fn unit(tap: TapId) -> Self {
        Meter {
            reading: Reading::Unit,
            ..Self::db(tap)
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn height(mut self, h: f64) -> Self {
        self.height = h;
        self
    }
    pub fn node(self) -> FaceNode {
        measured(self)
    }
    /// The bar's window, centred across the part.
    fn window(&self, r: Rect) -> Rect {
        Rect { x: r.x + (r.w - METER_W) / 2.0, y: r.y + 3.0, w: METER_W, h: self.height - 6.0 }
    }
    fn level(&self, v: f64) -> f64 {
        match self.reading {
            Reading::Unit => v.clamp(0.0, 1.0),
            Reading::Reduction => ((v.abs() - self.range.0) / (self.range.1 - self.range.0)).clamp(0.0, 1.0),
            Reading::Db | Reading::Linear => {
                let db = if self.reading == Reading::Linear { 20.0 * v.abs().max(1e-9).log10() } else { v };
                ((db - self.range.0) / (self.range.1 - self.range.0)).clamp(0.0, 1.0)
            }
        }
    }
}

impl Part for Meter {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let w = METER_W + 4.0 * th.space(0);
        match self.label {
            Some(l) => {
                let lw = fonts.width(l, ty.caption_size, &ty.caption) + th.space(0);
                (w.max(lw), self.height + th.line(ty.caption_size) + 1.5 * th.space(0))
            }
            None => (w, self.height),
        }
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        bake_window(cx, self.window(r), 3.0, 3.0);
        if let Some(l) = self.label {
            let th = cx.theme;
            let ty = &th.typography;
            cx.scene.text_in(pt(r.x + r.w / 2.0, r.y + r.h - th.space(0)), l, ty.caption_size, th.palette.ink_soft, ty.caption);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let win = self.window(r);
        let lvl = self.level(f.tap(self.tap));
        let lit = (lvl * SEGMENTS as f64).round() as usize;
        let seg_h = (win.h - 4.0) / SEGMENTS as f64;
        for i in 0..SEGMENTS {
            // Segment i counts from the end the bar hangs from.
            let y = match self.hang {
                Hang::Bottom => win.y + win.h - 2.0 - (i as f64 + 1.0) * seg_h,
                Hang::Top => win.y + 2.0 + i as f64 * seg_h,
            };
            // A level runs hot at its top; reduction is never "hot".
            let hot = self.hang == Hang::Bottom && i >= SEGMENTS - 3;
            let base = if hot { Color::rgba(1.0, 0.32, 0.2, 1.0) } else { th.palette.phosphor };
            let c = if i < lit { base } else { with_alpha(base, 0.1) };
            s.solid(Path::rect(pt(win.x + 2.5, y + 0.6), win.w - 5.0, seg_h - 1.2), c);
        }
        glass_over(f, s, win, 3.0);
    }

    fn reads(&self) -> Reads {
        Reads::taps(&[self.tap])
    }

    fn animating(&self, _f: &Frame<'_>) -> bool {
        false
    }
}

// ------------------------------------------------------------------ trace

/// A tap's recent history, scrolling on a screen.
pub struct Trace {
    pub tap: TapId,
    pub w: f64,
    pub h: f64,
    /// The tap values at the bottom and top of the screen.
    pub range: (f64, f64),
    pub label: Option<&'static str>,
    /// Draw about a centre line (the middle of `range`), filled to it: a
    /// modulation source swinging either side of zero.
    pub bipolar: bool,
    history: Mutex<History>,
}

/// A trace's samples, one per frame time: painting the same frame twice
/// (a hit test, a second layer pass) draws the same picture.
#[derive(Default)]
struct History {
    /// The frame time of the newest sample.
    at: Option<f64>,
    values: VecDeque<f64>,
}

impl Trace {
    pub fn new(tap: TapId, w: f64, h: f64, range: (f64, f64)) -> Self {
        Trace {
            tap,
            w,
            h,
            range,
            label: None,
            bipolar: false,
            history: Mutex::new(History::default()),
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
        self
    }
    /// About a centre line (see [`bipolar`](Self::bipolar)).
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }
    pub fn node(self) -> FaceNode {
        let (w, h) = (self.w + 6.0, self.h + 6.0);
        leaf(self, w, h)
    }
    fn screen(&self, r: Rect) -> Rect {
        Rect { x: r.x + 3.0, y: r.y + 3.0, w: self.w, h: self.h }
    }
}

impl Part for Trace {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        bake_window(cx, self.screen(r), 6.0, 3.0);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let sc = self.screen(r);
        screens::grid(f, sc, s, 8, 4);
        let n = (sc.w / 2.0) as usize;
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        if history.at != Some(f.time) {
            history.at = Some(f.time);
            history.values.push_back(f.tap(self.tap));
        }
        let h = &mut history.values;
        while h.len() > n {
            h.pop_front();
        }
        let (lo, hi) = self.range;
        let pts: Vec<_> = h
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                pt(sc.x + sc.w - (h.len() - 1 - i) as f64 * 2.0, sc.y + sc.h - 4.0 - t * (sc.h - 8.0))
            })
            .collect();
        if self.bipolar {
            let mid = sc.y + sc.h / 2.0;
            s.stroke(Path::new().move_to(pt(sc.x, mid)).line_to(pt(sc.x + sc.w, mid)), Stroke::new(with_alpha(th.palette.phosphor, 0.35), 1.0));
            if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                let mut area = Path::new().move_to(pt(first.x, mid));
                for &p in &pts {
                    area = area.line_to(p);
                }
                s.solid(area.line_to(pt(last.x, mid)).close(), with_alpha(th.palette.phosphor, 0.16));
            }
        }
        if pts.len() > 1 {
            s.stroke(Path::polyline(&pts), Stroke::round(with_alpha(th.palette.phosphor, 0.2), 4.0));
            s.stroke(Path::polyline(&pts), Stroke::round(th.palette.phosphor, 1.4));
        }
        if let Some(l) = self.label {
            let ty = &th.typography;
            s.text_in(pt(sc.x + 8.0, sc.y + 13.0), l, ty.caption_size, with_alpha(th.palette.phosphor, 0.7), ty.caption.align(stave_ui::scene::TextAlign::Left));
        }
        glass_over(f, s, sc, 6.0);
    }

    fn reads(&self) -> Reads {
        Reads::taps(&[self.tap])
    }

    fn animating(&self, _f: &Frame<'_>) -> bool {
        true
    }
}

// -------------------------------------------------------------------- pad

/// Two parameters on a plane: a lit puck on a screen, x across and y up.
#[derive(Copy, Clone, Debug)]
pub struct Pad {
    pub x: Bind,
    pub y: Bind,
    pub w: f64,
    pub h: f64,
}

impl Pad {
    pub fn new(x: Bind, y: Bind, w: f64, h: f64) -> Self {
        Pad { x, y, w, h }
    }
    pub fn node(self) -> FaceNode {
        let (w, h) = (self.w + 6.0, self.h + 6.0);
        leaf(self, w, h)
    }
    fn screen(&self, r: Rect) -> Rect {
        Rect { x: r.x + 3.0, y: r.y + 3.0, w: self.w, h: self.h }
    }
}

impl Part for Pad {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        bake_window(cx, self.screen(r), 6.0, 3.0);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let sc = self.screen(r);
        screens::grid(f, sc, s, 6, 6);
        let (vx, vy) = (f.value(self.x), f.value(self.y));
        let p = pt(sc.x + vx * sc.w, sc.y + (1.0 - vy) * sc.h);
        let faint = with_alpha(th.palette.phosphor, 0.25);
        s.stroke(Path::new().move_to(pt(p.x, sc.y)).line_to(pt(p.x, sc.y + sc.h)), Stroke::new(faint, 1.0));
        s.stroke(Path::new().move_to(pt(sc.x, p.y)).line_to(pt(sc.x + sc.w, p.y)), Stroke::new(faint, 1.0));
        s.solid(Path::circle(p, 9.0), with_alpha(th.palette.phosphor, 0.18));
        s.solid(Path::circle(p, 4.5), th.palette.phosphor);
        let ty = &th.typography;
        let text = format_xy(f, self.x, self.y);
        s.text_in(pt(sc.x + sc.w - 8.0, sc.y + sc.h - 8.0), text, ty.caption_size, with_alpha(th.palette.phosphor, 0.75), ty.value.align(stave_ui::scene::TextAlign::Right));
        glass_over(f, s, sc, 6.0);
    }

    fn grabs(&self, r: Rect, _f: &Frame<'_>, out: &mut Vec<Grab>) {
        let sc = self.screen(r);
        out.push(Grab {
            area: Area::Rect(sc),
            gesture: Gesture::Pad { x: self.x, y: self.y, area: sc },
        });
    }

    fn reads(&self) -> Reads {
        Reads::params(&[self.x, self.y])
    }
}

fn format_xy(f: &Frame<'_>, x: Bind, y: Bind) -> std::string::String {
    std::format!("{}  {}", f.controller.value_text(x.id), f.controller.value_text(y.id))
}

