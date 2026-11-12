//! Why a plugin failed to load.

use stave_vst3::sys::Tresult;

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
    #[error("{call} failed: tresult=0x{:08x}", *result as u32)]
    Call { call: &'static str, result: Tresult },
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
        let e = HostError::call("setActive")(stave_vst3::sys::K_NOT_IMPLEMENTED);
        assert_eq!(e.to_string(), "setActive failed: tresult=0x80000001");
    }
}
