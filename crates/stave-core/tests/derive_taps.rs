//! `#[derive(Taps)]`: one `TapId` per field, registration, a detached
//! default.

use stave_core::telemetry::{Tap, TapId, Taps, Telemetry};

#[derive(Taps)]
struct MeterTaps {
    gain_reduction: Tap,
    r#type: Tap,
}

#[test]
fn every_field_gets_an_id_named_after_it() {
    assert_eq!(MeterTaps::GAIN_REDUCTION, TapId::new("gain_reduction"));
    assert_eq!(MeterTaps::TYPE.name(), "type");
    assert_eq!(MeterTaps::ALL, [MeterTaps::GAIN_REDUCTION, MeterTaps::TYPE]);
}

#[test]
fn new_registers_every_tap_and_readers_resolve_by_id() {
    let telemetry = Telemetry::new();
    let taps = MeterTaps::new(&telemetry);
    assert!(taps.gain_reduction.is_attached() && taps.r#type.is_attached());
    assert_eq!(telemetry.taps().len(), 2, "registered before the first set");
    taps.gain_reduction.set(-6.0);
    assert_eq!(telemetry.value(MeterTaps::GAIN_REDUCTION), Some(-6.0));
    assert_eq!(telemetry.value(MeterTaps::TYPE), None);
    // A second table on the same telemetry shares the slots.
    let reader = MeterTaps::new(&telemetry);
    assert_eq!(reader.gain_reduction.get(), -6.0);
    assert_eq!(telemetry.taps().len(), 2);
}

#[test]
fn default_is_detached() {
    let taps = MeterTaps::default();
    taps.gain_reduction.set(1.0);
    assert!(!taps.gain_reduction.is_attached());
    assert_eq!(taps.gain_reduction.get(), 0.0);
}
