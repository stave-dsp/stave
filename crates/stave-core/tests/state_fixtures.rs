//! Committed-chunk compatibility corpus.
//!
//! Every file under `tests/fixtures/` is a chunk some past version
//! wrote. The corpus ONLY EVER GROWS: when the format or a plugin's
//! params change, add a new fixture — never regenerate an old one.
//! A failure here means shipped sessions would break.

use stave_core::params::{AnyParam, FloatParam, ParamId, Params, Range, Smooth};
use stave_core::state::{load, Migrations, StateStore};

/// v1-basic.stve: format v1, plugin_version 1, params
/// {10: 6.5, 11: 632.4553, 999: 0.777}, extra "legacy-extra",
/// one unknown section (tag 77).
const V1_BASIC: &[u8] = include_bytes!("fixtures/v1-basic.stve");

struct T2(FloatParam, FloatParam);
impl Params for T2 {
    fn count(&self) -> usize {
        2
    }
    fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
        [&self.0 as &dyn AnyParam, &self.1].get(i).copied()
    }
}

fn table() -> T2 {
    T2(
        FloatParam::new(ParamId(10), "drive", Range::linear(-12.0, 36.0))
            .smooth(Smooth::Off),
        FloatParam::new(ParamId(11), "cutoff", Range::log(20.0, 20_000.0)).default(1200.0)
            .smooth(Smooth::Off),
    )
}

#[test]
fn v1_basic_loads_forever() {
    let t = table();
    let mut store = StateStore::default();
    let (report, extra) =
        load(V1_BASIC, &t, &Migrations::default(), &mut store).expect("shipped chunk must load");
    assert_eq!(report.plugin_version, 1);
    assert_eq!(report.applied, 2);
    assert_eq!(report.unknown_params, 1, "id 999 retained, not lost");
    assert_eq!(report.unknown_sections, 1, "tag 77 retained");
    assert_eq!(AnyParam::value(&t.0).to_bits(), 6.5f64.to_bits());
    assert_eq!(AnyParam::value(&t.1).to_bits(), 632.4553f64.to_bits());
    assert_eq!(extra, b"legacy-extra");
}

#[test]
fn v1_basic_survives_a_resave_cycle() {
    let t = table();
    let mut store = StateStore::default();
    let _ = load(V1_BASIC, &t, &Migrations::default(), &mut store).unwrap();
    let resaved = stave_core::state::save(&t, 2, b"legacy-extra", &store);
    // A future version reading the resave still sees everything.
    let t2 = table();
    let mut store2 = StateStore::default();
    let (report, extra) = load(&resaved, &t2, &Migrations::default(), &mut store2).unwrap();
    assert_eq!(report.applied, 2);
    assert_eq!(report.unknown_params, 1);
    assert_eq!(report.unknown_sections, 1);
    assert_eq!(extra, b"legacy-extra");
}
