//! The 3-band Linkwitz-Riley crossover splitter — one concept, two
//! instances: the MAIN split feeds the audio path, and the SIDECHAIN
//! split feeds detection from the external aux at the same
//! cutoffs with its own state. LR4 split-and-sum is unity-gain
//! magnitude-wise (allpass phase at the crossovers).

use stave_dsp::LinkwitzRiley4;

pub(crate) struct Splitter {
    /// LP at low_xover → low band; HP at low_xover → "above low";
    /// then HP/LP at high_xover carve high/mid out of it. Per lane.
    lp_lo: [LinkwitzRiley4; 2],
    hp_lo: [LinkwitzRiley4; 2],
    lp_hi: [LinkwitzRiley4; 2],
    hp_hi: [LinkwitzRiley4; 2],
}

impl Splitter {
    pub(crate) fn new() -> Self {
        Self {
            lp_lo: [LinkwitzRiley4::new(); 2],
            hp_lo: [LinkwitzRiley4::new(); 2],
            lp_hi: [LinkwitzRiley4::new(); 2],
            hp_hi: [LinkwitzRiley4::new(); 2],
        }
    }

    pub(crate) fn reset(&mut self) {
        for f in self.lp_lo.iter_mut() {
            f.reset();
        }
        for f in self.hp_lo.iter_mut() {
            f.reset();
        }
        for f in self.lp_hi.iter_mut() {
            f.reset();
        }
        for f in self.hp_hi.iter_mut() {
            f.reset();
        }
    }

    pub(crate) fn set_low(&mut self, hz: f32, sr: f32) {
        for f in self.lp_lo.iter_mut() {
            f.set_lowpass(hz, sr);
        }
        for f in self.hp_lo.iter_mut() {
            f.set_highpass(hz, sr);
        }
    }

    pub(crate) fn set_high(&mut self, hz: f32, sr: f32) {
        for f in self.lp_hi.iter_mut() {
            f.set_lowpass(hz, sr);
        }
        for f in self.hp_hi.iter_mut() {
            f.set_highpass(hz, sr);
        }
    }

    /// Split one frame into [(lo_l, lo_r), (mid_l, mid_r), (hi_l, hi_r)].
    /// `stereo = false` ticks only lane 0 and mirrors it — the mono
    /// path's lane-1 states never advance (bit-compat contract).
    #[inline]
    pub(crate) fn split(&mut self, l: f32, r: f32, stereo: bool) -> [(f32, f32); 3] {
        let lo_l = self.lp_lo[0].process(l);
        let lo_r = if stereo {
            self.lp_lo[1].process(r)
        } else {
            lo_l
        };
        let mid_pre_l = self.hp_lo[0].process(l);
        let mid_pre_r = if stereo {
            self.hp_lo[1].process(r)
        } else {
            mid_pre_l
        };
        let hi_l = self.hp_hi[0].process(mid_pre_l);
        let hi_r = if stereo {
            self.hp_hi[1].process(mid_pre_r)
        } else {
            hi_l
        };
        let mid_l = self.lp_hi[0].process(mid_pre_l);
        let mid_r = if stereo {
            self.lp_hi[1].process(mid_pre_r)
        } else {
            mid_l
        };
        [(lo_l, lo_r), (mid_l, mid_r), (hi_l, hi_r)]
    }
}
