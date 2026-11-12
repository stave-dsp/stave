//! The stories: one showcase face that uses every part, rendered on each
//! theme.

use stave_face::prelude::*;
use stave_face::Frame;
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use crate::host::{ids::*, StoryHost};

/// A scope trace on the display: a few cycles of a wave whose shape
/// follows SHAPE and whose depth follows DEPTH, drifting with time.
fn scope(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    let th = f.theme;
    let ph = th.phosphor;
    let grid = Color::rgba(ph.r, ph.g, ph.b, 0.09);
    for i in 1..8 {
        let x = r.x + r.w * f64::from(i) / 8.0;
        s.stroke(Path::new().move_to(pt(x, r.y + 4.0)).line_to(pt(x, r.y + r.h - 4.0)), Stroke::new(grid, 1.0));
    }
    for i in 1..4 {
        let y = r.y + r.h * f64::from(i) / 4.0;
        s.stroke(Path::new().move_to(pt(r.x + 4.0, y)).line_to(pt(r.x + r.w - 4.0, y)), Stroke::new(grid, 1.0));
    }
    let shape = f.host.normalized(SHAPE);
    let depth = f.host.normalized(DEPTH);
    let t = f.probe("phase");
    let n = 160;
    let pts: Vec<_> = (0..=n)
        .map(|i| {
            let u = f64::from(i) / f64::from(n);
            let a = (u * 3.0 + t * 0.4) * std::f64::consts::TAU;
            let sine = a.sin();
            let square = sine.signum() * (1.0 - (-6.0 * sine.abs()).exp());
            let v = if shape < 0.25 {
                sine
            } else if shape < 0.75 {
                square
            } else {
                (a * 0.5).sin() * 0.6 + (a * 1.7).sin() * 0.4
            };
            pt(r.x + 6.0 + u * (r.w - 12.0), r.y + r.h / 2.0 - v * depth * (r.h / 2.0 - 10.0))
        })
        .collect();
    let glow = Color::rgba(ph.r, ph.g, ph.b, 0.18);
    s.stroke(Path::polyline(&pts), Stroke::round(glow, 5.0));
    s.stroke(Path::polyline(&pts), Stroke::round(ph, 1.6));
}

/// The showcase face on `theme`.
pub fn showcase(host: &StoryHost, theme: Theme) -> Face {
    let b = |id| host.bind(id);
    let gap = theme.space(3);
    let header = row(vec![
        Title::new("STAVE").tagline(theme.name).node(),
        spacer(),
        Lamp::param(b(HOLD)).label("HOLD").node(),
    ])
    .gap(gap)
    .align(Align::Start);
    let tone = Plate::titled("TONE").around(
        row(vec![Knob::new(b(DRIVE)).large().node(), Knob::new(b(TONE)).node(), Knob::new(b(MIX)).node()]).gap(gap),
    );
    let screen = Display::new(300.0, 120.0, scope).reads(&[b(SHAPE), b(DEPTH)]).probes(&["phase"]).animated().node();
    let motion = Plate::titled("MOTION").around(
        row(vec![
            Knob::new(b(RATE)).small().node(),
            Knob::new(b(DEPTH)).small().node(),
            Toggle::new(b(SHAPE)).node(),
            Toggle::new(b(SYNC)).node(),
        ])
        .gap(gap)
        .align(Align::Center),
    );
    let output = Plate::titled("OUTPUT")
        .around(row(vec![Knob::new(b(WIDTH)).small().centered().node(), Knob::new(b(LEVEL)).small().centered().node()]).gap(gap));
    let feet = row(vec![Footswitch::bypass(b(BYPASS)).node(), spacer(), Footswitch::new(b(HOLD)).node()]);
    let body = column(vec![
        header,
        row(vec![screen, tone]).gap(gap).align(Align::Center),
        row(vec![motion, output]).gap(gap),
        feet,
    ])
    .gap(theme.space(4))
    .align(Align::Stretch);
    Face::new(theme, Panel::default().around(body))
}
