//! Instruments: the segmented level [`Meter`], the moving [`Trace`] of a
//! probe over time, and the XY [`Pad`].

extern crate alloc;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use std::sync::Mutex;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use crate::part::{leaf, Bake, Bind, Control, FaceNode, Frame, Gesture, Hit, Part};
use crate::parts::display::{bake_window, glass_over};
use crate::screens;

fn with_alpha(c: Color, a: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a * a)
}

// ------------------------------------------------------------------ meter

/// How a meter's probe reads.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Reading {
    /// The probe is in dB already.
    Db,
    /// The probe is a linear amplitude (1 = 0 dBFS).
    Linear,
    /// The probe is 0..1 (a gain reduction depth, a position).
    Unit,
}

/// A vertical LED bar meter reading a probe, with an optional legend.
#[derive(Copy, Clone, Debug)]
pub struct Meter {
    pub probe: &'static str,
    pub reading: Reading,
    /// dB at the bottom and top of the scale (Db/Linear readings).
    pub range: (f64, f64),
    pub label: Option<&'static str>,
    pub height: f64,
}

const SEGMENTS: usize = 24;

impl Meter {
    pub fn db(probe: &'static str) -> Self {
        Meter {
            probe,
            reading: Reading::Db,
            range: (-60.0, 6.0),
            label: None,
            height: 150.0,
        }
    }
    pub fn linear(probe: &'static str) -> Self {
        Meter {
            reading: Reading::Linear,
            ..Self::db(probe)
        }
    }
    pub fn unit(probe: &'static str) -> Self {
        Meter {
            reading: Reading::Unit,
            ..Self::db(probe)
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
        let extra = if self.label.is_some() { 20.0 } else { 0.0 };
        leaf(self, 30.0, self.height + extra)
    }
    fn window(&self, r: Rect) -> Rect {
        Rect { x: r.x + 8.0, y: r.y + 3.0, w: 14.0, h: self.height - 6.0 }
    }
    fn level(&self, v: f64) -> f64 {
        match self.reading {
            Reading::Unit => v.clamp(0.0, 1.0),
            Reading::Db | Reading::Linear => {
                let db = if self.reading == Reading::Linear { 20.0 * v.abs().max(1e-9).log10() } else { v };
                ((db - self.range.0) / (self.range.1 - self.range.0)).clamp(0.0, 1.0)
            }
        }
    }
}

impl Part for Meter {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        bake_window(cx, self.window(r), 3.0, 3.0);
        if let Some(l) = self.label {
            let th = cx.theme;
            let ty = &th.type_;
            cx.scene.text_in(pt(r.x + r.w / 2.0, r.y + r.h - 4.0), l, ty.caption_size, th.ink_soft, ty.caption);
        }
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let th = f.theme;
        let win = self.window(r);
        let lvl = self.level(f.probe(self.probe));
        let lit = (lvl * SEGMENTS as f64).round() as usize;
        let seg_h = (win.h - 4.0) / SEGMENTS as f64;
        for i in 0..SEGMENTS {
            let y = win.y + win.h - 2.0 - (i as f64 + 1.0) * seg_h;
            let hot = i >= SEGMENTS - 3;
            let base = if hot { Color::rgba(1.0, 0.32, 0.2, 1.0) } else { th.phosphor };
            let c = if i < lit { base } else { with_alpha(base, 0.1) };
            s.solid(Path::rect(pt(win.x + 2.5, y + 0.6), win.w - 5.0, seg_h - 1.2), c);
        }
        glass_over(f, s, win, 3.0);
    }

    fn probes(&self) -> Vec<&'static str> {
        alloc::vec![self.probe]
    }

    fn animating(&self, _f: &Frame<'_>) -> bool {
        false
    }
}

// ------------------------------------------------------------------ trace

/// A probe's recent history, scrolling on a screen.
pub struct Trace {
    pub probe: &'static str,
    pub w: f64,
    pub h: f64,
    /// The probe values at the bottom and top of the screen.
    pub range: (f64, f64),
    pub label: Option<&'static str>,
    history: Mutex<VecDeque<f64>>,
}

impl Trace {
    pub fn new(probe: &'static str, w: f64, h: f64, range: (f64, f64)) -> Self {
        Trace {
            probe,
            w,
            h,
            range,
            label: None,
            history: Mutex::new(VecDeque::new()),
        }
    }
    pub fn label(mut self, l: &'static str) -> Self {
        self.label = Some(l);
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
        let mut h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        h.push_back(f.probe(self.probe));
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
        if pts.len() > 1 {
            s.stroke(Path::polyline(&pts), Stroke::round(with_alpha(th.phosphor, 0.2), 4.0));
            s.stroke(Path::polyline(&pts), Stroke::round(th.phosphor, 1.4));
        }
        if let Some(l) = self.label {
            let ty = &th.type_;
            s.text_in(pt(sc.x + 8.0, sc.y + 13.0), l, ty.caption_size, with_alpha(th.phosphor, 0.7), ty.caption.align(stave_ui::scene::TextAlign::Left));
        }
        glass_over(f, s, sc, 6.0);
    }

    fn probes(&self) -> Vec<&'static str> {
        alloc::vec![self.probe]
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
        let faint = with_alpha(th.phosphor, 0.25);
        s.stroke(Path::new().move_to(pt(p.x, sc.y)).line_to(pt(p.x, sc.y + sc.h)), Stroke::new(faint, 1.0));
        s.stroke(Path::new().move_to(pt(sc.x, p.y)).line_to(pt(sc.x + sc.w, p.y)), Stroke::new(faint, 1.0));
        s.solid(Path::circle(p, 9.0), with_alpha(th.phosphor, 0.18));
        s.solid(Path::circle(p, 4.5), th.phosphor);
        let ty = &th.type_;
        let text = format_xy(f, self.x, self.y);
        s.text_in(pt(sc.x + sc.w - 8.0, sc.y + sc.h - 8.0), text, ty.caption_size, with_alpha(th.phosphor, 0.75), ty.value.align(stave_ui::scene::TextAlign::Right));
        glass_over(f, s, sc, 6.0);
    }

    fn controls(&self, r: Rect) -> Vec<Control> {
        let sc = self.screen(r);
        alloc::vec![Control {
            hit: Hit::Rect(sc),
            gesture: Gesture::Pad { x: self.x, y: self.y, area: sc },
        }]
    }
}

fn format_xy(f: &Frame<'_>, x: Bind, y: Bind) -> alloc::string::String {
    alloc::format!("{}  {}", f.host.value_text(x.id), f.host.value_text(y.id))
}

