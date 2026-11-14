//! What the Audio Unit tests share: a plugin's `StaveAUFactory`
//! registered with AudioToolbox in this process (`AudioComponentRegister`)
//! and opened by stave-host's Audio Unit backend, so every call goes
//! through AudioToolbox's own dispatch, as in a DAW.
//!
//! One plugin per test binary: every stave plugin exports the same C
//! symbols, so two linked into one binary would share one factory.

use std::num::NonZeroUsize;
use std::sync::OnceLock;

use stave_au_sys::{fourcc, AudioComponentDescription, AudioComponentFactoryFunction};
use stave_host::au::{AuInstance, Component};
use stave_host::config::{HostConfig, SampleRate};
use stave_host::instance::Instance;
use stave_host::validate::{Status, Suite};

pub const RATE: f64 = 48_000.0;
pub const BLOCK: usize = 256;

pub fn config() -> HostConfig {
    HostConfig {
        sample_rate: SampleRate::new(RATE).unwrap(),
        max_block: NonZeroUsize::new(BLOCK).unwrap(),
        ..HostConfig::default()
    }
}

/// A fresh instance of the plugin `factory` makes, registered once per
/// process under `kind` (a second registration of the same codes would
/// replace the first and invalidate its handle).
pub fn open(kind: &[u8; 4], name: &str, factory: AudioComponentFactoryFunction) -> AuInstance {
    static COMPONENT: OnceLock<Component> = OnceLock::new();
    let c = COMPONENT.get_or_init(|| {
        let desc = AudioComponentDescription {
            componentType: fourcc(kind),
            componentSubType: fourcc(b"test"),
            componentManufacturer: fourcc(b"StTs"),
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        Component::register(desc, name, 0x0400, factory)
    });
    AuInstance::open_component(c.clone(), config()).expect("the plugin opens as an Audio Unit")
}

pub fn energy(out: &[f32]) -> f64 {
    out.iter().map(|v| f64::from(*v).powi(2)).sum()
}

/// Every check of the validation suite passes or does not apply.
pub fn passes_the_validation_suite(make: fn() -> AuInstance) {
    let mut factory = || Ok(Box::new(make()) as Box<dyn Instance>);
    let report = Suite::default().run(&mut factory, |_| {});
    let bad: Vec<_> = report.checks.iter().filter(|c| c.status == Status::Fail).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}
