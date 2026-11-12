//! Smear's 5.1 variant of the PerChannel lift. Two laws:
//!  1. The lift changes WHO the audio is, never what happens to it —
//!     the stereo pair renders bit-identically whether the buffer is
//!     2-lane or 6-lane (lanes are independent by construction).
//!  2. Each surround lane is its own garden — identical input to all
//!     six lanes produces six DIFFERENT outputs (per-lane seeds).

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;
const BLOCKS: usize = 32;

fn make_plugin() -> stave_smear_vst3::Smear {
    let mut plugin = stave_smear_vst3::Smear::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in [
        ("seed", 0.7),
        ("smear", 0.5),
        ("decay", 0.6),
        ("depth", 0.8),
        ("motion", 0.5),
        ("flux", 0.4),
    ] {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(v);
    }
    plugin.activate(FS, BLOCK);
    let _ = Arc::strong_count(&bus);
    plugin
}

/// Render `lanes` channels; lane signals come from `sig(lane, i)`.
/// Returns each lane's full output.
fn render(lanes: usize, sig: impl Fn(usize, usize) -> f32) -> Vec<Vec<f32>> {
    let mut plugin = make_plugin();
    let mut ins: Vec<Vec<f32>> = vec![vec![0.0; BLOCK]; lanes];
    let mut outs: Vec<Vec<f32>> = vec![vec![0.0; BLOCK]; lanes];
    let mut collected: Vec<Vec<f32>> = vec![Vec::new(); lanes];
    for blk in 0..BLOCKS {
        for l in 0..lanes {
            for f in 0..BLOCK {
                ins[l][f] = sig(l, blk * BLOCK + f);
            }
        }
        let mut in_ptrs = vec![core::ptr::null_mut(); ins.len()];
        let mut out_ptrs = vec![core::ptr::null_mut(); outs.len()];
        let input = AudioBuffer::from_slices(&mut in_ptrs, ins.iter_mut(), BLOCK);
        let output = AudioBuffer::from_slices(&mut out_ptrs, outs.iter_mut(), BLOCK);
        let mut ctx = ProcessCtx {
            sample_rate: FS,
            frames: BLOCK,
            transport: None,
            input,
            output,
            aux: &[],
            params: stave_core::ParamBlock::empty(),
            events: &[],
        };
        plugin.process(&mut ctx);
        for l in 0..lanes {
            collected[l].extend_from_slice(&outs[l]);
        }
    }
    collected
}

fn tone(lane: usize, i: usize) -> f32 {
    let t = i as f32 / FS;
    0.4 * (2.0 * core::f32::consts::PI * (220.0 + 30.0 * lane as f32) * t).sin()
}

#[test]
fn stereo_pair_is_bit_identical_through_the_lift() {
    let stereo = render(2, tone);
    let surround = render(6, tone);
    for l in 0..2 {
        assert_eq!(stereo[l].len(), surround[l].len());
        for (i, (a, b)) in stereo[l].iter().zip(surround[l].iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "lane {l} diverged at sample {i}: {a} vs {b}"
            );
        }
    }
}

#[test]
fn surround_lanes_are_independent_gardens() {
    // Identical input to every lane: outputs must differ (per-lane
    // seeds drive independent detune walks) and all be finite.
    let outs = render(6, |_, i| tone(0, i));
    for l in 0..6 {
        assert!(outs[l].iter().all(|x| x.is_finite()), "lane {l} not finite");
        assert!(outs[l].iter().any(|x| x.abs() > 1e-6), "lane {l} silent");
    }
    for a in 0..6 {
        for b in (a + 1)..6 {
            assert!(
                outs[a] != outs[b],
                "lanes {a} and {b} are identical — gardens not independent"
            );
        }
    }
}
