//! The event trace (docs/overhaul.md V5) of a real stave plugin (Tilt)
//! driven through the VST3 path: what the host did — activate, state
//! saves and loads, good and garbage — is on the instance's telemetry as
//! typed records, and the run keeps the trace's laws.

use std::num::NonZeroUsize;

use stave_host::config::{HostConfig, SampleRate};
use stave_host::instance::Instance;
use stave_host::plugin::LoadedPlugin;
use stave_telemetry::events::{check, quiescent, Recorder};
use stave_telemetry::Event;

#[test]
fn a_host_session_is_a_checkable_trace() {
    let mut p = LoadedPlugin::from_entry_points(
        stave_host::entry_points!(stave_tilt_vst3),
        HostConfig {
            sample_rate: SampleRate::new(48_000.0).unwrap(),
            max_block: NonZeroUsize::new(256).unwrap(),
            ..HostConfig::default()
        },
    )
    .expect("tilt loads");
    let saved = p.save_state().expect("saves");
    p.load_state(&saved).expect("loads what it saved");
    assert!(p.load_state(b"not a state").is_err());

    // SAFETY: the telemetry is the instance's, alive while `p` is.
    let telemetry = unsafe { &*p.telemetry_raw() };
    let mut rec = Recorder::new();
    rec.drain(telemetry.events());
    let trace = rec.trace();
    let events = trace.events();
    assert_eq!(events.first(), Some(&Event::Prepare { sample_rate: 48_000.0, max_block: 256 }), "{trace}");
    assert!(events.ends_with(&[Event::Restore(true), Event::Restore(false)]), "{trace}");
    assert_eq!(check(&trace), vec![], "{trace}");
    assert_eq!(quiescent(&trace), Ok(()));
}
