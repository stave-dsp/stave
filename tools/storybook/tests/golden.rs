//! Every story as a picture (half scale, to keep the goldens small):
//! the showcase on each theme, the hardware forms, and the controls
//! story on each of its pages. `STAVE_BLESS=1` rewrites them.

use std::sync::Arc;

use stave_core::params::Params;
use stave_storybook::controller::StoryParams;
use stave_storybook::stories;
use stave_test::assert_golden;
use stave_test::face::FaceDriver;

/// The story named `name`, in each of its states, against its goldens.
fn looks_right(name: &str) {
    let i = stories::all(&StoryParams::default()).iter().position(|(n, _)| n == name).unwrap_or_else(|| panic!("no story {name}"));
    let story = || {
        let params = Arc::new(StoryParams::default());
        let face = stories::all(&params).swap_remove(i).1;
        FaceDriver::new(face, params as Arc<dyn Params>, Vec::new())
    };
    let file = name.replace(' ', "-");
    for (k, state) in story().states().into_iter().enumerate() {
        let mut ui = story();
        ui.show(&state);
        let suffix = if k == 0 { String::new() } else { format!("-state{k}") };
        assert_golden!(&format!("story-{file}{suffix}"), &ui.render(0.5));
    }
}

macro_rules! story_goldens {
    ($($test:ident: $name:literal),* $(,)?) => {$(
        #[test]
        #[cfg_attr(miri, ignore)]
        fn $test() {
            looks_right($name);
        }
    )*};
}

story_goldens! {
    graphite_looks_right: "graphite",
    cream_looks_right: "cream",
    hammertone_looks_right: "hammertone",
    indigo_looks_right: "indigo",
    oxblood_looks_right: "oxblood",
    verdigris_looks_right: "verdigris",
    amp_looks_right: "amp",
    rack_looks_right: "rack",
    cabinet_looks_right: "cabinet",
    tabs_looks_right: "tabs",
    controls_looks_right: "controls",
}
