//! Print on the face: the product title and small captions.

extern crate alloc;

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, TextAlign};
use stave_ui::Dim;

use crate::part::{leaf, Bake, FaceNode, Part};

/// The product name, with an optional line beneath.
#[derive(Copy, Clone, Debug)]
pub struct Title {
    pub name: &'static str,
    pub tagline: Option<&'static str>,
    pub align: TextAlign,
}

impl Title {
    pub fn new(name: &'static str) -> Self {
        Title {
            name,
            tagline: None,
            align: TextAlign::Left,
        }
    }
    pub fn tagline(mut self, t: &'static str) -> Self {
        self.tagline = Some(t);
        self
    }
    pub fn centered(mut self) -> Self {
        self.align = TextAlign::Center;
        self
    }
    pub fn node(self) -> FaceNode {
        let h = if self.tagline.is_some() { 42.0 } else { 28.0 };
        // A strut as wide as the print (estimated at the house sizes),
        // so a row can never squeeze the title narrower than its text.
        let ty = &crate::theme::TYPE;
        let est = |s: &str, size: f64, track: f64| s.chars().count() as f64 * size * (0.64 + track);
        let w = est(self.name, ty.title_size, ty.title.tracking)
            .max(self.tagline.map_or(0.0, |t| est(t, ty.tagline_size, ty.tagline.tracking)));
        stave_ui::column(alloc::vec![leaf(self, 0.0, h).width(Dim::Fill(1.0)), stave_ui::boxed(alloc::vec![]).size(w, 0.0)])
            .width(Dim::Fill(1.0))
            .align(stave_ui::Align::Stretch)
    }
}

impl Part for Title {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let x = match self.align {
            TextAlign::Left => r.x,
            TextAlign::Center => r.x + r.w / 2.0,
            TextAlign::Right => r.x + r.w,
        };
        let base = r.y + ty.title_size * 0.95;
        cx.scene.text_in(pt(x, base), self.name, ty.title_size, th.ink, ty.title.align(self.align));
        if let Some(t) = self.tagline {
            cx.scene.text_in(
                pt(x, base + 8.0 + ty.tagline_size * 1.2),
                t,
                ty.tagline_size,
                th.ink_soft,
                ty.tagline.align(self.align),
            );
        }
    }
}

/// A line of small print.
#[derive(Copy, Clone, Debug)]
pub struct Caption {
    pub text: &'static str,
    pub align: TextAlign,
}

impl Caption {
    pub fn new(text: &'static str) -> Self {
        Caption {
            text,
            align: TextAlign::Center,
        }
    }
    pub fn node(self) -> FaceNode {
        leaf(self, 0.0, 14.0).width(Dim::Fill(1.0))
    }
}

impl Part for Caption {
    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.type_;
        let x = match self.align {
            TextAlign::Left => r.x,
            TextAlign::Center => r.x + r.w / 2.0,
            TextAlign::Right => r.x + r.w,
        };
        cx.scene
            .text_in(pt(x, r.y + 10.0), self.text, ty.caption_size, th.ink_soft, ty.caption.align(self.align));
    }
}
