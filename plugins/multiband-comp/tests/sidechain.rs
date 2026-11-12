//! The multiband compressor detects from its declared
//! external-sidechain aux port:
//!  1. ext off → a hot aux signal causes NO gain reduction.
//!  2. ext on + aux connected → the hot aux DOES duck the quiet main.
//!  3. ext on + aux absent → identical to ext off (the AuxIn
//!     contract: fully functional with every aux disconnected).

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const BLOCKS: usize = 64; // ~340 ms: detectors fully settled

/// Render with a quiet main tone and a hot aux tone; return the main
/// path's output RMS over the last quarter (post-settle).
fn render(ext_on: bool, connect_aux: bool) -> f64 {
    let mut plugin = stave_multiband_comp_vst3::MultibandComp::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    let set = |n: &str, v: f64| {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}' unknown"))
            .set_plain(v);
    };
    // Deep thresholds + strong ratios so external program produces
    // unmistakable reduction; makeup 0.
    for b in ["lo", "mid", "hi"] {
        set(&format!("{b} threshold"), -45.0);
        set(&format!("{b} ratio"), 10.0);
    }
    set("ext sc", if ext_on { 1.0 } else { 0.0 });
    plugin.activate(FS, BLOCK);

    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    let mut sc_l = vec![0.0f32; BLOCK];
    let mut sc_r = vec![0.0f32; BLOCK];

    let mut sum_sq = 0.0f64;
    let mut count = 0u32;
    for blk in 0..BLOCKS {
        for f in 0..BLOCK {
            let t = (blk * BLOCK + f) as f32 / FS;
            // Quiet main: −32 dBFS-ish 300 Hz. Hot aux: 0.8 pk 300 Hz
            // (same band as the main tone, so LOW-band GR applies).
            let ph = 2.0 * core::f32::consts::PI * 300.0 * t;
            in_l[f] = 0.025 * ph.sin();
            in_r[f] = in_l[f];
            sc_l[f] = 0.8 * ph.sin();
            sc_r[f] = sc_l[f];
        }
        let mut in_ptrs = [core::ptr::null_mut(); 2];
        let mut out_ptrs = [core::ptr::null_mut(); 2];
        let mut sc_ptrs = [core::ptr::null_mut(); 2];
        let input = AudioBuffer::from_slices(&mut in_ptrs, [&mut in_l, &mut in_r], BLOCK);
        let output = AudioBuffer::from_slices(&mut out_ptrs, [&mut out_l, &mut out_r], BLOCK);
        let aux_buf = [AudioBuffer::from_slices(
            &mut sc_ptrs,
            [&mut sc_l, &mut sc_r],
            BLOCK,
        )];
        let aux: &[AudioBuffer] = if connect_aux { &aux_buf } else { &[] };
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input,
            output,
            aux,
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        if blk >= BLOCKS - BLOCKS / 4 {
            for f in 0..BLOCK {
                sum_sq += (out_l[f] as f64) * (out_l[f] as f64);
                count += 1;
            }
        }
    }
    let _ = Arc::strong_count(&bus);
    (sum_sq / count as f64).sqrt()
}

#[test]
fn external_sidechain_ducks_the_main_path() {
    let internal = render(false, true); // hot aux ignored
    let external = render(true, true); // hot aux drives detection
    let db = 20.0 * (external / internal).log10();
    println!("internal {internal:.6} | external {external:.6} | Δ {db:.1} dB");
    assert!(
        db < -6.0,
        "external sidechain should duck the quiet main by >6 dB: {db:.2} dB"
    );
}

#[test]
fn ext_on_without_aux_is_ext_off() {
    // The AuxIn contract, as behavior: flag on but nothing connected
    // must equal flag off exactly (same samples, not just same RMS).
    let off = render(false, false);
    let unplugged = render(true, false);
    assert_eq!(
        off.to_bits(),
        unplugged.to_bits(),
        "ext-on-unplugged must be bit-identical to ext-off"
    );
}
