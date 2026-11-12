//! Host-supplied state bytes (`setState`, `clap_plugin_state.load`):
//! any bytes either load or are refused, never panic; a refused load
//! leaves the table untouched; a loaded state saves and loads back to
//! the same bytes; the committed fixture corpus seeds the search.
#![no_main]

use libfuzzer_sys::fuzz_target;
use stave_core::params::{AnyParam, BoolParam, FloatParam, ParamId, Params, Range, Smooth};
use stave_core::state::{load, save, Migrations, StateStore};

struct Table(FloatParam, FloatParam, BoolParam);
impl Params for Table {
    fn count(&self) -> usize {
        3
    }
    fn by_index(&self, i: usize) -> Option<&dyn AnyParam> {
        [&self.0 as &dyn AnyParam, &self.1, &self.2].get(i).copied()
    }
}

fn table() -> Table {
    Table(
        FloatParam::new(ParamId(10), "drive", Range::linear(-12.0, 36.0)).smooth(Smooth::Off),
        FloatParam::new(ParamId(11), "cutoff", Range::log(20.0, 20_000.0)).smooth(Smooth::Off),
        BoolParam::new(ParamId(12), "hold", false),
    )
}

fn values(t: &Table) -> [u64; 3] {
    [0, 1, 2].map(|i| t.by_index(i).unwrap().value().to_bits())
}

fuzz_target!(|bytes: &[u8]| {
    let t = table();
    let before = values(&t);
    let mut store = StateStore::default();
    match load(bytes, &t, &Migrations::default(), &mut store) {
        Err(_) => assert_eq!(values(&t), before, "a refused load changed the table"),
        Ok((_, extra)) => {
            let once = save(&t, 1, &extra, &store);
            let t2 = table();
            let mut store2 = StateStore::default();
            let (_, extra2) = load(&once, &t2, &Migrations::default(), &mut store2)
                .expect("a saved state loads");
            assert_eq!(values(&t2), values(&t));
            assert_eq!(save(&t2, 1, &extra2, &store2), once, "save ∘ load is the identity");
        }
    }
});
