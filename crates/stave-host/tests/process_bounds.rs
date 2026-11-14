//! `Instance::process` is a safe API: no shape of caller slices may make
//! the hosted plugin read or write past one (audit U1). Ragged channels
//! run for the shortest channel, and longer outputs are silenced past it.

use std::num::NonZeroUsize;

use stave_host::config::{HostConfig, SampleRate};
use stave_host::instance::Instance;
use stave_host::plugin::LoadedPlugin;

const BLOCK: usize = 128;
const GUARD: f32 = 12345.0;

fn tilt() -> LoadedPlugin {
    LoadedPlugin::from_entry_points(
        stave_host::entry_points!(stave_tilt_vst3),
        HostConfig {
            sample_rate: SampleRate::new(48_000.0).unwrap(),
            max_block: NonZeroUsize::new(BLOCK).unwrap(),
            ..HostConfig::default()
        },
    )
    .expect("tilt loads")
}

fn tone(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
}

#[test]
fn a_short_second_output_is_never_written_past_its_end() {
    let mut p = tilt();
    let input = tone(256);
    let mut long = vec![GUARD; 256];
    // The short channel is the front of a larger allocation, so a write
    // past its end lands in `guard`, where the test can see it.
    let mut backing = vec![GUARD; 256];
    let (short, guard) = backing.split_at_mut(64);
    {
        let mut outs: [&mut [f32]; 2] = [&mut long, short];
        p.process(&[&input, &input], &mut outs, &[]);
    }
    assert!(
        guard.iter().all(|&s| s == GUARD),
        "the plugin wrote past the end of the 64-frame output"
    );
    assert!(backing[..64].iter().any(|&s| s != GUARD), "short channel processed");
    assert!(long[..64].iter().any(|&s| s != GUARD), "long channel processed");
    assert!(
        long[64..].iter().all(|&s| s == 0.0),
        "frames past the run are silenced, not stale"
    );
}

#[test]
fn a_short_input_does_not_panic_and_bounds_the_run() {
    let mut p = tilt();
    let a = tone(256);
    let b = tone(64);
    let mut l = vec![GUARD; 256];
    let mut r = vec![GUARD; 256];
    p.process(&[&a, &b], &mut [&mut l, &mut r], &[]);
    assert!(l[..64].iter().all(|&s| s != GUARD));
    assert!(l[64..].iter().chain(&r[64..]).all(|&s| s == 0.0));
}

#[test]
fn no_channels_is_an_empty_run() {
    let mut p = tilt();
    p.process(&[], &mut [], &[]);
}
