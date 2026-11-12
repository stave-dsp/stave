//! Block-API bit-exactness limits: every
//! block-mode path must be BIT-identical to the per-sample path it
//! shadows — same components, same sample order, so any divergence is
//! a bug, not a tolerance. Blocks are deliberately ragged (LCG-drawn
//! sizes, 1..=192) so run/boundary handling is exercised, not just the
//! nice power-of-two case.

use stave_dsp::{
    shapes, Biquad, Chain, CurveStage, FilterSpec, Hysteresis, Oversampled2x, Slew, Stage, StageId,
    StageKind,
};

fn lcg_noise(n: usize, mut state: u32) -> Vec<f32> {
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

/// Ragged block sizes covering `total` samples.
fn ragged_blocks(total: usize, mut state: u32) -> Vec<usize> {
    let mut out = vec![];
    let mut left = total;
    while left > 0 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let b = (1 + (state >> 20) as usize % 192).min(left);
        out.push(b);
        left -= b;
    }
    out
}

/// A stateful nonlinear inner block — the realistic oversampler payload
/// (memory means any op-order slip shows up and STAYS visible).
struct StatefulInner {
    z: f32,
}
impl Stage for StatefulInner {
    fn tick(&mut self, x: f32) -> f32 {
        self.z = 0.95 * self.z + 0.05 * x;
        (x + 0.5 * self.z).tanh()
    }
}

#[test]
fn nested_fir_block_is_bit_identical_to_per_sample() {
    let input = lcg_noise(9_600, 0xBEEF);
    let stack = || Oversampled2x::front(Oversampled2x::nested(StatefulInner { z: 0.0 }));
    // Per-sample reference.
    let mut a = stack();
    a.prepare(48_000.0);
    let ref_out: Vec<f32> = input.iter().map(|&x| a.tick(x)).collect();
    // Block path, ragged sizes (crossing the region's internal chunk).
    let mut b = stack();
    b.prepare(48_000.0);
    let mut buf = input.clone();
    let mut pos = 0;
    for blk in ragged_blocks(input.len(), 0xCAFE) {
        b.process(&mut buf[pos..pos + blk]);
        pos += blk;
    }
    for (i, (r, b)) in ref_out.iter().zip(buf.iter()).enumerate() {
        assert_eq!(r.to_bits(), b.to_bits(), "sample {i}: {r} vs {b}");
    }
}

fn amp_like_chain() -> Chain<5> {
    let mut ch = Chain::<5>::new();
    ch.push(StageKind::Curve(CurveStage::from_fn(
        -6.0,
        6.0,
        0.0,
        shapes::knee(2.2),
    )))
    .unwrap();
    ch.push(StageKind::Filter(Biquad::new(FilterSpec::Peak {
        hz: 800.0,
        db: -6.0,
        q: 1.1,
    })))
    .unwrap();
    ch.push(StageKind::Curve(CurveStage::from_fn(
        -4.0,
        4.0,
        0.0,
        shapes::knee(3.0),
    )))
    .unwrap();
    ch.push(StageKind::Slew(Slew::new(150.0, 200.0))).unwrap();
    ch.push(StageKind::Hysteresis(Hysteresis::new(0.8, 0.2)))
        .unwrap();
    ch
}

#[test]
fn chain_block_is_bit_identical_and_taps_agree() {
    let input = lcg_noise(8_192, 0x1234);
    let mut a = amp_like_chain();
    let mut b = amp_like_chain();
    a.prepare(192_000.0);
    b.prepare(192_000.0);
    let mut refb = input.clone();
    let mut buf = input.clone();
    let mut pos = 0;
    for blk in ragged_blocks(input.len(), 0x5678) {
        for s in &mut refb[pos..pos + blk] {
            *s = a.tick(*s);
        }
        b.process(&mut buf[pos..pos + blk]);
        // The block-rate detector contract: taps agree at every block
        // boundary (that is where ModMatrix reads them).
        for i in 0..5 {
            assert_eq!(
                a.sense(StageId(i)).to_bits(),
                b.sense(StageId(i)).to_bits(),
                "tap {i} diverged at sample {pos}"
            );
        }
        pos += blk;
    }
    for (i, (r, v)) in refb.iter().zip(buf.iter()).enumerate() {
        assert_eq!(r.to_bits(), v.to_bits(), "sample {i}: {r} vs {v}");
    }
}

#[test]
fn chain_with_feedback_falls_back_and_still_matches() {
    let input = lcg_noise(4_096, 0x7777);
    let mut a = amp_like_chain();
    let mut b = amp_like_chain();
    for ch in [&mut a, &mut b] {
        ch.set_feedback(
            -0.12,
            StageId(4),
            StageId(0),
            Some(FilterSpec::HighShelf {
                hz: 2_000.0,
                db: 6.0,
            }),
        );
        ch.prepare(192_000.0);
    }
    let mut refb = input.clone();
    let mut buf = input;
    let mut pos = 0;
    for blk in ragged_blocks(buf.len(), 0x8888) {
        for s in &mut refb[pos..pos + blk] {
            *s = a.tick(*s);
        }
        b.process(&mut buf[pos..pos + blk]);
        pos += blk;
    }
    for (i, (r, v)) in refb.iter().zip(buf.iter()).enumerate() {
        assert_eq!(r.to_bits(), v.to_bits(), "sample {i}: {r} vs {v}");
    }
}
