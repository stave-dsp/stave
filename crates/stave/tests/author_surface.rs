//! The author surface from the prelude alone: params of every kind, a
//! taps table, smoothers and steps, presets, a face, migrations, and
//! `export!` with class ids kept from an earlier build. If this needs an
//! import beyond `stave::prelude::*`, the API is wrong, not the test.

use stave::prelude::*;

param_enum! {
    /// The delay's character.
    pub const MODES: Mode = ["CLEAN", "TAPE", "BBD"];
}

#[derive(Params)]
pub struct EchoParams {
    pub time: FloatParam,
    pub mode: EnumParam<Mode>,
    pub hold: BoolParam,
    pub bypass: BoolParam,
}

impl Default for EchoParams {
    fn default() -> Self {
        EchoParams {
            time: FloatParam::new(ParamId(1), "Time", Range::log(10.0, 2000.0))
                .default(350.0)
                .unit(Unit::Ms)
                .smooth(Smooth::linear_ms(50.0)),
            mode: EnumParam::new(ParamId(2), "Mode", Mode(1)),
            hold: BoolParam::new(ParamId(3), "Hold", false),
            bypass: BoolParam::bypass(ParamId(4)),
        }
    }
}

#[derive(Taps)]
pub struct EchoTaps {
    pub level: Tap,
}

pub struct Echo {
    params: Arc<EchoParams>,
    taps: EchoTaps,
    time: Smoother,
    hold: Steps<bool>,
}

impl Plugin for Echo {
    const INFO: Info = Info {
        id: "com.example.echo",
        name: "Echo",
        vendor: "Example",
        features: &[Feature::AudioEffect, Feature::Delay],
        ..Info::EFFECT
    };
    const STATE_VERSION: u32 = 2;
    type Params = EchoParams;

    fn new(params: &Arc<EchoParams>, telemetry: &Telemetry) -> Self {
        Echo {
            params: params.clone(),
            taps: EchoTaps::new(telemetry),
            time: Smoother::new(&params.time),
            hold: Steps::new(&params.hold),
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.time.prepare(env);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut time = self.time.ramp(block, &self.params.time);
        for step in self.hold.changes(block, &self.params.hold) {
            let _ = (step.frame, step.value);
        }
        let _mode: Mode = self.params.mode.value();
        let mut peak = 0.0f32;
        block.lanes::<2>().run(|frame| {
            let _ms = time.next();
            for lane in 0..frame.lanes() {
                let x = frame.input(lane);
                peak = peak.max(x.abs());
                frame.write(lane, x);
            }
        });
        self.taps.level.set(f64::from(peak));
    }

    fn face(p: &EchoParams) -> Option<Face> {
        let theme = Theme::cream();
        Some(Face::new(
            theme,
            Panel::default().around(row(vec![
                Knob::new(p.time.bind()).node(),
                Toggle::new(p.mode.bind()).node(),
                Footswitch::new(p.hold.bind()).momentary().node(),
                Meter::db(EchoTaps::LEVEL).node(),
            ])),
        ))
    }

    fn presets(p: &EchoParams) -> Vec<Preset> {
        vec![Preset::from_pairs("Slapback", &[(p.time.id(), 90.0), (p.mode.id(), 1.0)])]
    }

    fn migrations() -> Migrations {
        Migrations::default().renamed(ParamId(9), ParamId(1))
    }
}

stave::export!(Echo, vst3_class_ids: {
    component: [0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700001],
    controller: [0x7C31A2E4, 0x88B14D02, 0xA5E0C9D3, 0x11700002],
});

#[test]
fn the_plugin_reads_its_params_in_their_natural_types() {
    let p = EchoParams::default();
    assert_eq!(p.time.value(), 350.0);
    assert_eq!(p.mode.value(), Mode(1));
    assert!(!p.hold.value());
    assert_eq!(EchoTaps::ALL, [EchoTaps::LEVEL]);
    assert!(Echo::face(&p).is_some());
    assert_eq!(Echo::presets(&p)[0].name, "Slapback");
}
