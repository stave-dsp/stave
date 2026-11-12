//! Integration test for `#[derive(Params)]`: the derive generates the
//! same table shape as the hand impl in `params::types::tests`, and
//! `#[param(skip)]` excludes non-param fields.

use stave_core::params::{
    BoolParam, EnumParam, FloatParam, ParamEnum, ParamId, Params, Range, Smooth, Unit,
};
use stave_core::Params;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Voice {
    Flat,
    Scoop,
    Bump,
}

impl ParamEnum for Voice {
    const COUNT: u32 = 3;
    fn from_index(i: u32) -> Self {
        [Voice::Flat, Voice::Scoop, Voice::Bump][i as usize % 3]
    }
    fn to_index(self) -> u32 {
        self as u32
    }
    fn name(i: u32) -> &'static str {
        ["FLAT", "SCOOP", "BUMP"][i as usize % 3]
    }
}

#[derive(Params)]
struct DemoParams {
    drive: FloatParam,
    cutoff: FloatParam,
    voice: EnumParam<Voice>,
    power: BoolParam,
    #[param(skip)]
    _scratch: Vec<f32>,
}

impl DemoParams {
    fn new() -> Self {
        Self {
            drive: FloatParam::new(ParamId(10), "drive", Range::linear(-12.0, 36.0))
                .unit(Unit::Db)
                .smooth(Smooth::ms(15.0)),
            cutoff: FloatParam::new(ParamId(11), "cutoff", Range::log(20.0, 20_000.0)).default(1200.0)
                .unit(Unit::Hz),
            voice: EnumParam::new(ParamId(12), "voice", Voice::Flat),
            power: BoolParam::new(ParamId(13), "power", true),
            _scratch: Vec::new(),
        }
    }
}

#[test]
fn derive_generates_complete_table() {
    let p = DemoParams::new();
    assert_eq!(p.count(), 4, "skip field excluded");
    p.validate().expect("no duplicate ids");
    let names: Vec<&str> = (0..p.count())
        .map(|i| p.by_index(i).unwrap().meta().name)
        .collect();
    assert_eq!(
        names,
        ["drive", "cutoff", "voice", "power"],
        "declaration order preserved"
    );
    assert!(p.by_index(4).is_none());
}

#[test]
fn derive_table_is_id_addressable_and_live() {
    let p = DemoParams::new();
    let cutoff = p.by_id(ParamId(11)).expect("id 11");
    cutoff.set_normalized(0.5);
    // Log 20..20k midpoint = geometric mean.
    assert!(
        (cutoff.value() - 632.455).abs() < 0.01,
        "{}",
        cutoff.value()
    );
    assert_eq!(&*cutoff.format(cutoff.value()), "632.5 Hz");
    // The dyn view and the typed field are the same object.
    assert_eq!(f64::from(p.cutoff.value()), cutoff.value() as f32 as f64);
}
