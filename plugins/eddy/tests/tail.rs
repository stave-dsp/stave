//! Tail-quality laws — the reverb characteristics measured, so "how
//! good is the tail" is a number, not an opinion. A single impulse
//! with NO outer feedback isolates the field's ring; the metrics are
//! the standard ones:
//! * NED (normalized echo density, Abel/Huang): fraction of samples
//!   beyond the window std over the Gaussian expectation — 1.0 is a
//!   fully dense tail, spiky echo trains score low.
//! * Comb-ness: peak normalized autocorrelation over audible lags —
//!   a flangy/periodic tail shows its loop as a strong peak (the
//!   single-loop plate this tank replaced measured ~0.9; the law
//!   holds the tank under 0.35).
//! * EDC ripple: RMS deviation of the Schroeder decay curve from a
//!   straight dB line between −5 and −30 dB — beating and warble
//!   read as ripple; a smooth exponential is what ears call
//!   "expensive".
//!
//! Anchors measured 2026-07-06: PLATE NED 0.84 mid / 0.68 late,
//! comb 0.23, EDC 1.07 dB; MESH NED 0.45 / 0.39 (four strands — a
//! sparser space by design), comb 0.23, EDC 0.76 dB.

use std::sync::Arc;

use stave_core::{AudioBuffer, DebugBus, Plugin, ProcessCtx};
use stave_measure::tail::{autocorr_peak, edc_ripple_db, ned};

const FS: f32 = 48_000.0;
const BLOCK: usize = 256;

fn render(set: &[(&str, f64)], input: &[f32]) -> Vec<f32> {
    let mut plugin = stave_eddy_vst3::Eddy::new();
    let bus = DebugBus::new();
    plugin.debug_attach(bus.clone());
    let table = plugin.param_table();
    for (n, v) in set {
        table
            .by_name(n)
            .unwrap_or_else(|| panic!("param '{n}'"))
            .set_plain(*v);
    }
    plugin.activate(FS, BLOCK);
    let mut out = Vec::with_capacity(input.len());
    let mut in_l = vec![0.0f32; BLOCK];
    let mut in_r = vec![0.0f32; BLOCK];
    let mut out_l = vec![0.0f32; BLOCK];
    let mut out_r = vec![0.0f32; BLOCK];
    for chunk in input.chunks(BLOCK) {
        in_l[..chunk.len()].copy_from_slice(chunk);
        in_l[chunk.len()..].fill(0.0);
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
    out
}

/// The isolated field ring: impulse in, zero outer feedback.
fn field_ring(kind: usize) -> Vec<f32> {
    let mut imp = vec![0.0f32; 144_000];
    imp[0] = 1.0;
    render(
        &[
            ("time", 20.0),
            ("feedback", 0.0),
            ("dry", 0.0),
            ("wet", 1.0),
            ("diffusion", kind as f64),
            ("diffuse", 1.0),
            ("hold", 0.9),
            ("bleed", 0.0),
            ("density", 0.8),
            ("wander", 1.0),
            ("weave", 0.8),
            ("size", 0.6),
        ],
        &imp,
    )
}

/// The tail is DENSE: by the mid-tail the field reads as a near-
/// Gaussian wash, not a train of spikes. MESH's floor is lower — a
/// four-strand space is sparser by design.
#[test]
fn the_tail_thickens_to_density() {
    for (kind, mid_floor, late_floor) in [(0usize, 0.7, 0.5), (3, 0.3, 0.25)] {
        let out = field_ring(kind);
        let mid = ned(&out[24_000..48_000]);
        let late = ned(&out[48_000..120_000]);
        assert!(
            mid > mid_floor && late > late_floor,
            "voice {kind}: NED mid {mid:.2} / late {late:.2} — a spike train, not a wash"
        );
    }
}

/// The tail is NOT A COMB: no audible lag dominates the late field.
/// The single-loop plate this law retired measured ~0.9 at its 22 ms
/// loop — the flangy delay-tail the ears caught; the tank and the
/// damped woven mesh hold under 0.35.
#[test]
fn the_tail_is_not_a_comb() {
    for kind in [0usize, 3] {
        let out = field_ring(kind);
        let comb = autocorr_peak(&out[48_000..120_000], 150..4_000);
        assert!(
            comb < 0.35,
            "voice {kind}: late-tail autocorrelation {comb:.3} — a flangy loop, not a field"
        );
    }
}

/// The tail DECAYS SMOOTHLY: the Schroeder curve hugs a straight dB
/// line (beating or warble reads as ripple — what separates an
/// expensive tail from a ringing one).
#[test]
fn the_tail_decays_smoothly() {
    for kind in [0usize, 3] {
        let out = field_ring(kind);
        let ripple = edc_ripple_db(&out[2_000..]);
        assert!(
            ripple < 2.0,
            "voice {kind}: EDC ripple {ripple:.2} dB — the decay beats instead of fading"
        );
    }
}
