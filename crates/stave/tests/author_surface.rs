//! The author surface from the prelude alone: params of every kind, a
//! taps table, smoothers (one and a group) and steps, a sidechain read
//! inside the lane loop, a generator, a closure-driven oversampled
//! region, presets, a face that sets its own type, migrations, and
//! `export!` with class ids kept from an earlier build (read back with
//! `stave::identity`). If this needs an
//! import beyond `stave::prelude::*`, the API is wrong, not the test.

use stave::prelude::*;

param_enum! {
    /// The delay's character.
    pub const MODES: Mode = ["CLEAN", "TAPE", "BBD"];
}

#[derive(Params)]
pub struct EchoParams {
    pub time: FloatParam,
    pub drive: FloatParam,
    pub mix: FloatParam,
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
            drive: FloatParam::new(ParamId(5), "Drive", Range::linear(1.0, 8.0)).smooth(Smooth::ms(20.0)),
            mix: FloatParam::new(ParamId(6), "Mix", Range::linear(0.0, 1.0)).smooth(Smooth::ms(20.0)),
            mode: EnumParam::new(ParamId(2), "Mode", Mode(1)),
            hold: BoolParam::new(ParamId(3), "Hold", false),
            bypass: BoolParam::bypass(ParamId(4), "Bypass"),
        }
    }
}

#[derive(Taps)]
pub struct EchoTaps {
    pub level: Tap,
}

/// One frame of the tone knobs.
pub struct Tone {
    pub drive: f32,
    pub mix: f32,
}

#[derive(Smoothers)]
#[smoothers(params = EchoParams, frame = Tone)]
pub struct ToneSmoothers {
    drive: Smoother,
    mix: Smoother,
}

pub struct Echo {
    params: Arc<EchoParams>,
    taps: EchoTaps,
    time: Smoother,
    tone: ToneSmoothers,
    hold: Steps<bool>,
    wobble: Lfo,
    shaper: Oversampled2x<()>,
}

impl Plugin for Echo {
    const INFO: Info = Info {
        id: "com.example.echo",
        name: "Echo",
        vendor: "Example",
        features: &[Feature::AudioEffect, Feature::Delay],
        ..Info::EFFECT
    };
    const LAYOUTS: &'static [Layout] = &[
        Layout { aux: &[Aux { name: "key", channels: Channels::Mono }], ..Layout::STEREO },
        Layout::STEREO,
    ];
    const STATE_VERSION: u32 = 2;
    type Params = EchoParams;

    fn new(params: &Arc<EchoParams>, telemetry: &Telemetry) -> Self {
        Echo {
            params: params.clone(),
            taps: EchoTaps::new(telemetry),
            time: Smoother::new(&params.time),
            tone: ToneSmoothers::new(params),
            hold: Steps::new(&params.hold),
            wobble: Lfo::new(hz(0.5)),
            shaper: Oversampled2x::front(()),
        }
    }

    fn prepare(&mut self, env: &Environment) {
        self.time.prepare(env);
        self.tone.prepare(env);
        self.wobble.prepare(env.sample_rate);
        self.shaper.prepare(env.sample_rate);
    }

    fn process(&mut self, block: &mut Block<'_>) {
        let mut time = self.time.ramp(block, &self.params.time);
        let mut tone = self.tone.ramp(block, &self.params);
        for step in self.hold.changes(block, &self.params.hold) {
            let _ = (step.frame, step.value);
        }
        let _mode: Mode = self.params.mode.value();
        let mut key = block.aux(0).filter(|aux| aux.channel_count() > 0).map(|aux| aux.channel(0).iter());
        let (wobble, shaper) = (&mut self.wobble, &mut self.shaper);
        let mut peak = 0.0f32;
        block.lanes::<2>().run(|frame| {
            let _delay: Seconds = time.next_as();
            let Tone { drive, mix } = tone.next();
            let duck = key.as_mut().and_then(Iterator::next).map_or(1.0, |k| 1.0 - k.abs().min(1.0));
            let _depth = wobble.next();
            for lane in 0..frame.lanes() {
                let x = frame.input(lane);
                let wet = shaper.tick_with(x, |(), s| (s * drive).tanh());
                peak = peak.max(x.abs());
                frame.write(lane, (x + mix * (wet - x)) * duck);
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
                Display::new(120.0, 40.0, caption).node(),
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

/// Draw code sets its own type: family, weight, alignment, a gradient.
fn caption(f: &Frame<'_>, r: Rect, s: &mut Scene) {
    let at: Pt = pt(r.x + 4.0, r.y + r.h / 2.0);
    let font = Font::new(Family::Serif, 600).tracking(0.05).align(TextAlign::Left);
    s.text_in(at, "ECHO", 12.0, f.theme.palette.ink, font);
    let glow = Brush::Linear { from: pt(r.x, r.y), to: pt(r.x, r.y + r.h), stops: vec![(0.0, f.theme.palette.phosphor), (1.0, f.theme.palette.screen)] };
    s.fill(Path::rect(pt(r.x, r.y + r.h - 4.0), r.w, 4.0), glow);
    s.stroke(Path::new().move_to(pt(r.x, r.y)).line_to(pt(r.x + r.w, r.y)), Stroke { cap: Cap::Round, ..Stroke::new(f.theme.palette.ink_soft, 1.0) });
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

#[test]
fn the_identity_hosts_key_on_is_readable() {
    let id = stave::identity::<Echo>();
    assert_eq!(id.clap, "com.example.echo");
    assert_eq!(id.vst3_component[..4], [0x7C, 0x31, 0xA2, 0xE4]);
    assert_eq!(id.vst3_component[12..], [0x11, 0x70, 0x00, 0x01]);
    assert_eq!(id.vst3_controller[12..], [0x11, 0x70, 0x00, 0x02]);
}
