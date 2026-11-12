//! Echo — the delay lines and their feedback law, in MID/SIDE
//! lanes (the stereo image's anatomy, not left and right): SPREAD
//! allocates the wobble between them, so the phantom center's
//! stability is a dialed quantity (the image laws pin both ends).
//! The dry never enters here — it stays in L/R at the mix, which
//! is what keeps mix = 0 bit-identical dry.
//!
//! Echo is the SUBSTRATE the four algorithms share: the classic law
//! lives here whole (`stereo_frame`); CIRCUIT shapes the regeneration
//! through the same law (`stereo_frame_shaped`); ORBIT composes its
//! own law from the lane ports (`read`/`commit`).

use stave_dsp::{ms_decode, ms_encode, DelayLine, PerChannel};

/// One lane's state: its delay line.
struct Lane {
    line: DelayLine,
}
pub(super) const MID: usize = 0;
pub(super) const SIDE: usize = 1;

pub(super) struct Echo {
    lanes: PerChannel<Lane, 2>,
}

impl Echo {
    pub(super) fn new() -> Self {
        Self {
            lanes: PerChannel::new(|_| Lane {
                line: DelayLine::new(),
            }),
        }
    }

    pub(super) fn prepare(&mut self, capacity: usize) {
        for lane in self.lanes.iter_mut() {
            lane.line.prepare(capacity);
            lane.line.reset();
        }
    }

    /// The classic law with its two shaping seams: `wet(m, s)`
    /// takes the READ pair and returns TWO pairs — (heard, regen) —
    /// because the field topology lets the ear and the loop get
    /// different couplings (DIFFUSE vs BLEED); `head(lane, x)`
    /// touches everything WRITTEN to the line — program + fed-back
    /// tail together, once per lap (CIRCUIT's cell, the honest tape
    /// topology). Identity seams ARE `stereo_frame` — the same code
    /// path, so heat 0 / field-off stay bit-identical to the
    /// classic law.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stereo_frame_shaped(
        &mut self,
        l: f32,
        r: f32,
        base: f32,
        wobble: f32,
        spread: f32,
        fb: f32,
        mut head: impl FnMut(usize, f32) -> f32,
        wet: impl FnOnce(f32, f32) -> ((f32, f32), (f32, f32)),
    ) -> (f32, f32) {
        let (m64, s64) = ms_encode(l, r);
        let (m_in, s_in) = (m64 as f32, s64 as f32);
        let raw_m = self.lanes[MID]
            .line
            .read_cubic(base + wobble * (1.0 - spread));
        let raw_s = self.lanes[SIDE].line.read_cubic(base - wobble * spread);
        let ((hear_m, hear_s), (regen_m, regen_s)) = wet(raw_m, raw_s);
        let mid = &mut self.lanes[MID].line;
        mid.write(head(MID, m_in + regen_m * fb));
        mid.advance();
        let side = &mut self.lanes[SIDE].line;
        side.write(head(SIDE, s_in + regen_s * fb));
        side.advance();
        ms_decode(hear_m as f64, hear_s as f64)
    }

    /// One mono frame: a single line, the full wobble (spread needs
    /// two ears to mean anything).
    #[inline]
    pub(super) fn mono_frame(&mut self, dry: f32, base: f32, wobble: f32, fb: f32) -> f32 {
        let line = &mut self.lanes[MID].line;
        let wet = line.read_cubic(base + wobble);
        line.write(dry + wet * fb);
        line.advance();
        wet
    }

    /// Lane port: the wet sample at `pos` (for laws Echo's frame
    /// functions can't express — ORBIT reads both lanes before it
    /// decides what either writes back).
    #[inline]
    pub(super) fn read(&mut self, lane: usize, pos: f32) -> f32 {
        self.lanes[lane].line.read_cubic(pos)
    }

    /// Lane port: write this frame's sample and advance the lane.
    #[inline]
    pub(super) fn commit(&mut self, lane: usize, value: f32) {
        let line = &mut self.lanes[lane].line;
        line.write(value);
        line.advance();
    }
}
