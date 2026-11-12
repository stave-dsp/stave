//! The tilt engine: a complementary shelving pair per lane, the low
//! shelf at −tilt dB and the high shelf at +tilt dB, pivoting the
//! spectrum around `PIVOT_HZ`. Tilt glides per sample; the shelves
//! re-voice on each sample it moves.

extern crate alloc;
use alloc::sync::Arc;
use stave_dsp::FilterSpec;

use stave_core::params::ContinuousControl;
use stave_core::{Environment, ProcessCtx, Tail};
use stave_dsp::{db_to_amp, Biquad, PerChannel};

use crate::params::TiltParams;

/// Where the seesaw balances. The classic tilt pivot sits in the low
/// mids — high enough that "darker" keeps a voice's body, low enough
/// that "brighter" reads as air.
const PIVOT_HZ: f32 = 650.0;

/// One lane of the seesaw.
struct Shelves {
    low: Biquad,
    high: Biquad,
}

pub struct TiltEngine {
    params: Arc<TiltParams>,
    sr: f32,
    lanes: PerChannel<Shelves, 2>,
    /// The tilt the shelves are voiced for; NaN until the first sample.
    /// Tilt 0 bypasses them, so a flat setting is bit-exact.
    voiced: f32,
    tilt: ContinuousControl,
    output: ContinuousControl,
}

impl stave_core::Processor for TiltEngine {
    type Params = TiltParams;

    fn new(params: Arc<TiltParams>) -> Self {
        Self {
            output: ContinuousControl::new(&params.output),
            tilt: ContinuousControl::new(&params.tilt),
            params,
            sr: 48_000.0,
            lanes: PerChannel::new(|_| Shelves {
                low: Biquad::new(),
                high: Biquad::new(),
            }),
            voiced: f32::NAN,
        }
    }

    fn activate(&mut self, env: Environment) {
        self.sr = env.sample_rate;
        self.voiced = f32::NAN;
        self.tilt
            .prepare(f64::from(env.sample_rate), &self.params.tilt);
        self.output
            .prepare(f64::from(env.sample_rate), &self.params.output);
    }

    fn process(&mut self, ctx: &mut ProcessCtx<'_>) {
        let p = self.params.clone();
        let mut tilt = self.tilt.begin_block(ctx.params, &p.tilt);
        let mut out_ramp = self.output.begin_block(ctx.params, &p.output);
        let sr = self.sr;
        let voiced = &mut self.voiced;
        let lanes_state = &mut self.lanes;
        let lanes = ctx.lanes::<2>();
        let active = lanes.active().min(2);
        lanes.run(|fr| {
            let t = tilt.next();
            if t != *voiced {
                *voiced = t;
                for lane in lanes_state.iter_mut() {
                    lane.low.set(
                        FilterSpec::LowShelf {
                            hz: PIVOT_HZ,
                            db: -t,
                        },
                        sr,
                    );
                    lane.high.set(
                        FilterSpec::HighShelf {
                            hz: PIVOT_HZ,
                            db: t,
                        },
                        sr,
                    );
                }
            }
            let g = db_to_amp(out_ramp.next());
            for lane in 0..active {
                let x = fr.input(lane);
                let y = if t != 0.0 {
                    let s = &mut lanes_state[lane];
                    s.high.process(s.low.process(x))
                } else {
                    x
                };
                fr.write(lane, y * g);
            }
        });
    }

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail(&self) -> Tail {
        Tail::None
    }
}
