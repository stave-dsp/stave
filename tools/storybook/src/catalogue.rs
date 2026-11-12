//! The catalogue, as a list: every part of `stave_face::parts` built on
//! its own (1×, on the graphite theme, the enclosure left out unless the
//! part is the enclosure), at rest and with the readings that light it.
//!
//! One list, two readers: `tests/parts.rs` holds each picture to a
//! golden (text as boxes, the same on every machine), and the
//! documentation site (`tools/docsite`) shows the same pictures with
//! real type (the embedded IBM Plex of `stave-font`), drawn by
//! [`picture`] at build time. The site refuses to build while a type
//! implementing `Part` in stave-face has no entry.

use std::sync::Arc;

use stave_core::controller::{GestureSink, PluginController};
use stave_core::params::{ParamId, Params};
use stave_core::{Tap, Telemetry};
use stave_face::prelude::*;
use stave_face::Runtime;
use stave_font::FontText;
use stave_raster::{BoxText, Canvas};

use crate::controller::{taps, StoryParams};

/// One picture of one part.
pub struct Entry {
    /// The picture's name (its golden is `part-<name>.png`).
    pub name: &'static str,
    /// The public part type this picture shows (`Knob`, `Plate`, …): a
    /// type implementing `Part`, or the builder of one (`Plate`,
    /// `Panel`).
    pub part: &'static str,
    /// What this picture shows of it.
    pub shows: &'static str,
    /// The tap readings published before the picture is taken.
    pub readings: &'static [(TapId, f64)],
    /// The part, built against the story's params.
    pub make: fn(&StoryParams) -> FaceNode,
}

/// The display in [`ENTRIES`]: a stroke drawn past the screen on purpose,
/// which the display clips.
fn scope(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    let ph = f.theme.palette.phosphor;
    s.stroke(Path::new().move_to(pt(r.x - 20.0, r.y + r.h)).line_to(pt(r.x + r.w + 20.0, r.y)), Stroke::round(ph, 3.0));
}

/// Every picture in the catalogue, in the order the site shows them.
pub const ENTRIES: &[Entry] = &[
    Entry { name: "knob", part: "Knob", shows: "a knob bound to a percent param", readings: &[], make: |p| Knob::new(p.drive.bind()).node() },
    Entry {
        name: "knob-centered",
        part: "Knob",
        shows: "large, centred on a bipolar dB range, labelled",
        readings: &[],
        make: |p| Knob::new(p.tone.bind()).large().centered().label("TONE").node(),
    },
    Entry { name: "toggle", part: "Toggle", shows: "a toggle over an enum param", readings: &[], make: |p| Toggle::new(p.shape.bind()).node() },
    Entry {
        name: "footswitch",
        part: "Footswitch",
        shows: "the bypass footswitch",
        readings: &[],
        make: |p| Footswitch::bypass(p.bypass.bind()).node(),
    },
    Entry { name: "lamp", part: "Lamp", shows: "the pilot lamp", readings: &[], make: |_| Lamp::pilot().label("POWER").node() },
    Entry {
        name: "lamp-bipolar",
        part: "Lamp",
        shows: "bipolar lamps reading a negative and a positive tap",
        readings: &[(taps::LEVEL, -0.6), (taps::PHASE, 0.8)],
        make: |_| row(vec![Lamp::bipolar(taps::LEVEL).label("NEG").node(), Lamp::bipolar(taps::PHASE).label("POS").node()]),
    },
    Entry {
        name: "meter",
        part: "Meter",
        shows: "a unit meter reading 0.7",
        readings: &[(taps::LEVEL, 0.7)],
        make: |_| Meter::unit(taps::LEVEL).label("IN").height(90.0).node(),
    },
    Entry {
        name: "meter-reduction",
        part: "Meter",
        shows: "a gain-reduction meter hanging from the top, reading −9 dB",
        readings: &[(taps::LEVEL, -9.0)],
        make: |_| Meter::reduction(taps::LEVEL).label("GR").height(90.0).node(),
    },
    Entry {
        name: "trace-bipolar",
        part: "Trace",
        shows: "a bipolar trace of a tap's history",
        readings: &[(taps::LEVEL, -0.5)],
        make: |_| Trace::new(taps::LEVEL, 80.0, 44.0, (-1.0, 1.0)).bipolar().label("1").node(),
    },
    Entry { name: "pad", part: "Pad", shows: "an x/y pad over two params", readings: &[], make: |p| Pad::new(p.rate.bind(), p.depth.bind(), 90.0, 70.0).node() },
    Entry {
        name: "display-clipped",
        part: "Display",
        shows: "a display whose drawing is clipped to its screen",
        readings: &[],
        make: |_| Display::new(120.0, 50.0, scope).node(),
    },
    Entry { name: "selector", part: "Selector", shows: "a selector over an enum param", readings: &[], make: |p| Selector::new(p.shape.bind()).node() },
    Entry { name: "stepper", part: "Stepper", shows: "a stepper over an enum param", readings: &[], make: |p| Stepper::new(p.shape.bind()).node() },
    Entry { name: "tabs", part: "Tabs", shows: "tabs over an enum param", readings: &[], make: |p| Tabs::of(&p.shape).node() },
    Entry { name: "presets", part: "Presets", shows: "the preset bar", readings: &[], make: |_| Presets::default().node() },
    Entry { name: "pager", part: "Pager", shows: "a pager over two pages", readings: &[], make: |_| Pager::new(1, &["MAIN", "MODULATION"]).node() },
    Entry { name: "title", part: "Title", shows: "a title with its tagline", readings: &[], make: |_| Title::new("STAVE").tagline("a measured title").node() },
    Entry { name: "caption", part: "Caption", shows: "a caption", readings: &[], make: |_| Caption::new("SMALL PRINT").node() },
    Entry {
        name: "plate",
        part: "Plate",
        shows: "a titled plate around a small knob, never narrower than its title",
        readings: &[],
        make: |p| Plate::titled("A PLATE WIDER THAN ITS KNOB").around(Knob::new(p.mix.bind()).small().node()),
    },
    Entry {
        name: "panel",
        part: "Panel",
        shows: "the pedal enclosure around a knob",
        readings: &[],
        make: |p| Panel::pedal().around(Knob::new(p.drive.bind()).node()),
    },
];

/// How a picture sets its text.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Text {
    /// Boxes with the metrics of [`BoxText`]: the same on every machine
    /// (what the goldens compare, as `FaceDriver` draws).
    Boxes,
    /// The embedded fonts ([`FontText`]), as Windows and Linux show a face.
    Type,
}

/// Edits go nowhere: a picture is never touched.
struct Still;

impl GestureSink for Still {
    fn begin(&self, _: ParamId) {}
    fn perform(&self, _: ParamId, _: f64) {}
    fn end(&self, _: ParamId) {}
}

/// `entry`'s picture at `scale`: the part alone on graphite, with room
/// around it for what spills past it (a lamp's glow).
pub fn picture(entry: &Entry, scale: f64, text: Text) -> Canvas {
    let params = Arc::new(StoryParams::default());
    let face = Face::new(Theme::graphite(), column(vec![(entry.make)(&params)]).pad(16.0));
    draw(face, params, entry.readings, scale, text)
}

/// `face` over `params`, drawn at `scale` after frames in which each
/// reading swings in to where it ends (a trace draws its history).
pub fn draw(face: Face, params: Arc<StoryParams>, readings: &[(TapId, f64)], scale: f64, text: Text) -> Canvas {
    let telemetry = Telemetry::new();
    let controller = PluginController::new(params as Arc<dyn Params>, telemetry.clone(), Vec::new().into(), Box::new(Still));
    let mut runtime = match text {
        Text::Boxes => Runtime::new(face, &BoxText),
        Text::Type => Runtime::new(face, &FontText),
    };
    let mut draw = |scale: f64, time: f64| {
        let mut canvas = Canvas::new(0, 0);
        match text {
            Text::Boxes => runtime.render(&controller, scale, time, &mut canvas, &mut BoxText),
            Text::Type => runtime.render(&controller, scale, time, &mut canvas, &mut FontText),
        }
        canvas
    };
    const FRAMES: usize = 40;
    let mut time = 0.0;
    for i in 0..FRAMES {
        let swing = (0.25 * (FRAMES - 1 - i) as f64).cos();
        for &(tap, value) in readings {
            Tap::register(&telemetry, tap).set(value * swing);
        }
        if i + 1 < FRAMES {
            draw(0.25, time);
            time += 1.0 / 60.0;
        }
    }
    draw(scale, time)
}
