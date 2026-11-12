//! The display: a recessed screen behind glass, for anything live a
//! plugin wants to draw (a scope, a spectrum, a spinning record, a
//! field of neurons). The plugin supplies the picture; the face supplies
//! the bezel, the screen and the glass.

extern crate alloc;
use alloc::vec::Vec;

use stave_ui::layout::Rect;
use stave_ui::scene::{Path, Scene};

use crate::material::{self, Occluder, RRect};
use crate::part::{leaf, sprite_key, Bake, Bind, FaceNode, Frame, Part};
use crate::theme::Theme;

/// A display's picture: draw into `screen` (face coordinates). Plain fn,
/// no captured state: the picture is a function of the frame alone.
pub type Draw = fn(&Frame<'_>, Rect, &mut Scene);

#[derive(Clone, Debug)]
pub struct Display {
    pub w: f64,
    pub h: f64,
    pub draw: Draw,
    /// Probes the picture reads.
    pub probes: &'static [&'static str],
    /// Parameters the picture reads.
    pub reads: Vec<Bind>,
    /// Repaint every frame (the picture moves with time alone).
    pub animated: bool,
}

const LIP: f64 = 5.0;
const RADIUS: f64 = 7.0;

impl Display {
    pub fn new(w: f64, h: f64, draw: Draw) -> Self {
        Display {
            w,
            h,
            draw,
            probes: &[],
            reads: Vec::new(),
            animated: false,
        }
    }
    pub fn probes(mut self, p: &'static [&'static str]) -> Self {
        self.probes = p;
        self
    }
    pub fn reads(mut self, b: &[Bind]) -> Self {
        self.reads = b.to_vec();
        self
    }
    pub fn animated(mut self) -> Self {
        self.animated = true;
        self
    }
    pub fn node(self) -> FaceNode {
        let (w, h) = (self.w + 2.0 * LIP, self.h + 2.0 * LIP);
        leaf(self, w, h)
    }
    fn screen(r: Rect) -> Rect {
        Rect {
            x: r.x + LIP,
            y: r.y + LIP,
            w: r.w - 2.0 * LIP,
            h: r.h - 2.0 * LIP,
        }
    }
}

impl Part for Display {
    fn occluders(&self, r: Rect, _theme: &Theme, out: &mut Vec<Occluder>) {
        out.push(Occluder::Rect {
            rect: RRect::new(r.x, r.y, r.w, r.h, RADIUS + LIP),
            height: 3.0,
        });
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        bake_window(cx, Self::screen(r), RADIUS, LIP);
    }

    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let sc = Self::screen(r);
        (self.draw)(f, sc, s);
        glass_over(f, s, sc, RADIUS);
    }

    fn probes(&self) -> Vec<&'static str> {
        self.probes.to_vec()
    }

    fn reads(&self) -> Vec<Bind> {
        self.reads.clone()
    }

    fn animating(&self, _f: &Frame<'_>) -> bool {
        self.animated
    }
}

/// Bake a recessed screen: a `lip`-wide bezel around `screen`, and the
/// screen's dark ground.
pub fn bake_window(cx: &mut Bake<'_>, screen: Rect, radius: f64, lip: f64) {
    let scale = cx.scale;
    let r = Rect {
        x: screen.x - lip,
        y: screen.y - lip,
        w: screen.w + 2.0 * lip,
        h: screen.h + 2.0 * lip,
    };
    let win = RRect::new(screen.x, screen.y, screen.w, screen.h, radius);
    let img = material::bake_rgba(r.w, r.h, scale, |lx, ly| {
        material::bezel(lx + r.x, ly + r.y, &win, lip, scale).unwrap_or(([0.0; 3], 0.0))
    });
    cx.image(r.x, r.y, r.w, r.h, img);
    cx.scene.solid(rrect_path(screen, radius), cx.theme.screen);
}

/// Lay the glass over a screen (after its live content).
pub fn glass_over(f: &Frame<'_>, s: &mut Scene, screen: Rect, radius: f64) {
    let key = sprite_key(("glass", screen.w.to_bits(), screen.h.to_bits(), radius.to_bits()));
    let scale = f.scale;
    let (w, h) = (screen.w, screen.h);
    let img = f.sprites.get(key, scale, || {
        let win = RRect::new(0.0, 0.0, w, h, radius);
        material::bake_rgba(w, h, scale, |x, y| material::glass(x, y, &win, scale).unwrap_or(([0.0; 3], 0.0)))
    });
    s.ops.push(stave_ui::scene::Op::Image {
        at: stave_ui::scene::pt(screen.x, screen.y),
        w,
        h,
        image: img,
    });
}

/// A rounded-rectangle path (cubic corners).
pub fn rrect_path(r: Rect, rad: f64) -> Path {
    use stave_ui::scene::pt;
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    let k = rad * (1.0 - 0.552_284_75);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    Path::new()
        .move_to(pt(x0 + rad, y0))
        .line_to(pt(x1 - rad, y0))
        .cubic_to(pt(x1 - k, y0), pt(x1, y0 + k), pt(x1, y0 + rad))
        .line_to(pt(x1, y1 - rad))
        .cubic_to(pt(x1, y1 - k), pt(x1 - k, y1), pt(x1 - rad, y1))
        .line_to(pt(x0 + rad, y1))
        .cubic_to(pt(x0 + k, y1), pt(x0, y1 - k), pt(x0, y1 - rad))
        .line_to(pt(x0, y0 + rad))
        .cubic_to(pt(x0, y0 + k), pt(x0 + k, y0), pt(x0 + rad, y0))
        .close()
}
