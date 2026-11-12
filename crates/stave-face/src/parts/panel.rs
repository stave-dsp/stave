//! The enclosure and the recessed plates that group controls on it.

use std::sync::Arc;
use std::vec;
use std::vec::Vec;

use stave_ui::layout::Rect;
use crate::photo::{mix, mul, smoothstep, Rgb};
use stave_ui::{Dim, Edges};

use crate::material::{self, cov, RRect};
use crate::part::{Bake, FaceNode, Part, PartId};

/// What kind of hardware the face is.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Form {
    /// A die-cast pedal or desktop case: a lit, bevelled box with corner
    /// screws, controls on its top.
    Pedal,
    /// A 19" rack unit: a faceplate between rack ears with slotted
    /// mounting holes, at least `units` rack units tall.
    Rack { units: u32, handles: bool },
    /// An amp head: a tolex-covered box, its controls on a faceplate
    /// framed in piping, with an optional band of grille cloth.
    AmpHead { grille: Option<Grille> },
    /// A speaker cabinet: tolex around a large grille, the controls on a
    /// strip below it.
    Cabinet { grille: Grille },
}

/// Which side of a faceplate a grille band sits on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
}

/// A band of grille cloth, with an optional logo badge on it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Grille {
    pub side: Side,
    pub height: f64,
    pub badge: Option<&'static str>,
}

/// The enclosure, wrapping the whole face: the hardware form, the desk
/// showing around it (`margin`), and the faceplate's inner margin (`pad`).
#[derive(Copy, Clone, Debug)]
pub struct Panel {
    pub form: Form,
    pub margin: f64,
    pub pad: f64,
    /// Corner screws (pedal form).
    pub screws: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Panel::pedal()
    }
}

/// One rack unit, in points.
pub const RACK_U: f64 = 66.0;
/// Width of a rack ear.
const EAR: f64 = 38.0;
/// Tolex border around an amp's faceplate.
const TOLEX: f64 = 26.0;

impl Panel {
    pub fn pedal() -> Self {
        Panel {
            form: Form::Pedal,
            margin: 22.0,
            pad: 28.0,
            screws: true,
        }
    }
    pub fn rack(units: u32) -> Self {
        Panel {
            form: Form::Rack { units, handles: false },
            margin: 18.0,
            pad: 18.0,
            screws: false,
        }
    }
    /// Rack handles beside the ears (only on a rack form).
    pub fn handles(mut self) -> Self {
        if let Form::Rack { units, .. } = self.form {
            self.form = Form::Rack { units, handles: true };
        }
        self
    }
    pub fn amp_head() -> Self {
        Panel {
            form: Form::AmpHead { grille: None },
            margin: 22.0,
            pad: 22.0,
            screws: false,
        }
    }
    /// A grille band on an amp head (or the cabinet's grille).
    pub fn grille(mut self, side: Side, height: f64, badge: Option<&'static str>) -> Self {
        let g = Grille { side, height, badge };
        match self.form {
            Form::AmpHead { .. } => self.form = Form::AmpHead { grille: Some(g) },
            Form::Cabinet { .. } => self.form = Form::Cabinet { grille: g },
            _ => {}
        }
        self
    }
    pub fn cabinet() -> Self {
        Panel {
            form: Form::Cabinet {
                grille: Grille {
                    side: Side::Top,
                    height: 260.0,
                    badge: None,
                },
            },
            margin: 22.0,
            pad: 18.0,
            screws: false,
        }
    }

    /// Insets from the panel's edge to the faceplate (where the content
    /// sits, before `pad`): (top, right, bottom, left).
    fn plate_insets(&self) -> Edges {
        let m = self.margin;
        match self.form {
            Form::Pedal => Edges::all(m),
            Form::Rack { .. } => Edges { t: m, r: m + EAR, b: m, l: m + EAR },
            Form::AmpHead { grille } => {
                let g = grille.map_or(0.0, |g| g.height + 14.0);
                let (t, b) = match grille.map(|g| g.side) {
                    Some(Side::Top) => (g, 0.0),
                    Some(Side::Bottom) => (0.0, g),
                    None => (0.0, 0.0),
                };
                Edges { t: m + TOLEX + t, r: m + TOLEX, b: m + TOLEX + b, l: m + TOLEX }
            }
            Form::Cabinet { grille } => {
                let g = grille.height + 14.0;
                let (t, b) = if grille.side == Side::Top { (g, 0.0) } else { (0.0, g) };
                Edges { t: m + TOLEX + t, r: m + TOLEX, b: m + TOLEX + b, l: m + TOLEX }
            }
        }
    }

    pub fn around(self, content: FaceNode) -> FaceNode {
        let bg = stave_ui::widget(Arc::new(PanelPart(self)) as Arc<dyn Part>)
            .width(Dim::Fill(1.0))
            .height(Dim::Fill(1.0));
        let e = self.plate_insets();
        let p = self.pad;
        let mut kids = vec![bg, content.margin_edges(Edges { t: e.t + p, r: e.r + p, b: e.b + p, l: e.l + p })];
        if let Form::Rack { units, .. } = self.form {
            // A strut: at least `units` rack units tall.
            kids.push(stave_ui::boxed(vec![]).size(0.0, units as f64 * RACK_U + 2.0 * self.margin));
        }
        stave_ui::stack(kids).align(stave_ui::Align::Center)
    }
}

struct PanelPart(Panel);

/// How far a pedal case's front wall shows below it (a hint of depth).
const WALL: f64 = 7.0;

impl Part for PanelPart {
    fn id(&self) -> Option<PartId> {
        Some(PartId::Name("panel"))
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        match self.0.form {
            Form::Pedal => self.bake_pedal(r, cx),
            Form::Rack { handles, .. } => self.bake_rack(r, cx, handles),
            Form::AmpHead { grille } => self.bake_amp(r, cx, grille),
            Form::Cabinet { grille } => self.bake_amp(r, cx, Some(grille)),
        }
    }
}

impl PanelPart {
    fn bake_pedal(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let m = self.0.margin;
        let case = RRect::new(r.x + m, r.y + m, r.w - 2.0 * m, r.h - 2.0 * m - WALL, th.finish.radius);
        let (fw, fh) = cx.face;
        let (surface, finish, desk_c) = (th.finish.surface, th.finish.coat, th.finish.desk);
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

    fn bake_rack(&self, r: Rect, cx: &mut Bake<'_>, handles: bool) {
        let th = cx.theme;
        let m = self.0.margin;
        let plate = RRect::new(r.x + m, r.y + m, r.w - 2.0 * m, r.h - 2.0 * m, 3.0);
        let (fw, fh) = cx.face;
        let (surface, finish, desk_c) = (th.finish.surface, th.finish.coat, th.finish.desk);
        let shadows = cx.shadows;
        let scale = cx.scale;
        // The ears are the same plate; a faint seam marks where the
        // chassis behind them begins.
        let (seam_l, seam_r) = (plate.cx - plate.hw + EAR, plate.cx + plate.hw - EAR);
        let img = material::bake(r.w, r.h, scale, |lx, ly| {
            let (x, y) = (lx + r.x, ly + r.y);
            let c = material::desk(desk_c, x, y, fw, fh, &plate);
            let d = plate.sd(x, y);
            if d >= 1.0 {
                return c;
            }
            let n = plate.bevel(x, y, 3.0);
            let mut lit = mul(material::surface(finish, surface, x, y, n), shadows.light(x, y));
            let seam = ((x - seam_l).abs()).min((x - seam_r).abs());
            lit = mul(lit, 1.0 - 0.35 * (1.0 - smoothstep(0.3, 1.1, seam)));
            mix(c, lit, cov(d, scale))
        });
        cx.image(r.x, r.y, r.w, r.h, img);
        // Mounting slots, one rack unit apart, in each ear.
        let (hl, sr) = (6.0, 3.6);
        let slot = material::sprite(2.0 * (hl + sr) + 4.0, scale, |dx, dy| material::rack_slot(dx, dy, hl, sr, scale));
        let ss = 2.0 * (hl + sr) + 4.0;
        let top = plate.cy - plate.hh;
        let bottom = plate.cy + plate.hh;
        let mut ys = Vec::new();
        let mut y = top + RACK_U * 0.25;
        while y < bottom - RACK_U * 0.15 {
            ys.push(y);
            y += RACK_U;
        }
        if ys.len() == 1 {
            ys.push(bottom - RACK_U * 0.25);
        } else if let Some(last) = ys.last_mut() {
            *last = (*last).max(bottom - RACK_U * 0.25);
        }
        for ex in [plate.cx - plate.hw + EAR / 2.0, plate.cx + plate.hw - EAR / 2.0] {
            for &sy in &ys {
                cx.image(ex - ss / 2.0, sy - ss / 2.0, ss, ss, slot.clone());
            }
        }
        if handles {
            let len = (plate.hh * 2.0 - 30.0).max(20.0);
            let hw = 14.0;
            let hh = len + 14.0;
            let handle = material::bake_rgba(hw, hh, scale, |x, y| {
                material::rack_handle(x - hw / 2.0, y - hh / 2.0, len, 4.5, scale).unwrap_or(([0.0; 3], 0.0))
            });
            for hx in [seam_l + 12.0, seam_r - 12.0] {
                cx.image(hx - hw / 2.0, plate.cy - hh / 2.0, hw, hh, handle.clone());
            }
        }
    }

    fn bake_amp(&self, r: Rect, cx: &mut Bake<'_>, grille: Option<Grille>) {
        let th = cx.theme;
        let m = self.0.margin;
        let bx = RRect::new(r.x + m, r.y + m, r.w - 2.0 * m, r.h - 2.0 * m, 9.0);
        let e = self.0.plate_insets();
        let face = RRect::new(r.x + e.l, r.y + e.t, r.w - e.l - e.r, r.h - e.t - e.b, 5.0);
        let grille_rect = grille.map(|g| {
            let h = g.height;
            let y = match g.side {
                Side::Top => face.cy - face.hh - 14.0 - h,
                Side::Bottom => face.cy + face.hh + 14.0,
            };
            RRect::new(face.cx - face.hw, y, face.hw * 2.0, h, 5.0)
        });
        let (fw, fh) = cx.face;
        let (surface, finish, desk_c, tolex, pipe, cloth) = (th.finish.surface, th.finish.coat, th.finish.desk, th.coverings.tolex, th.coverings.piping, th.coverings.grille);
        let shadows = cx.shadows;
        let scale = cx.scale;
        let img = material::bake(r.w, r.h, scale, |lx, ly| {
            let (x, y) = (lx + r.x, ly + r.y);
            let mut c = material::desk(desk_c, x, y, fw, fh, &bx);
            let d = bx.sd(x, y);
            if d >= 1.0 {
                return c;
            }
            // The box: tolex wrapped over rounded edges.
            let n = bx.bevel(x, y, 10.0);
            let fall = 1.05 - 0.14 * ((y - bx.cy) / (2.0 * bx.hh));
            c = mix(c, mul(material::tolex(tolex, x, y, n), fall * shadows.light(x, y)), cov(d, scale));
            // The grille band, recessed behind its own piping.
            if let Some(g) = grille_rect {
                let dg = g.sd(x, y);
                if dg < 1.0 {
                    let inner = 1.0 - 0.45 * (1.0 - smoothstep(0.0, 9.0, -g.shifted(2.0, 3.0).sd(x, y)));
                    c = mix(c, mul(material::grille(cloth, x, y), inner), cov(dg, scale));
                }
                if let Some((pc, a)) = material::piping(pipe, g.sd(x, y) - 3.0, 2.2, grad(&g, x, y), scale) {
                    c = mix(c, pc, a);
                }
            }
            // The faceplate, raised a touch, framed in piping.
            let dp = face.sd(x, y);
            if dp < 1.0 {
                let pn = face.bevel(x, y, 2.5);
                let lit = mul(material::surface(finish, surface, x, y, pn), shadows.light(x, y));
                c = mix(c, lit, cov(dp, scale));
            }
            if let Some((pc, a)) = material::piping(pipe, dp - 4.5, 2.2, grad(&face, x, y), scale) {
                c = mix(c, pc, a);
            }
            c
        });
        cx.image(r.x, r.y, r.w, r.h, img);
        // Corner protectors.
        let cs = 22.0;
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            let (cx0, cy0) = (bx.cx - sx * bx.hw, bx.cy - sy * bx.hh);
            let size = cs + 4.0;
            let img = material::bake_rgba(size, size, scale, |x, y| {
                let (dx, dy) = (if sx > 0.0 { x } else { x - size }, if sy > 0.0 { y } else { y - size });
                material::corner(dx, dy, cs, sx, sy, scale).unwrap_or(([0.0; 3], 0.0))
            });
            let (ix, iy) = (if sx > 0.0 { cx0 } else { cx0 - size }, if sy > 0.0 { cy0 } else { cy0 - size });
            cx.image(ix, iy, size, size, img);
        }
        // The logo badge on the grille.
        if let (Some(g), Some(text)) = (grille_rect, grille.and_then(|g| g.badge)) {
            let ty = &th.typography;
            let size = (g.hh * 0.55).clamp(14.0, 40.0);
            let w = text.chars().count() as f64 * size * 0.62 + size * 1.2;
            let h = size * 1.5;
            let (bx0, by0) = (g.cx - w / 2.0, g.cy - h / 2.0);
            let plate = RRect::new(0.0, 0.0, w, h, h * 0.3);
            let metal = th.coverings.badge;
            let img = material::bake_rgba(w, h, scale, |x, y| {
                let d = plate.sd(x, y);
                if d > 0.5 {
                    return ([0.0; 3], 0.0);
                }
                let n = plate.bevel(x, y, 3.0);
                (crate::photo::chrome(n, metal), cov(d, scale))
            });
            cx.image(bx0, by0, w, h, img);
            let mut font = ty.title;
            font.family = stave_ui::scene::Family::Serif;
            font.weight = 800;
            font.tracking = 0.04;
            cx.scene.text_in(
                stave_ui::scene::pt(g.cx, g.cy + size * 0.34),
                text,
                size,
                stave_ui::style::Color::rgba(0.08, 0.07, 0.07, 0.92),
                font,
            );
        }
    }
}

/// The outward direction of a rounded rectangle's edge near (x, y).
fn grad(r: &RRect, x: f64, y: f64) -> (f64, f64) {
    crate::photo::grad_rrect(x, y, r.cx, r.cy, r.hw, r.hh, r.r)
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
        let (base, finish, scale) = (th.finish.plate, th.finish.coat, cx.scale);
        let shadows = cx.shadows;
        let img = material::bake_rgba(r.w, r.h, scale, |lx, ly| {
            let (x, y) = (lx + r.x, ly + r.y);
            let d = plate.sd(x, y);
            if d > 1.0 {
                return ([0.0; 3], 0.0);
            }
            // The floor, shaded by the lip along its top-left edge.
            let floor = material::surface(finish, base, x, y, crate::photo::v3(0.0, 0.0, 1.0));
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
            let ty = &th.typography;
            let font = ty.caption.align(stave_ui::scene::TextAlign::Left);
            cx.scene
                .text_in(stave_ui::scene::pt(r.x + 16.0, r.y + 19.0), t, ty.caption_size, th.palette.ink_soft, font);
        }
    }
}
