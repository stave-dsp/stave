//! stave-storybook — the design system's storybook: a showcase face
//! using every part, on every built-in theme, driven by a stand-in
//! plugin (an in-memory parameter table and taps that move with time).
//!
//! Usage:
//!   stave-storybook                                # live: drag everything; ←/→ change theme, Esc quits
//!   stave-storybook --screenshot DIR [--scale 2]   # one PNG per theme
//!
//! `STAVE_TEXT=embedded` sets text in the embedded fonts on macOS too
//! (how Windows and Linux show it).
//!
//! Screenshots are rendered offscreen through the real face runtime and
//! the platform's text, so they show exactly what a plugin window shows.

#![forbid(unsafe_code)]

mod live;

/// The platform's text: CoreText on macOS, the embedded fonts elsewhere
/// (and on macOS with `STAVE_TEXT=embedded`, to see a face as Windows and
/// Linux show it). Layout measures with the fonts that paint.
#[cfg(target_os = "macos")]
fn embedded() -> bool {
    std::env::var_os("STAVE_TEXT").is_some_and(|v| v == "embedded")
}

fn fonts() -> Box<dyn stave_ui::Fonts> {
    #[cfg(target_os = "macos")]
    if !embedded() {
        return Box::new(stave_ui_cocoa::text::SystemFonts);
    }
    Box::new(stave_font::FontText)
}

#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn text(scale: f64) -> Box<dyn stave_raster::TextPainter> {
    #[cfg(target_os = "macos")]
    if !embedded() {
        return Box::new(stave_ui_cocoa::offscreen::CoreText::new(scale));
    }
    Box::new(stave_font::FontText)
}

use std::path::PathBuf;

use stave_face::Runtime;

use stave_storybook::controller::StoryController;
use stave_storybook::stories;

fn screenshot(dir: PathBuf, scale: f64) {
    std::fs::create_dir_all(&dir).expect("create screenshot dir");
    let names: Vec<String> = stories::all(&StoryController::new().params).into_iter().map(|(n, _)| n).collect();
    for (i, name) in names.iter().enumerate() {
        let controller = StoryController::new();
        controller.freeze(1.7);
        let face = stories::all(&controller.params).swap_remove(i).1;
        let mut rt = Runtime::new(face, &*fonts());
        if std::env::var_os("STAVE_FACE_DEBUG").is_some() {
            for r in rt.rects() {
                println!("  {:7.1} {:7.1} {:7.1} {:7.1}", r.x, r.y, r.w, r.h);
            }
        }
        let mut canvas = stave_raster::Canvas::new(0, 0);
        let t0 = std::time::Instant::now();
        rt.render(&controller, scale, controller.time(), &mut canvas, &mut *text(scale));
        let first = t0.elapsed();
        let t1 = std::time::Instant::now();
        rt.render(&controller, scale, controller.time(), &mut canvas, &mut *text(scale));
        let frame = t1.elapsed();
        let path = dir.join(format!("story_{name}.png"));
        std::fs::write(&path, stave_raster::png::encode(&canvas)).expect("write png");
        let (w, h) = rt.size();
        println!(
            "{name}: {w}×{h} pt, first paint {:.0} ms, frame {:.1} ms -> {}",
            first.as_secs_f64() * 1e3,
            frame.as_secs_f64() * 1e3,
            path.display()
        );
    }
}

const USAGE: &str = "\
usage: stave-storybook [--screenshot DIR [--scale N]]

  (no options)          live: every part on every theme, draggable;
                        left/right change theme, Esc quits
  --screenshot DIR      one PNG per story into DIR (default build/storybook),
                        offscreen through the real face runtime
  --scale N             pixels per point for --screenshot (default 2)
  -h, --help            this text

STAVE_TEXT=embedded sets text in the embedded fonts on macOS too (how
Windows and Linux show a face).";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let scale = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let dir = args.get(i + 1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("build/storybook"));
        screenshot(dir, scale);
        return;
    }
    live::run();
}
