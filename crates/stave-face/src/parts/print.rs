//! Print on the face: the product title and small captions.

use stave_ui::layout::Rect;
use stave_ui::scene::{pt, TextAlign};
use stave_ui::{Dim, Fonts};

use crate::part::{measured, Bake, FaceNode, Part, PartId};
use crate::theme::Theme;

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
    /// A title takes its share of a row, and never less than its print
    /// measures.
    pub fn node(self) -> FaceNode {
        measured(self).width(Dim::Fill(1.0))
    }
    /// The baselines of the name and the tagline, below the top.
    fn baselines(&self, th: &Theme) -> (f64, f64) {
        let ty = &th.typography;
        let name = ty.title_size * 0.95;
        (name, name + th.space(1) + ty.tagline_size * 1.2)
    }
}

impl Part for Title {
    fn id(&self) -> Option<PartId> {
        Some(PartId::Name("title"))
    }

    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        let name = fonts.width(self.name, ty.title_size, &ty.title);
        let (top, under) = self.baselines(th);
        match self.tagline {
            Some(t) => (name.max(fonts.width(t, ty.tagline_size, &ty.tagline)), under + ty.tagline_size * 0.25),
            None => (name, top + ty.title_size * 0.32),
        }
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let x = match self.align {
            TextAlign::Left => r.x,
            TextAlign::Center => r.x + r.w / 2.0,
            TextAlign::Right => r.x + r.w,
        };
        let (top, under) = self.baselines(th);
        cx.scene.text_in(pt(x, r.y + top), self.name, ty.title_size, th.palette.ink, ty.title.align(self.align));
        if let Some(t) = self.tagline {
            cx.scene.text_in(
                pt(x, r.y + under),
                t,
                ty.tagline_size,
                th.palette.ink_soft,
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
        measured(self).width(Dim::Fill(1.0))
    }
}

impl Part for Caption {
    fn size(&self, th: &Theme, fonts: &dyn Fonts) -> (f64, f64) {
        let ty = &th.typography;
        (fonts.width(self.text, ty.caption_size, &ty.caption), th.line(ty.caption_size))
    }

    fn bake(&self, r: Rect, cx: &mut Bake<'_>) {
        let th = cx.theme;
        let ty = &th.typography;
        let x = match self.align {
            TextAlign::Left => r.x,
            TextAlign::Center => r.x + r.w / 2.0,
            TextAlign::Right => r.x + r.w,
        };
        cx.scene
            .text_in(pt(x, r.y + ty.caption_size * 1.33), self.text, ty.caption_size, th.palette.ink_soft, ty.caption.align(self.align));
    }
}
