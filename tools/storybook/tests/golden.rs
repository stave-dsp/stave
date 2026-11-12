//! Every story as a picture (half scale, to keep the goldens small):
//! the showcase on each theme, the hardware forms, and the controls
//! story on each of its pages. `STAVE_BLESS=1` rewrites them.

use std::sync::Arc;

use stave_core::params::Params;
use stave_storybook::controller::StoryParams;
use stave_storybook::stories;
use stave_test::assert_golden;
use stave_test::face::FaceDriver;

#[test]
#[cfg_attr(miri, ignore)]
fn every_story_looks_as_it_should() {
    let names: Vec<String> = stories::all(&StoryParams::default()).into_iter().map(|(n, _)| n).collect();
    for (i, name) in names.iter().enumerate() {
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
}
