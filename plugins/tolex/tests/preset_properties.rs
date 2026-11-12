//! Preset laws — a flagship preset is a PROMISE, so it gets the law
//! treatment: BREAKDOWN (the modern-metalcore cabinet) must be
//! TIGHT below the chug, present in the mix band, and controlled
//! above the fizz line. Measured on rendered audio, so any future
//! retune of the engine that silently un-metals the preset fails
//! the build.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

const BREAKDOWN: &[(&str, f64)] = &[
    ("size", 2.0),
    ("resonance", 92.0),
    ("q", 0.7),
    ("mass", 1.05),
    ("cone", 21.0),
    ("breakup", 0.6),
    ("doping", 0.38),
    ("drive", 6.0),
    ("damping", 0.3),
    ("back", 0.0),
    ("cab", 170.0),
    ("material", 0.0),
    ("speakers", 2.0),
    ("model", 1.0),
    ("mic", 2.5),
    ("position", 0.15),
];

fn gain_db(freq: f32) -> f64 {
    let mut plugin = stave_tolex_vst3::Tolex::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in BREAKDOWN {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let n = 24_064usize;
    let input: Vec<f32> = (0..n)
        .map(|i| (core::f32::consts::TAU * freq * i as f32 / FS).sin() * 0.1)
        .collect();
    let mut out = Vec::with_capacity(n);
    let (mut in_l, mut in_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut out_l, mut out_r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_r.copy_from_slice(&in_l);
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let inb = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let outb = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input: inb,
            output: outb,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        out.extend_from_slice(&out_l);
    }
    let _ = Arc::strong_count(&bus);
    let rms = |w: &[f32]| {
        (w.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / w.len() as f64).sqrt()
    };
    20.0 * (rms(&out[12_032..]) / rms(&input[12_032..])).log10()
}

#[test]
fn the_breakdown_holds() {
    // TIGHT: the chug shelf stands well above the sub it refuses.
    let tight = gain_db(105.0) - gain_db(60.0);
    assert!(
        tight >= 4.0,
        "chug shelf only {tight:.1} dB above 60 Hz — not tight"
    );
    // PRESENT: the mix band's teeth are not scooped away.
    let presence = gain_db(2_800.0) - gain_db(500.0);
    assert!(
        presence >= -4.0,
        "2.8 kHz sits {presence:.1} dB under 500 — buried"
    );
    // CONTROLLED: the fizz line falls hard above the teeth.
    let fizz = gain_db(7_500.0) - gain_db(2_800.0);
    assert!(
        fizz <= -6.0,
        "7.5 kHz only {fizz:.1} dB under 2.8 k — fizzy"
    );
}
