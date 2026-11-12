//! A plugin with no parameters is a whole plugin: `NoParams` is its
//! table (built by `Default`), `export!` makes it a VST3 and a CLAP
//! plugin, and the harness drives it.

use stave::prelude::*;
use stave_test::PluginHarness;

/// out = −in.
pub struct Invert;

impl Plugin for Invert {
    const INFO: Info = Info {
        id: "com.stave.test.param-less",
        name: "Invert",
        vendor: "stave tests",
        ..Info::EFFECT
    };
    type Params = NoParams;

    fn new(_params: &Arc<NoParams>, _telemetry: &Telemetry) -> Self {
        Invert
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let (input, output) = block.io();
        for (x, y) in input.channels().zip(output.channels_mut()) {
            for (x, y) in x.iter().zip(y) {
                *y = -x;
            }
        }
    }
}

stave::export!(Invert);

#[test]
fn a_plugin_without_params_exports_and_runs() {
    let mut h = PluginHarness::<Invert>::new(48_000.0, 32);
    assert_eq!(h.table().count(), 0);
    assert!(h.table().validate().is_ok());
    h.inputs_mut().for_each(|c| c.fill(0.25));
    h.process(&[]);
    assert!(h.outputs().iter().all(|c| c.iter().all(|&y| y == -0.25)));
    // State round-trips with nothing in the table.
    let saved = h.save();
    h.load(&saved).expect("a param-less state loads");
}
