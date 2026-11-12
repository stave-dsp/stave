//! THE MID-FIELD — the missing region between the room's discrete
//! answer and the tank's statistics (field: "cathedral sounds like
//! a multitap delay" — at size 2.7 the order-2 images spread over
//! ~300 ms with nothing between them; measured NED t90 0.105 s and
//! flutter 0.5 at cathedral scale vs 0.060/0.29 at size 1). A
//! per-ear cascade of two long allpasses whose delays are the
//! room's own MEAN FREE PATH (4V/S — the scene's statistics, so
//! big rooms densify slower and small rooms faster, both honest):
//! every discrete arrival grows an exponentially-densifying tail
//! at the spacing real higher-order bounces would have. The late
//! tank feeds FROM this densified field, so its bloom arrives
//! smoothly instead of as a second lump.

use stave_dsp::DelayLine;

const STAGES: usize = 2;
/// Stage ratios: mean free path and its golden fraction (mutually
/// incommensurate — the two trains interleave, never align).
const RATIOS: [f32; STAGES] = [1.0, 0.618];
/// The allpass coefficient is FIXED — an allpass redistributes
/// time at full strength at ANY g (gating by g degenerated to a
/// pure 21 ms delay at the gate's bottom, measured: the whole wet
/// shifted late and the ears law read bit-identical mono LF). The
/// GATE is a parallel BLEND instead: specular path always
/// instant, scattered path mixed in — the mix IS the scattering
/// coefficient, which is the honest physics of a partly-rough
/// wall.
const G: f32 = 0.45;
const MAX_SLEW: f32 = 0.02;

struct Stage {
    delay: DelayLine,
    len: f32,
    step: f32,
    drift_ph: f32,
    drift_rate: f32,
}

pub(super) struct Midfield {
    stages: [Stage; STAGES],
    mix: f32,
    /// Per-sample blend glide (worklist 7): the mix is the size
    /// gate's hand on the wet — stepped per block it clicked at
    /// every block boundary of a size sweep (found by the
    /// lookahead's 144-sample signature in the spike census).
    mix_step: f32,
    /// Inter-ear detune (the ears' clouds must differ late, and
    /// these delays are long enough to decorrelate honestly).
    detune: f32,
    sr: f32,
    cold: bool,
}

impl Midfield {
    pub fn new(detune: f32) -> Self {
        Self {
            stages: core::array::from_fn(|i| Stage {
                delay: DelayLine::new(),
                len: 100.0,
                step: 0.0,
                drift_ph: (i as f32) * 2.1 + detune * 37.0,
                drift_rate: 0.05 + 0.04 * (i as f32),
            }),
            detune,
            mix: 0.0,
            mix_step: 0.0,
            sr: 48_000.0,
            cold: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        // Mean free path at size 3 ≈ 37 ms; headroom for detune.
        let cap = (0.045 * sr) as usize + 16;
        for st in self.stages.iter_mut() {
            st.delay.prepare(cap);
        }
        self.reset();
        self.cold = true;
    }

    pub fn reset(&mut self) {
        let detune = self.detune;
        for (i, st) in self.stages.iter_mut().enumerate() {
            st.delay.reset();
            st.step = 0.0;
            // Known place after transport stop (the dynamo lesson).
            st.drift_ph = (i as f32) * 2.1 + detune * 37.0;
        }
    }

    /// Aim the stages at the room's own bounce interval, each
    /// DRIFTING slowly (±3%, hash-rated) — real scattering never
    /// repeats; static recursion at mfp spacing read as flutter
    /// 0.7 in the very band the stage was built to clean. The
    /// SIZE GATE: this stage exists to fill the order-2 truncation
    /// gap, which only opens at large sizes (at size 1 the model
    /// is already dense — engaging there ADDED flutter 0.29→0.60
    /// and dragged C80, measured).
    pub fn tune(&mut self, mfp_s: f32, diffuse: f32, size: f32, frames: usize) {
        let gate = ((size - 1.0) / 0.8).clamp(0.0, 1.0);
        let mix_t = (diffuse / 0.7).min(1.2) * gate * 0.5;
        let n = frames.max(1) as f32;
        if self.cold {
            self.mix = mix_t;
            self.mix_step = 0.0;
        } else {
            self.mix_step = (mix_t - self.mix) / n;
        }
        let block_s = n / self.sr;
        for (i, st) in self.stages.iter_mut().enumerate() {
            st.drift_ph = (st.drift_ph + core::f32::consts::TAU * st.drift_rate * block_s)
                % core::f32::consts::TAU;
            // ±1.2%: 3% at cathedral scale read as slow chorus,
            // width-exposed through the inter-ear detune (field).
            let drift = 1.0 + 0.012 * st.drift_ph.sin();
            let target = (mfp_s * RATIOS[i] * self.detune * drift * self.sr).max(8.0);
            if self.cold {
                st.len = target;
                st.step = 0.0;
            } else {
                let slew = MAX_SLEW * n;
                st.step = (target - st.len).clamp(-slew, slew) / n;
            }
        }
        self.cold = false;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let mut v = x;
        for st in self.stages.iter_mut() {
            let d = st.delay.read(st.len);
            st.len += st.step;
            let y = d - G * v;
            st.delay.write(v + G * y);
            st.delay.advance();
            v = y;
        }
        let y = x + self.mix * (v - x);
        self.mix += self.mix_step;
        y
    }
}
