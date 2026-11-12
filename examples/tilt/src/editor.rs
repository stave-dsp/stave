//! The tilt face — the whole window, a pure function of params: a title
//! and a bypass switch over one plate holding the two knobs. Layout is
//! rows and columns; the look is the theme's.

use stave_core::face::prelude::*;
use stave_core::params::BindParam;

use crate::params::TiltParams;

pub fn face(p: &TiltParams) -> Face {
    let theme = Theme::graphite();
    let gap = theme.space(4);
    let header = row(vec![
        Title::new("TILT").tagline("tilt eq · ±6 dB around 650 Hz").node(),
        Footswitch::bypass(p.bypass.bind()).compact().node(),
    ])
    .gap(gap)
    .align(Align::Start);
    let tone = Plate::titled("TONE").around(
        row(vec![
            Knob::new(p.tilt.bind()).huge().centered().node(),
            Knob::new(p.output.bind()).centered().node(),
        ])
        .gap(gap)
        .align(Align::Center),
    );
    let body = column(vec![header, tone]).gap(theme.space(3)).align(Align::Stretch);
    Face::new(theme, Panel::default().around(body))
}
