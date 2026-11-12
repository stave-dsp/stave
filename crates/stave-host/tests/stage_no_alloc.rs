// assert_no_alloc compiles its guard out of release builds.
#![cfg(debug_assertions)]

use assert_no_alloc::{assert_no_alloc, AllocDisabler};
use stave_host::config::HostConfig;
use stave_host::instance::Instance;
use stave_host::stages::{CatalogInstance, CATALOG};

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

#[test]
fn a_setting_change_is_applied_without_allocating() {
    for stage in CATALOG {
        let config = HostConfig {
            max_block: std::num::NonZeroUsize::new(64).unwrap(),
            ..HostConfig::default()
        };
        let mut instance = CatalogInstance::new(stage, &config);
        let handle = instance.handle();
        let input = vec![0.25f32; 64];
        let mut left = vec![0.0f32; 64];
        let mut right = vec![0.0f32; 64];
        for setting in stage.settings {
            handle.set(setting.name, setting.range.max()).unwrap();
            assert_no_alloc(|| {
                instance.process(&[&input, &input], &mut [&mut left, &mut right], &[]);
            });
        }
    }
}
