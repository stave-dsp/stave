//! A panic's message and location in the plugin's log before the default
//! hook runs — with `panic = "abort"` the last thing the plugin says
//! before its host goes down. Each plugin is its own binary with its own
//! copy of std, so the hook sees only this plugin's panics.

use super::{Diagnostics, Severity};
use crate::Info;
use std::sync::Once;

/// Idempotent; the format entry points call it.
pub fn log_panics(plugin: &'static Info) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Diagnostics::new(plugin)
                .log
                .write(Severity::Fatal, info);
            previous(info);
        }));
    });
}
