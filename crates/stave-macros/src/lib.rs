//! stave's procedural macros, re-exported where they are used:
//! `#[derive(Params)]` by `stave_core::params`, `#[derive(Taps)]` by
//! `stave_core::telemetry`.
//! (Proc-macros must live in a crate of their own.)

use proc_macro::TokenStream;

mod params;
mod taps;

/// The `stave_core::params::Params` table for a struct of parameters.
#[proc_macro_derive(Params, attributes(param))]
pub fn derive_params(input: TokenStream) -> TokenStream {
    params::derive(input)
}

/// A telemetry table for a struct of `Tap` fields: a `TapId` constant per
/// field, `new(&Arc<Telemetry>)` registering them, `ALL`, and a detached
/// `Default`. See `stave_core::telemetry`.
#[proc_macro_derive(Taps)]
pub fn derive_taps(input: TokenStream) -> TokenStream {
    taps::derive(input)
}
