//! Every story keeps the interaction laws (stave_test::face::laws), on
//! every page and step: the design system's parts, each held to what a
//! person's hands rely on.

use std::sync::Arc;

use stave_core::params::Params;
use stave_storybook::controller::StoryParams;
use stave_storybook::stories;
use stave_test::face::{laws, FaceDriver};

/// The story at `i` over fresh params.
fn story(i: usize) -> FaceDriver {
    let params = Arc::new(StoryParams::default());
    let face = stories::all(&params).swap_remove(i).1;
    FaceDriver::new(face, params as Arc<dyn Params>, Vec::new())
}

/// Each story shows a few of the demo parameters, not all.
const PARTIAL: laws::Laws = laws::Laws {
    off_face: &["DRIVE", "TONE", "MIX", "TIME", "RATE", "DEPTH", "SHAPE", "SYNC", "LEVEL", "BYPASS", "HOLD", "WIDTH"],
};

/// The story named `name`, held to every law.
fn laws_of(name: &str) {
    let i = stories::all(&StoryParams::default()).iter().position(|(n, _)| n == name).unwrap_or_else(|| panic!("no story {name}"));
    laws::assert_all(&format!("story {name}"), || story(i), &PARTIAL);
}

/// One test per story, so they run side by side.
macro_rules! story_laws {
    ($($test:ident: $name:literal),* $(,)?) => {
        $(
            #[test]
            #[cfg_attr(miri, ignore)]
            fn $test() {
                laws_of($name);
            }
        )*

        #[test]
        fn every_story_has_its_test() {
            let names: Vec<String> = stories::all(&StoryParams::default()).into_iter().map(|(n, _)| n).collect();
            assert_eq!(names, [$($name),*], "a story was added or renamed: list it in story_laws!");
        }
    };
}

story_laws! {
    graphite_keeps_the_laws: "graphite",
    cream_keeps_the_laws: "cream",
    hammertone_keeps_the_laws: "hammertone",
    oxblood_keeps_the_laws: "oxblood",
    verdigris_keeps_the_laws: "verdigris",
    indigo_keeps_the_laws: "indigo",
    amp_keeps_the_laws: "amp",
    rack_keeps_the_laws: "rack",
    cabinet_keeps_the_laws: "cabinet",
    tabs_keeps_the_laws: "tabs",
    controls_keeps_the_laws: "controls",
}

/// The controls story reaches every gesture kind the laws know, across
/// its pages and steps (so the sweep above covers them all).
#[test]
fn the_controls_story_covers_every_gesture_kind() {
    use stave_face::Gesture;
    let last = stories::all(&StoryParams::default()).len() - 1;
    let ui = story(last);
    let states = ui.states();
    assert!(states.len() >= 4, "two pages and two steps: {states:?}");
    let mut kinds = std::collections::BTreeSet::new();
    for s in &states {
        let mut u = story(last);
        u.show(s);
        for (_, _, g) in u.runtime().all_grabs(u.controller()) {
            kinds.insert(match g.gesture {
                Gesture::Rotary { .. } => "rotary",
                Gesture::Stepped { .. } => "stepped",
                Gesture::Choose { .. } => "choose",
                Gesture::Latch { .. } => "latch",
                Gesture::Momentary { .. } => "momentary",
                Gesture::Pad { .. } => "pad",
                Gesture::Nudge { .. } => "nudge",
                Gesture::Preset { .. } => "preset",
                Gesture::Page { .. } => "page",
            });
        }
    }
    for k in ["rotary", "choose", "momentary", "pad", "nudge", "page"] {
        assert!(kinds.contains(k), "{k} missing: {kinds:?}");
    }
}

