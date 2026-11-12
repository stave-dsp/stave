//! One band: its controls, its detector, its gain computer and
//! saturation, and the parallel blend against the dry band.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::{DebugBus, Tap};
use stave_dsp::db_to_amp;

use super::dynamics::band_reduction_db;
use crate::detector::{Detector, DetectorSettings};
use crate::params::BandParams;
use crate::saturation::{BandSaturator, SaturationCharacter, StereoMakeupLink};
use crate::style::Style;

stave_core::continuous_controls! {
    pub(super) struct BandControls<BandParams> => BandRamps, BandSettings {
        threshold,
        ratio,
        knee,
        makeup,
        attack,
        release,
        transient,
        rms,
        sidechain_hpf,
        drive,
        saturation_mix,
        mix,
    }
}

pub(super) struct Band {
    style: Style,
    detector: Detector,
    /// One per lane, sharing the tube makeup across the stereo pair.
    saturators: [BandSaturator; 2],
    drive_db: f32,
    reduction_db: f32,
    reduction_tap: Tap,
}

impl Band {
    pub fn new(reduction_tap: &'static str) -> Self {
        let link = Arc::new(StereoMakeupLink::default());
        let mut saturators = [BandSaturator::new(), BandSaturator::new()];
        for (lane, saturator) in saturators.iter_mut().enumerate() {
            saturator.set_stereo_link(link.clone(), lane);
        }
        Self {
            style: Style::Vca,
            detector: Detector::new(),
            saturators,
            drive_db: 0.0,
            reduction_db: 0.0,
            reduction_tap: Tap::new(reduction_tap),
        }
    }

    pub fn prepare(&mut self, sample_rate: f32, p: &BandParams) {
        self.style = p.style.get();
        self.detector.prepare(sample_rate);
        self.drive_db = p.drive.value();
        for saturator in &mut self.saturators {
            saturator.set_drive_db(self.drive_db);
            saturator.set_mix(p.saturation_mix.value());
            saturator.set_type(p.saturation.get());
            saturator.prepare(sample_rate);
        }
        self.reduction_db = 0.0;
    }

    pub fn latency_samples(&self) -> u32 {
        self.saturators[0].latency_samples()
    }

    pub fn configure(&mut self, style: Style, saturation: SaturationCharacter) {
        self.style = style;
        for saturator in &mut self.saturators {
            saturator.set_type(saturation);
        }
    }

    /// One frame: detect from `detect`, compress and saturate `band`,
    /// blend against it by `s.mix`.
    pub fn process(
        &mut self,
        s: &BandSettings,
        detect: (f32, f32),
        band: (f32, f32),
        stereo: bool,
    ) -> (f32, f32) {
        let character = self.style.character();
        let settings = DetectorSettings {
            threshold_db: s.threshold,
            attack_ms: s.attack,
            release_ms: s.release,
            transient: s.transient,
            rms: s.rms,
            hpf_hz: s.sidechain_hpf,
        };
        let envelope = self
            .detector
            .tick(0.5 * (detect.0 + detect.1), &settings, &character);
        let knee_db = if s.knee > 0.0 {
            s.knee
        } else {
            character.knee_db
        } * character.knee_stretch(envelope, s.threshold);
        self.reduction_db = band_reduction_db(envelope, s.threshold, s.ratio, knee_db);
        let gain = db_to_amp(s.makeup - self.reduction_db);

        if s.drive != self.drive_db {
            self.drive_db = s.drive;
            for saturator in &mut self.saturators {
                saturator.set_drive_db(s.drive);
            }
        }
        let lanes = if stereo { 2 } else { 1 };
        let mut out = [band.0, band.1];
        for (lane, x) in out.iter_mut().enumerate().take(lanes) {
            let saturator = &mut self.saturators[lane];
            saturator.set_mix(s.saturation_mix);
            let wet = saturator.process(*x * gain);
            *x += (wet - *x) * s.mix;
        }
        (out[0], if stereo { out[1] } else { out[0] })
    }

    pub fn publish(&self, bus: &DebugBus) {
        self.reduction_tap.set(bus, -f64::from(self.reduction_db));
    }
}
