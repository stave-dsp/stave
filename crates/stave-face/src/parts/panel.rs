//! The enclosure and the recessed plates that group controls on it.

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec;

use stave_ui::layout::Rect;
use stave_ui::photo::{mix, mul, smoothstep, Rgb};
use stave_ui::{Dim, Edges};

use crate::material::{self, cov, RRect};
use crate::part::{Bake, FaceNode, Part};

/// The enclosure: a lit case on a desk, wrapping the whole face.
/// `margin` is the desk showing around the case; `pad` is the case's
/// own inner margin.
#[derive(Copy, Clone, Debug)]
pub struct Panel {
    pub margin: f64,
    pub pad: f64,
    /// Corner screws.
    pub screws: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            margin: 22.0,
            pad: 28.0,
            screws: true,
        }
    }
}

impl Panel {
    pub fn around(self, content: FaceNode) -> FaceNode {
        let bg = stave_ui::widget(Arc::new(PanelPart(self)) as Arc<dyn Part>)
            .width(Dim::Fill(1.0))
            .height(Dim::Fill(1.0));
        stave_ui::stack(vec![bg, content.margin_edges(Edges::all(self.margin + self.pad))])
    }
}

struct PanelPart(Panel);

/// How far the case's front wall shows below it (a hint of depth).
const WALL: f64 = 7.0;

impl Part for PanelPart {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let m = self.0.margin;
        let case = RRect::new(r.x + m, r.y + m, r.w - 2.0 * m, r.h - 2.0 * m - WALL, th.radius);
        let (fw, fh) = cx.face;
        let (surface, finish, desk_c) = (th.surface, th.finish, th.desk);
        let shadows = cx.shadows;
        let scale = cx.scale;
        let wall_c: Rgb = mul(surface, 0.62);
        let img = material::bake(r.w, r.h, scale, |lx, ly| {
            let (x, y) = (lx + r.x, ly + r.y);
            let mut c = material::desk(desk_c, x, y, fw, fh, &case.shifted(0.0, WALL / 2.0));
            let dw = case.shifted(0.0, WALL).sd(x, y);
            let d = case.sd(x, y);
            if dw < 1.0 && d > -1.0 {
                let t = ((y - (case.cy + case.hh - 12.0)) / (WALL + 12.0)).clamp(0.0, 1.0);
                c = mix(c, mul(wall_c, 0.95 - 0.4 * t), cov(dw, scale));
            }
            if d >= 1.0 {
                return c;
            }
            let n = case.bevel(x, y, 9.0);
            let fall = 1.06 - 0.16 * (((x - case.cx) / (2.0 * case.hw)) * 0.4 + ((y - case.cy) / (2.0 * case.hh)) * 0.6);
            let lit = mul(material::surface(finish, surface, x, y, n), fall * shadows.light(x, y));
            mix(c, lit, cov(d, scale))
        });
        cx.image(r.x, r.y, r.w, r.h, img);
        if self.0.screws {
            let inset = 13.0;
            let sr = 5.0;
            let screw = material::sprite(2.0 * sr + 2.0, scale, |dx, dy| material::screw(dx, dy, sr, scale));
            let (x0, y0, x1, y1) = (
                case.cx - case.hw + inset,
                case.cy - case.hh + inset,
                case.cx + case.hw - inset,
                case.cy + case.hh - inset,
            );
            for (sx, sy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                cx.image(sx - sr - 1.0, sy - sr - 1.0, 2.0 * sr + 2.0, 2.0 * sr + 2.0, screw.clone());
            }
        }
    }
}

/// A recessed plate grouping controls, optionally titled.
#[derive(Copy, Clone, Debug, Default)]
pub struct Plate {
    pub title: Option<&'static str>,
}

impl Plate {
    pub fn titled(title: &'static str) -> Self {
        Plate { title: Some(title) }
    }
    pub fn around(self, content: FaceNode) -> FaceNode {
        let bg = stave_ui::widget(Arc::new(PlatePart(self)) as Arc<dyn Part>)
            .width(Dim::Fill(1.0))
            .height(Dim::Fill(1.0));
        let top = if self.title.is_some() { 30.0 } else { 16.0 };
        // A plate takes its share of the row and centres what it holds.
        stave_ui::stack(vec![bg, content.margin_edges(Edges { t: top, r: 18.0, b: 14.0, l: 18.0 })])
            .width(Dim::Fill(1.0))
            .justify(stave_ui::Justify::Center)
    }
}

struct PlatePart(Plate);

impl Part for PlatePart {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let plate = RRect::new(r.x, r.y, r.w, r.h, 9.0);
        let (base, finish, scale) = (th.plate, th.finish, cx.scale);
        let shadows = cx.shadows;
        let img = material::bake_rgba(r.w, r.h, scale, |lx, ly| {
            let (x, y) = (lx + r.x, ly + r.y);
            let d = plate.sd(x, y);
            if d > 1.0 {
                return ([0.0; 3], 0.0);
            }
            // The floor, shaded by the lip along its top-left edge.
            let floor = material::surface(finish, base, x, y, stave_ui::photo::v3(0.0, 0.0, 1.0));
            let inner = 1.0 - 0.5 * (1.0 - smoothstep(0.0, 7.0, -plate.shifted(2.5, 3.5).sd(x, y)));
            let mut c = mul(floor, inner * shadows.light(x, y));
            // The lip: a thin chamfer catching the light on the far edge.
            let rim = 1.0 - smoothstep(0.0, 1.6, -d);
            let lit_edge = smoothstep(-0.2, 0.6, ((x - plate.cx) / plate.hw) * 0.4 + ((y - plate.cy) / plate.hh) * 0.6);
            c = mix(c, mul(base, 0.55 + 0.9 * lit_edge), rim * 0.8);
            (c, cov(d, scale))
        });
        cx.image(r.x, r.y, r.w, r.h, img);
        if let Some(t) = self.0.title {
            let ty = &th.type_;
            let font = ty.caption.align(stave_ui::scene::TextAlign::Left);
            cx.scene
                .text_in(stave_ui::scene::pt(r.x + 16.0, r.y + 19.0), t, ty.caption_size, th.ink_soft, font);
        }
    }
}
