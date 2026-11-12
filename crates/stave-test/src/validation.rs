//! Every plugin's price of admission: stave-host's validation suite, run
//! through the plugin's own VST3 entry points, so the ABI is under test
//! too.

use stave_host::config::HostConfig;
use stave_host::opened::OpenError;
use stave_host::plugin::{EntryPoints, LoadedPlugin};
use stave_host::validate::{Status, Suite};

/// Run [`Suite::default`] on fresh instances of the linked plugin and
/// panic with every failing check's details.
pub fn assert_passes(entry: EntryPoints) {
    let mut fresh = || {
        LoadedPlugin::from_entry_points(entry, HostConfig::default())
            .map(|p| Box::new(p) as _)
            .map_err(OpenError::Linked)
    };
    let report = Suite::default().run(&mut fresh, |_| {});
    let failures: Vec<String> = report
        .checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| format!("{}: {}", c.check, c.details))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
