//! stave-storybook — the design system's storybook: a showcase face
//! using every part, on every built-in theme, driven by a stand-in
//! plugin (an in-memory parameter table and probes that move with time).
//!
//! Usage:
//!   stave-storybook                                # live: drag everything; ←/→ change theme, Esc quits
//!   stave-storybook --screenshot DIR [--scale 2]   # one PNG per theme
//!
//! Screenshots are rendered offscreen through the real face runtime and
//! the platform's text, so they show exactly what a plugin window shows.

mod host;
mod live;
mod stories;

use std::path::PathBuf;

use stave_face::Runtime;

fn screenshot(dir: PathBuf, scale: f64) {
    std::fs::create_dir_all(&dir).expect("create screenshot dir");
    let names: Vec<String> = stories::all(&host::StoryHost::new()).into_iter().map(|(n, _)| n).collect();
    for (i, name) in names.iter().enumerate() {
        let host = host::StoryHost::new();
        host.time.set(1.7);
        let face = stories::all(&host).swap_remove(i).1;
        let mut rt = Runtime::new(face);
        if std::env::var_os("STAVE_FACE_DEBUG").is_some() {
            for r in rt.rects() {
                println!("  {:7.1} {:7.1} {:7.1} {:7.1}", r.x, r.y, r.w, r.h);
            }
        }
        let mut canvas = stave_raster::Canvas::new(0, 0);
        let t0 = std::time::Instant::now();
        rt.render(&host, scale, host.time.get(), &mut canvas, &mut stave_ui_cocoa::offscreen::CoreText::new(scale));
        let first = t0.elapsed();
        let t1 = std::time::Instant::now();
        rt.render(&host, scale, host.time.get(), &mut canvas, &mut stave_ui_cocoa::offscreen::CoreText::new(scale));
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
