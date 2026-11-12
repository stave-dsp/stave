//! `RouteSlot` — one modulation route with its SHAPING chain,
//! shared by every synth that modulates.
//!
//! A slot is seven params' worth of block-rate data: which SOURCE
//! (an index into the consumer's source table; 0 = OFF), which
//! TARGET (the consumer's vocabulary — the kernel never names a
//! product inlet, the `Route<T>` rule), a bipolar DEPTH, and the
//! shaping chain — VIA (depth scaled by a second source,
//! mod-the-mod), MODE (bipolar → unipolar remap), QUANT (snap to N
//! rungs), LAG (one-pole slew; the caller owns one state per slot
//! per voice and derives `lag_k` from its own block rate).
//!
//! The APPLICATION invariant stays with the consumer (displace-from-base:
//! knobs set the base, routes displace, they never fight) and so do
//! the target semantics — this type owns only what a route VALUE is.

/// One slot, snapshotted from its seven params per block.
#[derive(Copy, Clone, Debug, Default)]
pub struct RouteSlot {
    /// Source table index; 0 = OFF (the slot is silent).
    pub src: usize,
    /// Target index — the CONSUMER's vocabulary.
    pub tgt: usize,
    /// Bipolar depth, −1..1.
    pub depth: f32,
    /// Second source scaling the depth (unipolar); 0 = none.
    pub via: usize,
    /// Remap the shaped value from ±1 to 0..1 (envelope-style).
    pub uni: bool,
    /// One-pole coefficient (1.0 = no lag), derived by the caller
    /// from the lag-ms knob and ITS block rate.
    pub lag_k: f32,
    /// Quantizer rung count; < 2 = off.
    pub quant: f32,
}

impl RouteSlot {
    pub fn active(&self) -> bool {
        self.src != 0
    }

    /// The shaping chain: polarity → quantize → lag. The lag state
    /// is the caller's, one per slot per voice.
    #[inline]
    pub fn shape(&self, raw: f32, lag: &mut f32) -> f32 {
        let mut v = raw;
        if self.uni {
            v = (v + 1.0) * 0.5;
        }
        if self.quant >= 2.0 {
            v = (v * self.quant).round() / self.quant;
        }
        *lag += self.lag_k * (v - *lag);
        *lag
    }

    /// The slot's raw source value, `sources[src - 1]`; None when OFF.
    #[inline]
    pub fn source(&self, sources: &[f32]) -> Option<f32> {
        sources.get(self.src.checked_sub(1)?).copied()
    }

    /// The slot's contribution for one voice: `sources[src - 1]`
    /// shaped, times the (via-scaled) depth. `sources` is the
    /// consumer's table in enum-step order WITHOUT the OFF step —
    /// index k in the params maps to `sources[k - 1]`. Returns
    /// None when the slot is OFF (lag state untouched — the
    /// bit-identity invariant depends on it).
    #[inline]
    pub fn value(&self, sources: &[f32], lag: &mut f32) -> Option<f32> {
        let raw = self.source(sources)?;
        let v = self.shape(raw, lag);
        let depth = match self.via.checked_sub(1).and_then(|i| sources.get(i)) {
            Some(g) => self.depth * ((g + 1.0) * 0.5),
            None => self.depth,
        };
        Some(depth * v)
    }
}

#[cfg(test)]
mod properties {
    use super::*;

    fn slot(src: usize, depth: f32) -> RouteSlot {
        RouteSlot {
            src,
            tgt: 0,
            depth,
            via: 0,
            uni: false,
            lag_k: 1.0,
            quant: 0.0,
        }
    }

    /// UNI remaps ±1 → 0..1: a source at its trough contributes
    /// ZERO, not negative — the envelope-style opening.
    #[test]
    fn unipolar_mode_opens_from_zero() {
        let mut s = slot(1, 1.0);
        s.uni = true;
        let mut lag = 0.0;
        assert_eq!(s.value(&[-1.0], &mut lag), Some(0.0));
        let mut lag = 0.0;
        assert_eq!(s.value(&[1.0], &mut lag), Some(1.0));
    }

    /// The quantizer snaps to exactly N rungs — a saw through it is
    /// an arpeggio, not a slide.
    #[test]
    fn quantize_snaps_to_the_declared_rungs() {
        let mut s = slot(1, 1.0);
        s.quant = 4.0;
        for (x, want) in [
            (0.30f32, 0.25f32),
            (0.35, 0.25),
            (0.40, 0.50),
            (-0.30, -0.25),
        ] {
            let mut lag = 0.0;
            let v = s.shape(x, &mut lag);
            assert!((v - want).abs() < 1e-6, "{x} → {v}, want {want}");
        }
    }

    /// Lag is a one-pole: monotone toward a step, k=1 transparent.
    #[test]
    fn lag_slews_and_off_is_transparent() {
        let mut s = slot(1, 1.0);
        s.lag_k = 0.25;
        let mut lag = 0.0;
        let a = s.shape(1.0, &mut lag);
        let b = s.shape(1.0, &mut lag);
        let c = s.shape(1.0, &mut lag);
        assert!(
            a < b && b < c && c < 1.0,
            "monotone toward the step: {a} {b} {c}"
        );
        s.lag_k = 1.0;
        let mut lag = 0.0;
        assert_eq!(s.shape(0.7, &mut lag), 0.7);
    }

    /// VIA scales DEPTH by the second source, unipolar.
    #[test]
    fn via_scales_depth_by_the_second_source() {
        let mut s = slot(1, 1.0);
        s.via = 2;
        let mut lag = 0.0;
        assert_eq!(s.value(&[1.0, 1.0], &mut lag), Some(1.0));
        let mut lag = 0.0;
        assert_eq!(s.value(&[1.0, -1.0], &mut lag), Some(0.0));
        let mut lag = 0.0;
        assert_eq!(s.value(&[1.0, 0.0], &mut lag), Some(0.5));
    }

    /// OFF slots touch NOTHING — the bit-identity invariant's foundation.
    #[test]
    fn off_slots_leave_the_lag_state_alone() {
        let s = slot(0, 1.0);
        let mut lag = 0.5;
        assert_eq!(s.value(&[1.0], &mut lag), None);
        assert_eq!(lag, 0.5);
    }
}
