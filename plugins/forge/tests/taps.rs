//! An instrument attaches to a processor's DECLARED
//! observation points through typed handles — no name strings on the
//! instrument side, no drift possible between what the engine
//! publishes and what the measurement reads. The bus is transport.

use stave_core::debug::DebugBus;
use stave_core::Plugin;
use stave_forge_vst3::Forge;
use stave_test::PluginHarness;

const FS: f64 = 48_000.0;
const BLOCK: usize = 256;

#[test]
fn instrument_attaches_through_declared_handles() {
    let mut h = PluginHarness::<Forge>::new(FS as f32, BLOCK);
    let bus = DebugBus::new();
    h.plugin_mut().debug_attach(bus.clone());

    // Silence first: taps publish, levels read the floor.
    let quiet = vec![0.0f32; BLOCK];
    h.set_input(0, &quiet);
    h.set_input(1, &quiet);
    h.process_block(&[]);
    let taps = h.plugin().engine().taps();
    let floor = taps.io_in.get(&bus).expect("io_in tap published");
    assert!(
        floor < -100.0,
        "silence should read the dB floor, got {floor:.1}"
    );

    // A hot tone: the same handles read program level in and out.
    let mut buf = vec![0.0f32; BLOCK];
    for _ in 0..24 {
        for (i, s) in buf.iter_mut().enumerate() {
            *s = 0.5 * (2.0 * std::f64::consts::PI * 997.0 * i as f64 / FS).sin() as f32;
        }
        h.set_input(0, &buf);
        h.set_input(1, &buf);
        h.process_block(&[]);
    }
    let taps = h.plugin().engine().taps();
    let in_db = taps.io_in.get(&bus).unwrap();
    let out_db = taps.io_out.get(&bus).unwrap();
    assert!(
        (in_db + 6.0).abs() < 1.0,
        "0.5 FS peak ≈ −6 dBFS, got {in_db:.1}"
    );
    assert!(
        out_db > -40.0,
        "wet path alive through the out tap, got {out_db:.1}"
    );

    // Stage taps carry values on stable wires; labels are nowhere in
    // the wire names (the editor declaration owns them).
    for tap in &taps.stage {
        assert!(
            !tap.name().contains(':'),
            "label leaked into wire name {}",
            tap.name()
        );
    }
    assert!(
        taps.stage[0].get(&bus).unwrap_or(0.0) > 0.0,
        "stage 1 tap saw program"
    );
}
