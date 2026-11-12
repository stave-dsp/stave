//! The multiband engine: an LR4 split into three [`Band`]s, each a
//! compressor with its own detector, saturation and parallel blend,
//! summed back. An optional external sidechain is split at the same
//! crossovers to drive detection.

extern crate alloc;
use alloc::sync::Arc;

use stave_core::params::ContinuousControl;
use stave_core::{DebugBus, Environment, ProcessCtx};

use crate::params::{MbParams, BANDS, GR_PROBES};

mod band;
pub(crate) mod dynamics;
mod splitter;

use band::{Band, BandControls};
use splitter::Splitter;

pub struct MultibandEngine {
    params: Arc<MbParams>,
    sample_rate: f32,
    split: Splitter,
    /// The external sidechain's own split: same cutoffs, own state.
    sidechain_split: Splitter,
    sidechain_was_live: bool,
    crossovers: [ContinuousControl; 2],
    crossover_hz: [f32; 2],
    controls: [BandControls; BANDS],
    bands: [Band; BANDS],
    bus: Option<Arc<DebugBus>>,
}

impl MultibandEngine {
    fn set_crossovers(&mut self, low_hz: f32, high_hz: f32) {
        set_crossovers(
            [&mut self.split, &mut self.sidechain_split],
            &mut self.crossover_hz,
            [low_hz, high_hz],
            self.sample_rate,
        );
    }
}

/// Retune both splits wherever a cutoff moved.
fn set_crossovers(
    mut splits: [&mut Splitter; 2],
    current: &mut [f32; 2],
    hz: [f32; 2],
    sample_rate: f32,
) {
    if hz[0] != current[0] {
        for split in splits.iter_mut() {
            split.set_low(hz[0], sample_rate);
        }
    }
    if hz[1] != current[1] {
        for split in splits.iter_mut() {
            split.set_high(hz[1], sample_rate);
        }
    }
    *current = hz;
}

impl stave_core::Processor for MultibandEngine {
    type Params = MbParams;

    fn new(params: Arc<MbParams>) -> Self {
        Self {
            sample_rate: 48_000.0,
            split: Splitter::new(),
            sidechain_split: Splitter::new(),
            sidechain_was_live: false,
            crossovers: [
                ContinuousControl::new(&params.xover_low),
                ContinuousControl::new(&params.xover_high),
            ],
            crossover_hz: [f32::NAN; 2],
            controls: core::array::from_fn(|i| BandControls::new(&params.bands[i])),
            bands: GR_PROBES.map(Band::new),
            bus: None,
            params,
        }
    }

    fn activate(&mut self, env: Environment) {
        let p = self.params.clone();
        let rate = f64::from(env.sample_rate);
        self.sample_rate = env.sample_rate;
        self.split.reset();
        self.sidechain_split.reset();
        self.sidechain_was_live = false;
        self.crossovers[0].prepare(rate, &p.xover_low);
        self.crossovers[1].prepare(rate, &p.xover_high);
        self.crossover_hz = [f32::NAN; 2];
        self.set_crossovers(p.xover_low.value(), p.xover_high.value());
        for ((controls, band), bp) in self.controls.iter_mut().zip(&mut self.bands).zip(&p.bands) {
            controls.prepare(rate, bp);
            band.prepare(env.sample_rate, bp);
        }
    }

    /// The saturators' 4× FIR stack: constant across bands and settings,
    /// and the plug-in's only latency.
    fn latency_samples(&self) -> u32 {
        self.bands[0].latency_samples()
    }

    fn attach_telemetry(&mut self, bus: Arc<DebugBus>) {
        self.bus = Some(bus);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        for (band, bp) in self.bands.iter_mut().zip(&p.bands) {
            band.configure(bp.style.get(), bp.saturation.get());
        }
        let any_solo = p.bands.iter().any(|b| b.solo.value());
        let audible: [bool; BANDS] = core::array::from_fn(|i| {
            !p.bands[i].mute.value() && (p.bands[i].solo.value() || !any_solo)
        });

        let sidechain = ctx
            .aux
            .first()
            .filter(|aux| p.external_sidechain.value() && aux.channel_count() > 0);
        if sidechain.is_some() && !self.sidechain_was_live {
            self.sidechain_split.reset();
        }
        self.sidechain_was_live = sidechain.is_some();
        let sidechain = sidechain.map(|aux| {
            let left = aux.channel_ref(0);
            let right = if aux.channel_count() > 1 {
                aux.channel_ref(1)
            } else {
                left
            };
            (left, right)
        });

        let Self {
            sample_rate,
            split,
            sidechain_split,
            crossovers,
            crossover_hz,
            controls,
            bands,
            ..
        } = self;
        let [low, high] = crossovers;
        let mut low = low.begin_block(ctx.params, &p.xover_low);
        let mut high = high.begin_block(ctx.params, &p.xover_high);
        let [c0, c1, c2] = controls;
        let mut ramps = [
            c0.begin_block(ctx.params, &p.bands[0]),
            c1.begin_block(ctx.params, &p.bands[1]),
            c2.begin_block(ctx.params, &p.bands[2]),
        ];
        let lanes = ctx.lanes::<2>();
        let stereo = match lanes.active() {
            0 => return,
            active => active > 1,
        };
        lanes.run(|fr| {
            set_crossovers(
                [&mut *split, &mut *sidechain_split],
                crossover_hz,
                [low.next(), high.next()],
                *sample_rate,
            );
            let left = fr.input(0);
            let right = if stereo { fr.input(1) } else { left };
            let signal = split.split(left, right, stereo);
            let detect = match sidechain {
                Some((l, r)) => sidechain_split.split(l[fr.index()], r[fr.index()], true),
                None => signal,
            };
            let mut out = (0.0, 0.0);
            for (i, band) in bands.iter_mut().enumerate() {
                let settings = ramps[i].next();
                let (l, r) = band.process(&settings, detect[i], signal[i], stereo);
                if audible[i] {
                    out.0 += l;
                    out.1 += r;
                }
            }
            fr.write(0, out.0);
            if stereo {
                fr.write(1, out.1);
            }
        });
        if let Some(bus) = &self.bus {
            for band in &self.bands {
                band.publish(bus);
            }
        }
    }
}
