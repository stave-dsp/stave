//! Stave Tilt — a one-knob tilt EQ, and the shape every stave plugin
//! copies: params, processing and face, top to bottom, from one import.
//!
//! A complementary shelving pair per lane — the low shelf at −tilt dB,
//! the high shelf at +tilt dB — tips the spectrum around 650 Hz.
//! `tests/properties.rs` holds the laws only this plugin can state (its
//! first build had the knob backwards and passed every generic check).

use stave::prelude::*;

#[derive(Params)]
pub struct TiltParams {
    /// Negative is darker (lows up, highs down), positive brighter.
    pub tilt: FloatParam,
    pub output: FloatParam,
    pub bypass: BoolParam,
}

impl Default for TiltParams {
    fn default() -> Self {
        TiltParams {
            tilt: FloatParam::new(ParamId(1), "Tilt", Range::linear(-6.0, 6.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(30.0)),
            output: FloatParam::new(ParamId(2), "Output", Range::linear(-24.0, 24.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(5.0)),
            bypass: BoolParam::bypass(ParamId(3), "Bypass"),
        }
    }
}

/// Where the seesaw balances: low enough that "brighter" reads as air,
/// high enough that "darker" keeps a voice's body.
const PIVOT: Hz = hz(650.0);

/// One lane's shelving pair.
#[derive(Default)]
struct Shelves {
    low: Biquad,
    high: Biquad,
}

pub struct Tilt {
    params: Arc<TiltParams>,
    tilt: Smoother,
    output: Smoother,
    shelves: [Shelves; 2],
    /// The tilt the shelves are voiced for (NaN: not yet).
    voiced: f32,
    /// What the shelves cost a block: `shelves_ns_per_sample` and
    /// `shelves_load`, the stage the perf check names when over budget.
    cost: CostTaps,
}

impl Plugin for Tilt {
    const INFO: Info = Info {
        id: "com.ryanbahan.stave.tilt",
        name: "Stave Tilt",
        vendor: "Ryan Bahan",
        url: "https://github.com/stave-dsp/stave",
        features: &[Feature::AudioEffect, Feature::Eq, Feature::Stereo],
        ..Info::EFFECT
    };
    /// Two biquads a lane: at the worst knobs ~6 ns a sample a channel
    /// on Apple Silicon, ~14 on an x86_64 Linux build host. The budget
    /// leaves the slower machine room and fails the day a lane costs
    /// a few filters more.
    const CPU_BUDGET: Budget = Budget::per_channel(40.0);
    type Params = TiltParams;

    fn new(params: &Arc<TiltParams>, telemetry: &Telemetry) -> Self {
        Tilt {
            params: params.clone(),
            tilt: Smoother::new(&params.tilt),
            output: Smoother::new(&params.output),
            shelves: Default::default(),
            voiced: f32::NAN,
            cost: CostTaps::new(telemetry, cost_tap_ids!("shelves")),
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.tilt.prepare(env);
        self.output.prepare(env);
        for lane in &mut self.shelves {
            lane.low.prepare(env.sample_rate);
            lane.high.prepare(env.sample_rate);
        }
        self.voiced = f32::NAN;
        self.cost.prepare(env.sample_rate);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut tilt = self.tilt.ramp(block, &self.params.tilt);
        let mut output = self.output.ramp(block, &self.params.output);
        let (shelves, voiced) = (&mut self.shelves, &mut self.voiced);
        let frames = block.frames();
        self.cost.measure(frames, || block.lanes::<2>().run(|frame| {
            let t = tilt.next();
            if t != *voiced {
                *voiced = t;
                for lane in shelves.iter_mut() {
                    lane.low.set(FilterSpec::LowShelf { frequency: PIVOT, gain: db(-t) });
                    lane.high.set(FilterSpec::HighShelf { frequency: PIVOT, gain: db(t) });
                }
            }
            let gain: Gain = output.next_as();
            for (lane, s) in shelves.iter_mut().enumerate().take(frame.lanes()) {
                let x = frame.input(lane);
                // Flat is bit-exact: no recursion at tilt 0.
                let y = if t == 0.0 { x } else { s.high.tick(s.low.tick(x)) };
                frame.write(lane, y * gain);
            }
        }));
    }

    fn face(p: &TiltParams) -> Option<Face> {
        let theme = Theme::graphite();
        let header = row(vec![
            Title::new("TILT").tagline("tilt eq · ±6 dB around 650 Hz").node(),
            Footswitch::bypass(p.bypass.bind()).compact().node(),
        ])
        .gap(theme.space(4))
        .align(Align::Start);
        let tone = Plate::titled("TONE").around(
            row(vec![
                Knob::new(p.tilt.bind()).huge().centered().node(),
                Knob::new(p.output.bind()).centered().node(),
            ])
            .gap(theme.space(4))
            .align(Align::Center),
        );
        let body = column(vec![header, tone]).gap(theme.space(3)).align(Align::Stretch);
        Some(Face::new(theme, Panel::default().around(body)))
    }
}

stave::export!(Tilt);
