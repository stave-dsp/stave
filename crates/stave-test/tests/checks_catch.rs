//! The host-behaviour checks catch what they claim to: plugins broken
//! on purpose fail them, and the harness drives through stave-host's
//! `Driver` as an `Instance` (one block driver for everything).

use std::sync::atomic::{AtomicU32, Ordering};

use stave::prelude::*;
use stave_host::instance::Instance;
use stave_host::note::NoteSchedule;
use stave_host::render::Driver;
use stave_measure::signal::Impulse;
use stave_test::{checks, PluginHarness};

#[derive(Params)]
struct Level {
    level: FloatParam,
}

impl Default for Level {
    fn default() -> Self {
        Level { level: FloatParam::new(ParamId(1), "Level", Range::linear(0.0, 2.0)).default(1.0) }
    }
}

/// Gain, but NaN above 200 kHz (a filter that blows up at high rates).
struct HighRateNan {
    p: Arc<Level>,
    rate: f32,
}

impl Plugin for HighRateNan {
    const INFO: Info = info_const("com.stave.test.high-rate-nan");
    type Params = Level;
    fn new(p: &Arc<Level>, _: &Telemetry) -> Self {
        HighRateNan { p: p.clone(), rate: 48_000.0 }
    }
    fn prepare(&mut self, env: &Environment) {
        self.rate = env.sample_rate;
    }
    fn process(&mut self, block: &mut Block<'_>) {
        let g = if self.rate > 200_000.0 { f32::NAN } else { self.p.level.value() };
        let (input, output) = block.io();
        for (x, y) in input.channels().zip(output.channels_mut()) {
            for (x, y) in x.iter().zip(y) {
                *y = x * g;
            }
        }
    }
}

const fn info_const(id: &'static str) -> Info {
    Info { id, name: "broken", vendor: "stave tests", ..Info::EFFECT }
}

/// Every instance made after the first sounds louder: hidden global state.
static MADE: AtomicU32 = AtomicU32::new(0);

struct Global {
    gain: f32,
}

impl Plugin for Global {
    const INFO: Info = info_const("com.stave.test.global");
    type Params = Level;
    fn new(_: &Arc<Level>, _: &Telemetry) -> Self {
        Global { gain: 1.0 + MADE.fetch_add(1, Ordering::Relaxed) as f32 }
    }
    fn process(&mut self, block: &mut Block<'_>) {
        let g = self.gain;
        let (input, output) = block.io();
        for (x, y) in input.channels().zip(output.channels_mut()) {
            for (x, y) in x.iter().zip(y) {
                *y = x * g;
            }
        }
    }
}

/// A one-pole whose `reset` forgets to clear its memory.
struct Sticky {
    z: f32,
}

impl Plugin for Sticky {
    const INFO: Info = info_const("com.stave.test.sticky");
    type Params = Level;
    fn new(_: &Arc<Level>, _: &Telemetry) -> Self {
        Sticky { z: 0.0 }
    }
    fn process(&mut self, block: &mut Block<'_>) {
        let (input, output) = block.io();
        let mut z = self.z;
        for (x, y) in input.channels().zip(output.channels_mut()) {
            z = self.z;
            for (x, y) in x.iter().zip(y) {
                z = 0.9 * z + 0.1 * x;
                *y = z;
            }
        }
        self.z = z;
    }
}

#[test]
#[should_panic(expected = "Hz")]
fn a_plugin_that_fails_at_high_rates_fails_sample_rates() {
    checks::sample_rates::<HighRateNan>();
}

#[test]
#[should_panic(expected = "two fresh instances differ")]
fn hidden_global_state_fails_determinism() {
    checks::deterministic::<Global>();
}

#[test]
#[should_panic(expected = "does not sound as it did fresh")]
fn a_reset_that_keeps_state_fails_the_cycles() {
    checks::prepare_and_reset_cycles::<Sticky>();
}

#[test]
fn sound_plugins_pass() {
    checks::block_sizes::<Sticky>();
    checks::random_automation::<HighRateNan>(0..2);
    checks::state_round_trip::<HighRateNan>();
}

#[test]
fn the_harness_is_an_instance_the_host_driver_drives() {
    let mut h = PluginHarness::<HighRateNan>::builder().block_size(64).set("level", 0.5).build();
    assert_eq!(h.controls().iter().find(|c| c.name == "Level").map(|c| c.value), Some(0.5));
    let mut d = Driver::new(&mut h);
    let cap = d.capture(200, &Impulse { frame: 130 }, &NoteSchedule::default());
    assert_eq!(cap.channel(0)[130], 0.5);
    assert_eq!(cap.channel(1).iter().filter(|&&y| y != 0.0).count(), 1);
}
