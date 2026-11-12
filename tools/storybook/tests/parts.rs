//! Every part in the catalogue as a picture of its own
//! (`stave_storybook::catalogue`): what each part looks like, at rest and
//! with the readings that light it. A part whose picture changes shows
//! up here by name, not as a pixel somewhere in a story; the docs site
//! shows these same pictures. `STAVE_BLESS=1` rewrites them.

use stave_storybook::catalogue::{picture, Text, ENTRIES};
use stave_test::assert_golden;

#[test]
#[cfg_attr(miri, ignore)]
fn every_part_looks_right() {
    for entry in ENTRIES {
        assert_golden!(&format!("part-{}", entry.name), &picture(entry, 1.0, Text::Boxes));
    }
}

#[test]
fn picture_names_are_unique() {
    let mut names: Vec<_> = ENTRIES.iter().map(|e| e.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ENTRIES.len());
}
