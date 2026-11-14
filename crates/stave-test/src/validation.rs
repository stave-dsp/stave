//! Every plugin's price of admission: stave-host's validation suite, run
//! through the plugin's own VST3 entry points, so the ABI is under test
//! too — and on macOS through its Audio Unit as well
//! (`assert_au_passes`).
//!
//! [`plugin_checks!`](macro@crate::plugin_checks) mounts it as one test
//! per check ([`assert_check_passes`]), and the per-param checks (sweep,
//! glide, solver) as [`PARTS`] tests each over a part of the params, so
//! a test runner runs them side by side: the same checks of the same
//! params as [`assert_passes`], each test a fraction of the time.

use stave_host::config::HostConfig;
use stave_host::opened::OpenError;
use stave_host::plugin::{EntryPoints, LoadedPlugin};
use stave_host::validate::{CheckResult, ParamShard, Status, Suite, SuiteConfig};

/// How many tests each per-param check is split into (the macro mounts
/// `part_1_of_4` … `part_4_of_4`).
pub const PARTS: usize = 4;

/// Run [`Suite::default`] on fresh instances of the linked plugin and
/// panic with every failing check's details.
pub fn assert_passes(entry: EntryPoints) {
    let report = Suite::default().run(&mut fresh(entry), |_| {});
    assert_no_failures(&report.checks);
}

/// Run the check named `check` of [`Suite::default`] over the params in
/// `params` (a per-param check; the others take [`ParamShard::ALL`]) and
/// panic with its details if it fails.
pub fn assert_check_passes(entry: EntryPoints, check: &str, params: ParamShard) {
    let suite = Suite { config: SuiteConfig { params, ..SuiteConfig::default() }, ..Suite::default() };
    let found = suite.checks.iter().find(|c| c.name == check);
    let check = found.unwrap_or_else(|| panic!("stave-host's suite has no check named {check}"));
    assert_no_failures(&[suite.run_check(&mut fresh(entry), check)]);
}

fn fresh(entry: EntryPoints) -> impl FnMut() -> Result<Box<dyn stave_host::instance::Instance>, OpenError> {
    move || {
        LoadedPlugin::from_entry_points(entry, HostConfig::default())
            .map(|p| Box::new(p) as _)
            .map_err(OpenError::Linked)
    }
}

/// Run [`Suite::default`] on fresh instances of the linked plugin's Audio
/// Unit: its factory registered with AudioToolbox under `codes` and
/// driven through AudioToolbox, as a DAW drives it. Once per test binary
/// (the registration is the process's).
#[cfg(target_os = "macos")]
pub fn assert_au_passes(codes: [[u8; 4]; 3], name: &str, factory: stave_host::au::AudioComponentFactoryFunction) {
    use stave_host::au::{AuInstance, Component};
    static COMPONENT: std::sync::OnceLock<Component> = std::sync::OnceLock::new();
    let component = COMPONENT.get_or_init(|| Component::linked(codes, name, factory));
    let mut fresh = || {
        AuInstance::open_component(component.clone(), HostConfig::default())
            .map(|a| Box::new(a) as Box<dyn stave_host::instance::Instance>)
            .map_err(|source| OpenError::AudioUnit { query: name.to_string(), source })
    };
    let report = Suite::default().run(&mut fresh, |_| {});
    assert_no_failures(&report.checks);
}

fn assert_no_failures(checks: &[CheckResult]) {
    let failures: Vec<String> = checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| format!("{}: {}", c.check, c.details))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
