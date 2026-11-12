//! Why a plugin failed to load.

use stave_vst3_sys::{tresult_name, Tresult};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum HostError {
    #[error("a {0}-frame block does not fit VST3's 32-bit block size")]
    BlockTooLarge(usize),
    #[error("dlopen failed: {0}")]
    DlopenFailed(String),
    #[error("symbol missing: {0}")]
    SymbolMissing(&'static str),
    #[error("bundleEntry returned false")]
    BundleEntryFailed,
    #[error("GetPluginFactory returned NULL")]
    FactoryNull,
    #[error("no \"Audio Module Class\" among the factory's classes {classes:?}")]
    NoAudioModule { classes: Vec<String> },
    #[error("{call} failed: {}", describe(*result))]
    Call { call: &'static str, result: Tresult },
}

/// The SDK name (per platform: the values differ between Windows and
/// macOS/Linux) plus the raw code.
fn describe(result: Tresult) -> String {
    let name = tresult_name(result).unwrap_or("unknown result");
    format!("{name} (tresult=0x{:08x})", result as u32)
}

impl HostError {
    pub(crate) fn call(call: &'static str) -> impl FnOnce(Tresult) -> Self {
        move |result| Self::Call { call, result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_errors_name_the_call_and_hex_result() {
        let e = HostError::call("setActive")(stave_vst3_sys::K_NOT_IMPLEMENTED);
        #[cfg(not(windows))]
        let expected = "setActive failed: kNotImplemented (tresult=0x00000003)";
        #[cfg(windows)]
        let expected = "setActive failed: kNotImplemented (tresult=0x80004001)";
        assert_eq!(e.to_string(), expected);
        let e = HostError::call("process")(-7);
        assert_eq!(
            e.to_string(),
            "process failed: unknown result (tresult=0xfffffff9)"
        );
    }
}
