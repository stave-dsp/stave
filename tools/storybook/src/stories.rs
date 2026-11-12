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

/// A plexi-style amp head: black tolex, a brushed-gold faceplate in
/// cream piping, chicken-head knobs, a jewel pilot, a grille with a badge.
pub fn amp(host: &StoryHost) -> Face {
    use stave_face::photo::hex;
    let b = |id| host.bind(id);
    let theme = Theme {
        name: "amp head",
        surface: hex(0xB8923E),
        finish: stave_face::Finish::Anodized,
        plate: hex(0xA8853A),
        ink: Color::rgba(0.09, 0.07, 0.05, 1.0),
        ink_soft: Color::rgba(0.09, 0.07, 0.05, 0.7),
        accent: Color::rgba(0.55, 0.08, 0.05, 1.0),
        lamp: [1.9, 0.18, 0.08],
        caps: stave_face::CapStyle::Chicken,
        cap: hex(0x141312),
        tolex: hex(0x121212),
        piping: hex(0xE6DCC4),
        grille: hex(0x3B342C),
        badge: [0.95, 0.93, 0.88],
        ..Theme::graphite()
    };
    let gap = theme.space(3);
    let controls = row(vec![
        Lamp::pilot().label("POWER").node(),
        Toggle::new(b(SYNC)).label("CHANNEL").node(),
        Knob::new(b(DRIVE)).label("GAIN").node(),
        Knob::new(b(TONE)).label("BASS").centered().node(),
        Knob::new(b(MIX)).label("MIDDLE").centered().node(),
        Knob::new(b(TIME)).label("TREBLE").centered().node(),
        Knob::new(b(WIDTH)).label("PRESENCE").node(),
        Knob::new(b(LEVEL)).label("MASTER").node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::amp_head().grille(Side::Top, 120.0, Some("Stave")).around(controls))
}

/// A 2U rack processor with handles.
pub fn rack(host: &StoryHost) -> Face {
    let b = |id| host.bind(id);
    let theme = Theme { name: "rack", ..Theme::graphite() };
    let gap = theme.space(3);
    let body = row(vec![
        column(vec![Title::new("STAVE").tagline("rack processor").node(), Presets::default().node()]).gap(8.0),
        Meter::unit("level").label("IN").height(110.0).node(),
        Knob::new(b(DRIVE)).large().node(),
        Knob::new(b(TONE)).centered().node(),
        Knob::new(b(MIX)).node(),
        Selector::new(b(SHAPE)).node(),
        Meter::unit("level").label("OUT").height(110.0).node(),
        Footswitch::bypass(b(BYPASS)).compact().node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::rack(2).handles().around(body))
}

/// A speaker cabinet: grille above, a control strip below.
pub fn cabinet(host: &StoryHost) -> Face {
    use stave_face::photo::hex;
    let b = |id| host.bind(id);
    let theme = Theme {
        name: "cabinet",
        surface: hex(0x1E1E1F),
        finish: stave_face::Finish::Crinkle,
        tolex: hex(0x151515),
        grille: hex(0x2A2724),
        caps: stave_face::CapStyle::Chicken,
        cap: hex(0xE8E0CC),
        ..Theme::graphite()
    };
    let gap = theme.space(4);
    let strip = row(vec![
        Selector::new(b(SHAPE)).label("SPEAKER").node(),
        Knob::new(b(TONE)).label("MIC").centered().node(),
        Knob::new(b(DEPTH)).label("DISTANCE").node(),
        Knob::new(b(WIDTH)).label("ROOM").node(),
        Knob::new(b(LEVEL)).label("LEVEL").centered().node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::cabinet().grille(Side::Top, 300.0, Some("Stave")).around(strip))
}

/// Every story: the showcase on each theme, then the hardware forms.
pub fn all(host: &StoryHost) -> Vec<(String, Face)> {
    let mut v: Vec<(String, Face)> = Theme::all().into_iter().map(|t| (t.name.to_string(), showcase(host, t))).collect();
    v.push(("amp".into(), amp(host)));
    v.push(("rack".into(), rack(host)));
    v.push(("cabinet".into(), cabinet(host)));
    v
}
