# Faces, parts and themes

A plugin's editor is a *face*: a tree of parts — knobs, switches, lamps,
meters, displays, print — laid out in rows and columns inside an
enclosure, and drawn from a theme that holds the whole look as data. A
face is declared, never programmed: [`Plugin::face`](stave::Plugin::face)
is a pure function of the params, and the framework's runtime does the
layout, the drawing and every gesture, the same way on every face and
every platform.

## A face is a function of the params

[`Plugin::face`](stave::Plugin::face) receives the params and returns
`Option<`[`Face`](stave_face::Face)`>` — a [`Theme`](stave_face::Theme)
and a root node. Parts bind to params with `p.x.bind()` (a
[`Bind`](stave_face::Bind): the param's [`ParamId`](stave::ParamId) and
name), so the face and the parameter table cannot disagree about which
knob is which. Returning `None` (the default) means the plugin has no
editor and the host shows its own generic one.

```
use stave::prelude::*;

#[derive(Params)]
pub struct EchoParams {
    pub time: FloatParam,
    pub mix: FloatParam,
    pub bypass: BoolParam,
}

impl Default for EchoParams {
    fn default() -> Self {
        EchoParams {
            time: FloatParam::new(ParamId(1), "Time", Range::log(10.0, 2000.0))
                .default(250.0)
                .unit(Unit::Ms),
            mix: FloatParam::new(ParamId(2), "Mix", Range::linear(0.0, 1.0))
                .default(0.5)
                .unit(Unit::Percent),
            bypass: BoolParam::bypass(ParamId(3), "Bypass"),
        }
    }
}

/// The face: a pure function of the params.
fn face(p: &EchoParams) -> Face {
    let theme = Theme::graphite();
    let header = row(vec![
        Title::new("ECHO").tagline("a face, declared").node(),
        Footswitch::bypass(p.bypass.bind()).compact().node(),
    ])
    .gap(theme.space(4));
    let knobs = Plate::titled("DELAY").around(
        row(vec![Knob::new(p.time.bind()).large().node(), Knob::new(p.mix.bind()).node()])
            .gap(theme.space(4)),
    );
    let body = column(vec![header, knobs]).gap(theme.space(3));
    Face::new(theme, Panel::default().around(body))
}

// Drive it as a person would: by part name, not coordinates.
let params = Arc::new(EchoParams::default());
let mut ui = stave_test::FaceDriver::new(face(&params), params, Vec::new());
assert_eq!(ui.part("mix"), PartId::Param(ParamId(2)));     // a bound part is named by its param
ui.click("bypass");
assert_eq!(ui.value("bypass"), 1.0);                       // the footswitch latches
ui.double_click("time");
assert_eq!(ui.value("time"), 250.0);                       // double-click resets to the default
ui.assert_bracketed();                                     // every edit was begin → perform → end
```

The same `face` function goes on the [`Plugin`](stave::Plugin) as
`fn face(p: &EchoParams) -> Option<Face> { Some(face(p)) }`; Tilt's
(`examples/tilt/src/lib.rs`) is the worked example.

## Parts

A [`Part`](stave_face::Part) is one leaf of the tree. Its contract is a
handful of methods, all with defaults:

| method | what it says |
|---|---|
| [`size`](stave_face::Part::size) | its natural size, from the theme's spacing and type scale and its text as measured by the platform's fonts |
| [`bake`](stave_face::Part::bake) | the static picture (legends, scales, the knob's body), drawn once per pixel scale and cached |
| [`paint`](stave_face::Part::paint) | the live picture (a pointer, a needle, a lamp's glow), drawn from a [`Frame`](stave_face::Frame) |
| [`reads`](stave_face::Part::reads) | the params and taps `paint` reads, declared once: the runtime repaints when any of them moves |
| [`grabs`](stave_face::Part::grabs) | where the part can be grabbed, as [`Grab`](stave_face::Grab)s: an area and a [`Gesture`](stave_face::Gesture) |
| [`id`](stave_face::Part::id) | its name when its node has none: by default its first read param |

Parts never hold state and never talk to the plugin directly. Every
value they show comes through the [`Frame`](stave_face::Frame) (params by
[`Bind`](stave_face::Bind), taps by [`TapId`](stave::TapId)), and every
edit they make is a [`Gesture`](stave_face::Gesture) that the runtime
carries out — continuous drags with Shift for fine control, double-click
and Option-click to reset, wheel and arrow-key nudges, stepping,
latching and momentary switches, two-param pads, preset and page
buttons. Because the runtime owns the gestures, every knob on every face
behaves the same, and the interaction laws are checked once for all of
them (see [testing](crate::concepts::testing)).

Every part has a stable name, a [`PartId`](stave_face::PartId): its bound
parameter (`PartId::Param`), or a name — one given with `node.id("…")`,
or a role the part picks for itself (a [`Title`](stave_face::Title) is
`"title"`). Tests and tools address parts by these names, never by
coordinates.

### The catalogue

The parts every plugin can use live in [`stave_face::parts`]:
[`Knob`](stave_face::Knob), [`Toggle`](stave_face::Toggle),
[`Footswitch`](stave_face::Footswitch), [`Lamp`](stave_face::Lamp),
[`Selector`](stave_face::Selector), [`Stepper`](stave_face::Stepper),
[`Tabs`](stave_face::Tabs), [`Presets`](stave_face::Presets),
[`Pager`](stave_face::Pager), [`Meter`](stave_face::Meter),
[`Trace`](stave_face::parts::Trace), [`Pad`](stave_face::Pad),
[`Display`](stave_face::Display), [`Title`](stave_face::Title) and
[`Caption`](stave_face::Caption), with [`Plate`](stave_face::Plate) and
[`Panel`](stave_face::Panel) around them. The catalogue page of this
site shows a picture of each, drawn from the real part; `tools/storybook`
shows every part live on every theme
(`cargo run -p stave-storybook`).

A picture the catalogue lacks starts in your own project as your own
part: implement [`Part`](stave_face::Part), declare what it reads, paint
a [`Scene`](stave_face::prelude::Scene) from its frame. A part graduates
into the catalogue when a second plugin needs it. For a one-off picture
inside a screen, a [`Display`](stave_face::Display) takes a draw function
and clips it to its glass.

```
use stave::prelude::*;
use stave::face::{measured, Frame, Part, Reads};

/// A bar that fills with a param: the smallest useful part.
struct Bar(Bind);

impl Part for Bar {
    fn size(&self, theme: &Theme, _fonts: &dyn stave_ui::Fonts) -> (f64, f64) {
        (theme.space(5) * 3.0, theme.space(1))
    }
    fn reads(&self) -> Reads {
        Reads::params(&[self.0])
    }
    fn paint(&self, r: Rect, f: &Frame<'_>, s: &mut Scene) {
        let w = r.w * f.bound(0);           // the first declared read, normalized
        let y = r.y + r.h / 2.0;
        s.stroke(Path::new().move_to(pt(r.x, y)).line_to(pt(r.x + w, y)), Stroke::round(f.theme.palette.accent, r.h));
    }
}

# #[derive(Params)]
# pub struct P { pub mix: FloatParam }
# let params = Arc::new(P { mix: FloatParam::new(ParamId(1), "Mix", Range::linear(0.0, 1.0)).default(0.25) });
let face = Face::new(Theme::cream(), column(vec![measured(Bar(params.mix.bind()))]));
let ui = stave_test::FaceDriver::new(face, params, Vec::new());
assert_eq!(ui.part("mix"), PartId::Param(ParamId(1)));   // named by its first read param
assert_eq!(ui.size(), (96.0, 8.0));                      // sized from cream's spacing scale: 3 × 32, 8
```

## Layout

A face is a layout tree: [`row`](stave_face::prelude::row),
[`column`](stave_face::prelude::column) and
[`stack`](stave_face::prelude::stack) (children overlaid) with
[`spacer`](stave_face::prelude::spacer)s, gaps, alignment and
`Dim::Fill` shares, solved by a small flexbox solver in `stave-ui`. Leaves
are parts: [`measured`](stave_face::measured) parts size themselves from
the theme and their measured print (every catalogue part's `node()` is
measured); [`leaf`](stave_face::leaf) fixes a size. A
[`Plate`](stave_face::Plate) is a recessed, optionally titled group,
never narrower than its title; a [`Panel`](stave_face::Panel) is the
enclosure around the whole face, in one of four
[`Form`](stave_face::Form)s — a pedal (the default), a rack unit, an amp
head, a speaker cabinet. Plate and panel margins are fixed constants, set
when the node is built; everything inside sizes from the theme.

Space comes from the theme's scale, never from literals:
`theme.space(4)` is the fifth step of the 4-point grid (24 points on
every built-in theme). Size names for
knobs (`.small()`, `.large()`, `.huge()`) are the other dial.

## Pages and steps

Content can follow the editor or a parameter:

- [`pages`](stave_face::pages)`(key, nodes)` overlays pages in one rect,
  each stretched to the largest so flipping never moves the rest of the
  face; a [`Pager`](stave_face::Pager) (or [`Tabs`](stave_face::Tabs))
  flips them. Pages are editor-local: they are not parameters and the
  host never sees them.
- [`on_step`](stave_face::on_step)`(node, p.mode.bind(), step)` shows a
  node only while a stepped parameter sits on `step` — an algorithm's
  own controls under its selector. Steps are real parameter values, so
  automation flips them too.

```
use stave::prelude::*;

param_enum! {
    pub const MODES: Mode = ["CLEAN", "DIRTY"];
}

#[derive(Params)]
pub struct P { pub mode: EnumParam<Mode>, pub drive: FloatParam }

let params = Arc::new(P {
    mode: EnumParam::new(ParamId(1), "Mode", Mode(0)),
    drive: FloatParam::new(ParamId(2), "Drive", Range::linear(0.0, 1.0)),
});
let face = Face::new(
    Theme::graphite(),
    column(vec![
        Selector::new(params.mode.bind()).node(),
        on_step(Knob::new(params.drive.bind()).node(), params.mode.bind(), 1),
    ]),
);
let mut ui = stave_test::FaceDriver::new(face, params, Vec::new());
assert!(ui.grabs("drive").is_empty());   // CLEAN: the drive knob is hidden
ui.set("mode", 1.0);                     // DIRTY, as automation would set it
assert!(!ui.grabs("drive").is_empty());  // and now it can be grabbed
```

## Themes

A [`Theme`](stave_face::Theme) is the whole look as plain data, grouped
by what it dresses:

| field | type | holds |
|---|---|---|
| `finish` | [`Finish`](stave_face::Finish) | the enclosure's [`Coat`](stave_face::Coat) (anodized, powder, hammertone, crinkle, enamel), surface, plate and desk colours, corner radius |
| `palette` | [`Palette`](stave_face::Palette) | print (`ink`, `ink_soft`), the `accent` that means "selected", display glass (`screen`, `phosphor`) |
| `hardware` | [`Hardware`](stave_face::Hardware) | the knob family ([`CapStyle`](stave_face::CapStyle)), cap and pointer colours, the LED colour |
| `typography` | [`Typography`](stave_face::Typography) | a font and size per text role: title, tagline, legend, caption, value |
| `spacing` | `[f64; 6]` | the spacing scale (4-point grid), read through `theme.space(step)` |
| `coverings` | [`Coverings`](stave_face::Coverings) | tolex, piping, grille cloth and badge metal for amp and cabinet forms |

Parts never mint a colour, a font or a coordinate: they ask the theme for
a role. Swapping the theme re-skins a face without touching its layout or
its parts. [`Theme::all`](stave_face::Theme::all) lists the built-in
themes, which the storybook pages through:

```
use stave::face::Theme;

let names: Vec<&str> = Theme::all().iter().map(|t| t.name).collect();
assert_eq!(names, ["graphite", "cream", "hammertone", "oxblood", "verdigris", "indigo"]);
assert_eq!(Theme::default(), Theme::graphite());   // the house default
```

The site's theme page shows each one's swatches.

## The runtime and the platforms

One [`Runtime`](stave_face::Runtime) runs each open editor. It lays out
the tree (measuring each part once against the platform's fonts), bakes
the static layer once per pixel scale, paints the live layer over a copy
of it only when something a part [`reads`](stave_face::Part::reads)
moves (or a part animates), and turns pointer, wheel and key input into
parameter edits. It is portable and headless-testable: it reads and
writes the plugin only through the [`Controller`](stave::Controller),
and draws a [`Scene`](stave_face::prelude::Scene) — the vector vocabulary
of `stave-ui` — into pixels with `stave-raster`, the portable software
rasteriser that gives the same pixels on every platform.

A platform view is only a window, a pointer, a blit and text. Each
platform crate exposes a `FaceView` of the same shape, and the format
wrappers (VST3 and CLAP) embed it in the window the host gives them:

| platform | crate | text | notes |
|---|---|---|---|
| macOS | `stave-ui-cocoa` | CoreText | an `NSView`; the storybook and both format wrappers use it |
| Windows | `stave-ui-win32` | `stave-font` | an `HWND` child window at the parent's DPI, with its own repaint timer |
| Linux and the BSDs | `stave-ui-x11` | `stave-font` | an X11 child window, pumped by the host's run loop (it has no loop of its own) |

`stave-ui-shell` is what the Windows and Linux views share: a face
session behind a reentrancy guard and a panic boundary. `stave-font`
embeds IBM Plex Sans and Mono (SIL Open Font License) and fills their
outlines with `stave-raster`, so a face looks the same on every machine
without a system text stack. `stave-ui-cocoa` is empty off macOS and
`stave-ui-win32` off Windows; `stave-ui-x11` builds on every unix,
macOS included (for checking), and is empty elsewhere.

Every view honours two environment variables: `STAVE_FACE_TRACE=FILE`
records every pointer and key event as a [`Trace`](stave_face::Trace)
(a session that replays as a test), and `STAVE_FACE_SNAPSHOT=FILE.png`
makes the face write its own picture once it has run for a second.
