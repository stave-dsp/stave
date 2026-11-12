//! Every part in the catalogue as a picture of its own (1×, on the
//! graphite theme, the enclosure left out): what each part looks like,
//! at rest and with the readings that light it. A part whose picture
//! changes shows up here by name, not as a pixel somewhere in a story.
//! `STAVE_BLESS=1` rewrites them.

use std::sync::Arc;

use stave_core::params::Params;
use stave_face::prelude::*;
use stave_storybook::controller::{taps, StoryParams};
use stave_test::assert_golden;
use stave_test::face::FaceDriver;

/// The part `make` builds, alone on graphite, with taps set to `readings`.
fn looks_right(name: &str, readings: &[(TapId, f64)], make: impl Fn(&StoryParams) -> FaceNode) {
    let params = Arc::new(StoryParams::default());
    // Room around the part for what spills past it (a lamp's glow).
    let face = Face::new(Theme::graphite(), column(vec![make(&params)]).pad(16.0));
    let mut ui = FaceDriver::new(face, params as Arc<dyn Params>, Vec::new());
    // A trace draws its history: frames pass with each reading swinging
    // in to where it ends.
    const FRAMES: usize = 40;
    for i in 0..FRAMES {
        let swing = (0.25 * (FRAMES - 1 - i) as f64).cos();
        for &(tap, value) in readings {
            ui.tap(tap, value * swing);
        }
        if i + 1 < FRAMES {
            ui.render(0.25);
            ui.advance(1.0 / 60.0);
        }
    }
    assert_golden!(&format!("part-{name}"), &ui.render(1.0));
}

fn scope(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    // Drawn past the screen on purpose: the display clips it.
    let ph = f.theme.palette.phosphor;
    s.stroke(Path::new().move_to(pt(r.x - 20.0, r.y + r.h)).line_to(pt(r.x + r.w + 20.0, r.y)), Stroke::round(ph, 3.0));
}

macro_rules! part_goldens {
    ($($test:ident: $name:literal, $readings:expr, $make:expr;)*) => {$(
        #[test]
        #[cfg_attr(miri, ignore)]
        fn $test() {
            looks_right($name, &$readings, $make);
        }
    )*};
}

part_goldens! {
    knob: "knob", [], |p| Knob::new(p.drive.bind()).node();
    knob_centered: "knob-centered", [], |p| Knob::new(p.tone.bind()).large().centered().label("TONE").node();
    toggle: "toggle", [], |p| Toggle::new(p.shape.bind()).node();
    footswitch: "footswitch", [], |p| Footswitch::bypass(p.bypass.bind()).node();
    lamp: "lamp", [], |_| Lamp::pilot().label("POWER").node();
    lamp_bipolar: "lamp-bipolar", [(taps::LEVEL, -0.6), (taps::PHASE, 0.8)], |_| row(vec![
        Lamp::bipolar(taps::LEVEL).label("NEG").node(),
        Lamp::bipolar(taps::PHASE).label("POS").node(),
    ]);
    meter: "meter", [(taps::LEVEL, 0.7)], |_| Meter::unit(taps::LEVEL).label("IN").height(90.0).node();
    meter_reduction: "meter-reduction", [(taps::LEVEL, -9.0)], |_| Meter::reduction(taps::LEVEL).label("GR").height(90.0).node();
    trace_bipolar: "trace-bipolar", [(taps::LEVEL, -0.5)], |_| Trace::new(taps::LEVEL, 80.0, 44.0, (-1.0, 1.0)).bipolar().label("1").node();
    pad: "pad", [], |p| Pad::new(p.rate.bind(), p.depth.bind(), 90.0, 70.0).node();
    display_clipped: "display-clipped", [], |_| Display::new(120.0, 50.0, scope).node();
    selector: "selector", [], |p| Selector::new(p.shape.bind()).node();
    stepper: "stepper", [], |p| Stepper::new(p.shape.bind()).node();
    presets: "presets", [], |_| Presets::default().node();
    pager: "pager", [], |_| Pager::new(1, &["MAIN", "MODULATION"]).node();
    tabs: "tabs", [], |p| Tabs::of(&p.shape).node();
    title: "title", [], |_| Title::new("STAVE").tagline("a measured title").node();
    caption: "caption", [], |_| Caption::new("SMALL PRINT").node();
    plate: "plate", [], |p| Plate::titled("A PLATE WIDER THAN ITS KNOB").around(Knob::new(p.mix.bind()).small().node());
}
