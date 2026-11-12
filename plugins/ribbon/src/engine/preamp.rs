//! `Preamp` — the tube stage in front of the record head: a 12AX7
//! (the shared Koren triode, ADAA'd) with cathode self-bias, driven
//! from barely-there warmth to grid-conduction grind by DRIVE, then a
//! coupling capacitor, at unity small-signal gain. At DRIVE 0 the stage is bypassed outright, so a
//! clean machine stays exactly clean.

use stave_dsp::{Bias, Coupler, Koren, Stage, Triode};

/// Grid volts per unit of input: clean at 0, saturated past ~30%.
const CLEAN_VOLTS: f64 = 0.4;
const SPAN_VOLTS: f64 = 60.0;

pub(super) struct Preamp {
    tubes: [Triode; 2],
    couplers: [Coupler; 2],
    drive: f32,
}

impl Preamp {
    pub(super) fn new() -> Self {
        // The default CALIBRATED level policy: small-signal gain held at
        // unity, so DRIVE moves the tube toward its knee (more grit)
        // without making the record level louder.
        let tube = || Triode::with_tube(Koren::T12AX7);
        Self { tubes: [tube(), tube()], couplers: [Coupler::new(12.0), Coupler::new(12.0)], drive: 0.0 }
    }

    pub(super) fn prepare(&mut self, sr: f32) {
        for (t, c) in self.tubes.iter_mut().zip(self.couplers.iter_mut()) {
            t.prepare(sr);
            let bypass_farads = 22e-3 / t.cathode_ohms();
            t.set_bias(Bias::Cathode { bypass_farads });
            c.prepare(sr);
        }
    }

    /// Re-drive for a block.
    pub(super) fn set_drive(&mut self, drive: f32) {
        self.drive = drive;
        let volts = CLEAN_VOLTS + SPAN_VOLTS * (10f64.powf(1.6 * f64::from(drive)) - 1.0) / (10f64.powf(1.6) - 1.0);
        for t in &mut self.tubes {
            t.set_grid_drive(volts);
        }
    }

    #[inline]
    pub(super) fn tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        if self.drive <= 0.0 {
            return x;
        }
        let mut y = [0.0; 2];
        for ch in 0..2 {
            let warm = self.couplers[ch].tick(self.tubes[ch].process_sample(x[ch]));
            // Blend in over the knob's first few percent: no step.
            y[ch] = x[ch] + (warm - x[ch]) * (self.drive * 10.0).min(1.0);
        }
        y
    }
}
