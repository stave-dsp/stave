//! The live storybook: one window showing the showcase face, driven by
//! the stand-in controller. Everything drags; ←/→ step through the
//! stories; Esc or ⌘Q quits.

use stave_ui_cocoa::FaceView;

use crate::controller::StoryController;
use crate::stories;

const ESCAPE: u16 = 53;
const LEFT: u16 = 123;
const RIGHT: u16 = 124;

pub fn run() {
    let controller = StoryController::new();
    let params = controller.params.clone();
    let count = stories::all(&params).len();
    let mut view = FaceView::new(stories::all(&params).swap_remove(0).1, Box::new(controller));
    let mut at = 0;
    view.on_key(move |code| {
        match code {
            ESCAPE => std::process::exit(0),
            LEFT => at = (at + count - 1) % count,
            RIGHT => at = (at + 1) % count,
            _ => return None,
        }
        Some(stories::all(&params).swap_remove(at).1)
    });
    stave_ui_cocoa::run_app("stave storybook  (←/→ story)", view);
}
