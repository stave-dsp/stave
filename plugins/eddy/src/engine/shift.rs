//! SHIFT — every repeat is TRANSPOSED by the dialed interval: the
//! wet tap passes a dual-head crossfade transposer (the classic
//! delay-line harmonizer: a tap whose delay sweeps at 1 − ratio, a
//! twin half a window behind, complementary raised-cosine gains
//! hiding the wraps), and the TRANSPOSED signal is both what you
//! hear and what regenerates — so repeat n sits n intervals from
//! the program. +12 is the shimmer; negative spirals down.
//!
//! The seams are the instrument's artifacts, so they are DIALED,
//! not suffered:
//! * GRAIN is the window — the splice cadence. Small = tight
//!   flutter, large = washy. Its declared glide bends the heads
//!   while you turn it (the same honesty as the TIME knob).
//! * SPLICE is how hard the machine hides each seam: at every head
//!   takeover the incoming head may start anywhere within its
//!   reach, and a normalized cross-correlation search picks the
//!   start that PHASE-ALIGNS it with the head still playing — the
//!   "extra or missing cycles" of a raw splice disappear. At 0 the
//!   head lands wherever the sweep says (the dirty tape-splice
//!   warble, as a flavor). The search runs on the MID lane and both
//!   lanes share the offset — one machine, one splice — so seams
//!   never wander the stereo image.
//!
//! The head gains sum to ONE at every instant (Hann + its
//! complement), so the transposer adds no loop gain and the train
//! still dies at the dialed feedback (a law). Retunes land only
//! while the retuned head is SILENT — click-free by construction.
//!
//! SHIFT = 0 bypasses the transposer, which keeps the classic law
//! bit-identical (a law test). (This slot replaced TRACK, an octave
//! divider that couldn't track real program — see git history.)

use stave_dsp::{ms_decode, ms_encode, DelayLine, PerChannel};

use super::diffusion::Diffusion;
use super::echo::{Echo, MID, SIDE};

/// GRAIN's ceiling (params.rs dials 16..160 ms); the tap lines are
/// sized once for the worst case.
const MAX_GRAIN_MS: f32 = 160.0;

/// How far the splice search may move an incoming head, at full
/// SPLICE — a few cycles of anything musical.
const REACH_MS: f32 = 5.0;

/// Correlation window: ~1.3 ms at 48 kHz per candidate, stepped by
/// `STEP` samples across the reach. One search per head per window
/// (~every 32 ms at the default grain) — a bounded, allocation-free
/// burst.
const CORR_LEN: usize = 64;
const STEP: f32 = 3.0;

struct Tap {
    line: DelayLine,
}

pub(super) struct Shift {
    taps: PerChannel<Tap, 2>,
    /// The sweep phase, SHARED by lanes and heads (head 1 rides half
    /// a window behind): one machine, co-timed seams.
    phase: f32,
    /// Per-head splice offsets (samples), retuned at each takeover.
    off: [f32; 2],
    sr: f32,
}

impl Shift {
    pub(super) fn new() -> Self {
        Self {
            taps: PerChannel::new(|_| Tap {
                line: DelayLine::new(),
            }),
            phase: 0.0,
            off: [0.0; 2],
            sr: 48_000.0,
        }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let cap = ((MAX_GRAIN_MS + 2.0 * REACH_MS) * 0.001 * sr) as usize + 8;
        for tap in self.taps.iter_mut() {
            tap.line.prepare(cap);
            tap.line.reset();
        }
        self.phase = 0.0;
        self.off = [0.0; 2];
    }

    /// The splice: the incoming head starts its sweep at `d_start`;
    /// pick the offset within ±reach whose content best lines up
    /// with what the outgoing head (at `d_ref`) is playing right
    /// now. Normalized correlation, so a loud patch can't outvote a
    /// phase-aligned quiet one.
    ///
    /// The span and the reference are copied to the stack ONCE and
    /// the scan correlates in-buffer: candidate offsets step on an
    /// integer grid from the fractional base, so the buffered reads
    /// are the SAME positions (bit-identical scores) at ~1% of the
    /// cost — re-reading through the cubic interpolator per
    /// candidate measured ~200 µs per splice, +215 ns/frame
    /// amortized at a 16 ms grain.
    fn align(line: &DelayLine, d_start: f32, d_ref: f32, reach: f32) -> f32 {
        if reach < STEP {
            return 0.0;
        }
        // Reach is bounded by REACH_MS at the highest supported rate
        // we size for (192 kHz): 5 ms → 960 samples.
        const MAX_SPAN: usize = 2 * 960 + CORR_LEN + 2;
        let mut span = [0.0f32; MAX_SPAN];
        let reach_n = (reach / STEP).floor() * STEP; // grid-aligned
        let span_len = ((2.0 * reach_n) as usize + CORR_LEN).min(MAX_SPAN);
        let base = d_start - reach_n;
        for (j, slot) in span.iter_mut().enumerate().take(span_len) {
            *slot = line.read_cubic((base + j as f32).max(1.0));
        }
        let mut reference = [0.0f32; CORR_LEN];
        for (i, slot) in reference.iter_mut().enumerate() {
            *slot = line.read_cubic((d_ref + i as f32).max(1.0));
        }
        let mut best = (0.0f32, f32::MIN);
        let mut c = -reach_n;
        while c <= reach_n {
            let off = (c + reach_n) as usize;
            if off + CORR_LEN > span_len {
                break;
            }
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for i in 0..CORR_LEN {
                let cand = span[off + i];
                dot += cand * reference[i];
                energy += cand * cand;
            }
            let score = dot / energy.sqrt();
            if score > best.1 {
                best = (c, score);
            }
            c += STEP;
        }
        best.0
    }

    /// One stereo frame: classic reads, transposed wet, transposed
    /// regeneration. Composed from Echo's lane ports because the
    /// heard signal is NOT the raw tap — it's the transposer's
    /// output, and that is also what feeds back.
    #[inline]
    pub(super) fn frame(
        &mut self,
        echo: &mut Echo,
        diffusion: &mut Diffusion,
        cx: super::FrameCtx,
        semitones: f32,
        grain_ms: f32,
        splice: f32,
    ) -> (f32, f32) {
        let (m64, s64) = ms_encode(cx.l, cx.r);
        let (m_in, s_in) = (m64 as f32, s64 as f32);
        let wet_m = echo.read(MID, cx.base + cx.wobble * (1.0 - cx.spread));
        let wet_s = echo.read(SIDE, cx.base - cx.wobble * cx.spread);
        // SHIFT = 0 is a true bypass (bit-classic); the tap lines
        // stay fed so engaging the knob never starts cold.
        let bypass = semitones.abs() < 1e-3;
        let ratio = (semitones / 12.0).exp2();
        let w = grain_ms * 0.001 * self.sr;

        self.taps[MID].line.write(wet_m);
        self.taps[MID].line.advance();
        self.taps[SIDE].line.write(wet_s);
        self.taps[SIDE].line.advance();

        // Advance the shared sweep; a head is retuned exactly when
        // its gain touches zero (head 0 as the phase wraps, head 1
        // as it crosses the half) — silent, so the retune is free.
        let prev = self.phase;
        self.phase = (self.phase + (1.0 - ratio) / w).rem_euclid(1.0);
        if !bypass {
            let reach = splice.clamp(0.0, 1.0) * REACH_MS * 0.001 * self.sr;
            let wrapped = (self.phase - prev).abs() > 0.5;
            let crossed_half = (prev < 0.5) != (self.phase < 0.5);
            if wrapped {
                let d_start = self.phase * w;
                let d_ref = ((self.phase + 0.5) % 1.0) * w + self.off[1];
                self.off[0] = Self::align(&self.taps[MID].line, d_start, d_ref, reach);
            } else if crossed_half {
                let d_start = ((self.phase + 0.5) % 1.0) * w;
                let d_ref = self.phase * w + self.off[0];
                self.off[1] = Self::align(&self.taps[MID].line, d_start, d_ref, reach);
            }
        }

        let (out_m, out_s) = if bypass {
            (wet_m, wet_s)
        } else {
            // Raised-cosine takeovers: g0 + g1 = 1 at every instant
            // (the no-loop-gain rule), zero-derivative at the seams.
            let p0 = self.phase;
            let p1 = (p0 + 0.5) % 1.0;
            let g0 = 0.5 * (1.0 - (2.0 * core::f32::consts::PI * p0).cos());
            let g1 = 1.0 - g0;
            let d0 = (p0 * w + self.off[0]).max(1.0);
            let d1 = (p1 * w + self.off[1]).max(1.0);
            let read2 = |line: &DelayLine| g0 * line.read_cubic(d0) + g1 * line.read_cubic(d1);
            (read2(&self.taps[MID].line), read2(&self.taps[SIDE].line))
        };
        // The field rides the TRANSPOSED pair (what you hear IS what
        // regenerates — the same read-side rule as every algorithm).
        // BLEED re-enters the loop and is re-shifted next lap:
        // shimmer feeding reverb feeding shimmer. (The FrameCtx
        // refactor caught this pair being silently dropped — the
        // DIFFUSE knob was dead on this tab.)
        let ((hear_m, hear_s), (regen_m, regen_s)) =
            diffusion.couple(out_m, out_s, cx.diffuse, cx.bleed);
        echo.commit(MID, m_in + regen_m * cx.fb);
        echo.commit(SIDE, s_in + regen_s * cx.fb);
        ms_decode(hear_m as f64, hear_s as f64)
    }
}
