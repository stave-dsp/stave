//! The stories: one showcase face that uses every part, rendered on each
//! theme.

use stave_face::prelude::*;
use stave_face::Frame;
use stave_ui::layout::Rect;
use stave_ui::scene::{pt, Path, Scene, Stroke};
use stave_ui::style::Color;

use crate::controller::{taps, StoryParams};

/// A scope trace on the display: a few cycles of a wave whose shape
/// follows SHAPE and whose depth follows DEPTH, drifting with time.
fn scope(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    let th = f.theme;
    let ph = th.palette.phosphor;
    let grid = Color::rgba(ph.r, ph.g, ph.b, 0.09);
    for i in 1..8 {
        let x = r.x + r.w * f64::from(i) / 8.0;
        s.stroke(Path::new().move_to(pt(x, r.y + 4.0)).line_to(pt(x, r.y + r.h - 4.0)), Stroke::new(grid, 1.0));
    }
    for i in 1..4 {
        let y = r.y + r.h * f64::from(i) / 4.0;
        s.stroke(Path::new().move_to(pt(r.x + 4.0, y)).line_to(pt(r.x + r.w - 4.0, y)), Stroke::new(grid, 1.0));
    }
    let (shape, depth) = (f.bound(0), f.bound(1));
    let t = f.tap(taps::PHASE);
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
pub fn showcase(p: &StoryParams, theme: Theme) -> Face {
    let gap = theme.space(3);
    let header = row(vec![
        Title::new("STAVE").tagline(theme.name).node(),
        spacer(),
        Lamp::param(p.hold.bind()).label("HOLD").node(),
    ])
    .gap(gap)
    .align(Align::Start);
    let tone = Plate::titled("TONE").around(
        row(vec![Knob::new(p.drive.bind()).large().node(), Knob::new(p.tone.bind()).node(), Knob::new(p.mix.bind()).node()]).gap(gap),
    );
    let screen = Display::new(300.0, 120.0, scope).reads(&[p.shape.bind(), p.depth.bind()]).taps(&[taps::PHASE]).animated().node();
    let motion = Plate::titled("MOTION").around(
        row(vec![
            Knob::new(p.rate.bind()).small().node(),
            Knob::new(p.depth.bind()).small().node(),
            Toggle::new(p.shape.bind()).node(),
            Toggle::new(p.sync.bind()).node(),
        ])
        .gap(gap)
        .align(Align::Center),
    );
    let output = Plate::titled("OUTPUT")
        .around(row(vec![Knob::new(p.width.bind()).small().centered().node(), Knob::new(p.level.bind()).small().centered().node()]).gap(gap));
    let feet = row(vec![Footswitch::bypass(p.bypass.bind()).node(), spacer(), Footswitch::new(p.hold.bind()).node()]);
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
pub fn amp(p: &StoryParams) -> Face {
    use stave_face::photo::hex;
    let base = Theme::graphite();
    let theme = Theme {
        name: "amp head",
        finish: stave_face::Finish {
            coat: stave_face::Coat::Anodized,
            surface: hex(0xB8923E),
            plate: hex(0xA8853A),
            ..base.finish
        },
        palette: stave_face::Palette {
            ink: Color::rgba(0.09, 0.07, 0.05, 1.0),
            ink_soft: Color::rgba(0.09, 0.07, 0.05, 0.7),
            accent: Color::rgba(0.55, 0.08, 0.05, 1.0),
            ..base.palette
        },
        hardware: stave_face::Hardware {
            lamp: [1.9, 0.18, 0.08],
            caps: stave_face::CapStyle::Chicken,
            cap: hex(0x141312),
            ..base.hardware
        },
        coverings: stave_face::Coverings {
            tolex: hex(0x121212),
            piping: hex(0xE6DCC4),
            grille: hex(0x3B342C),
            badge: [0.95, 0.93, 0.88],
        },
        ..base
    };
    let gap = theme.space(3);
    let controls = row(vec![
        Lamp::pilot().label("POWER").node(),
        Toggle::new(p.sync.bind()).label("CHANNEL").node(),
        Knob::new(p.drive.bind()).label("GAIN").node(),
        Knob::new(p.tone.bind()).label("BASS").centered().node(),
        Knob::new(p.mix.bind()).label("MIDDLE").centered().node(),
        Knob::new(p.time.bind()).label("TREBLE").centered().node(),
        Knob::new(p.width.bind()).label("PRESENCE").node(),
        Knob::new(p.level.bind()).label("MASTER").node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::amp_head().grille(Side::Top, 120.0, Some("Stave")).around(controls))
}

/// A 2U rack processor with handles.
pub fn rack(p: &StoryParams) -> Face {
    let theme = Theme { name: "rack", ..Theme::graphite() };
    let gap = theme.space(3);
    let body = row(vec![
        column(vec![Title::new("STAVE").tagline("rack processor").node(), Presets::default().node()]).gap(8.0),
        Meter::unit(taps::LEVEL).label("IN").height(110.0).node(),
        Knob::new(p.drive.bind()).large().node(),
        Knob::new(p.tone.bind()).centered().node(),
        Knob::new(p.mix.bind()).node(),
        Selector::new(p.shape.bind()).node(),
        Meter::unit(taps::LEVEL).label("OUT").height(110.0).node(),
        Footswitch::bypass(p.bypass.bind()).compact().node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::rack(2).handles().around(body))
}

/// A speaker cabinet: grille above, a control strip below.
pub fn cabinet(p: &StoryParams) -> Face {
    use stave_face::photo::hex;
    let base = Theme::graphite();
    let theme = Theme {
        name: "cabinet",
        finish: stave_face::Finish {
            coat: stave_face::Coat::Crinkle,
            surface: hex(0x1E1E1F),
            ..base.finish
        },
        coverings: stave_face::Coverings {
            tolex: hex(0x151515),
            grille: hex(0x2A2724),
            ..base.coverings
        },
        hardware: stave_face::Hardware {
            caps: stave_face::CapStyle::Chicken,
            cap: hex(0xE8E0CC),
            ..base.hardware
        },
        ..base
    };
    let gap = theme.space(4);
    let strip = row(vec![
        Selector::new(p.shape.bind()).label("SPEAKER").node(),
        Knob::new(p.tone.bind()).label("MIC").centered().node(),
        Knob::new(p.depth.bind()).label("DISTANCE").node(),
        Knob::new(p.width.bind()).label("ROOM").node(),
        Knob::new(p.level.bind()).label("LEVEL").centered().node(),
    ])
    .gap(gap)
    .align(Align::Center);
    Face::new(theme, Panel::cabinet().grille(Side::Top, 300.0, Some("Stave")).around(strip))
}

/// The controls the showcase leaves out: a pager over two pages, an x/y
/// pad, a stepper, a momentary footswitch, and knobs that follow a
/// toggle's step (`on_step`).
pub fn controls(p: &StoryParams) -> Face {
    let theme = Theme { name: "controls", ..Theme::graphite() };
    let gap = theme.space(3);
    let header = row(vec![Title::new("STAVE").tagline("every control").node(), spacer(), Pager::new(1, &["MAIN", "MOD"]).node()])
        .gap(gap)
        .align(Align::Start);
    let main = row(vec![Pad::new(p.rate.bind(), p.depth.bind(), 140.0, 120.0).node(), Stepper::new(p.shape.bind()).node()])
        .gap(gap)
        .align(Align::Center);
    let sync = p.sync.bind();
    let follow = stack(vec![on_step(Knob::new(p.drive.bind()).node(), sync, 0), on_step(Knob::new(p.mix.bind()).node(), sync, 1)]);
    let modulation = row(vec![Toggle::new(sync).node(), follow, Footswitch::new(p.hold.bind()).momentary().label("TAP").node()])
        .gap(gap)
        .align(Align::Center);
    let body = column(vec![header, pages(1, vec![main, modulation])]).gap(theme.space(4)).align(Align::Stretch);
    Face::new(theme, Panel::default().around(body))
}

/// Every story: the showcase on each theme, then the hardware forms.
pub fn all(p: &StoryParams) -> Vec<(String, Face)> {
    let mut v: Vec<(String, Face)> = Theme::all().into_iter().map(|t| (t.name.to_string(), showcase(p, t))).collect();
    v.push(("amp".into(), amp(p)));
    v.push(("rack".into(), rack(p)));
    v.push(("cabinet".into(), cabinet(p)));
    v.push(("controls".into(), controls(p)));
    v
}
