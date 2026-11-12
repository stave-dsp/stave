//! stave's procedural macros, re-exported where they are used:
//! `#[derive(Params)]` and `#[derive(Taps)]` by `stave_core` (paths into
//! `::stave_core`) and by the `stave` umbrella (paths into
//! `::stave::__core`, so an author needs no other dependency).
//! (Proc-macros must live in a crate of their own.)

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod params;
mod taps;

/// The `Params` table for a struct of parameters.
#[proc_macro_derive(Params, attributes(param))]
pub fn derive_params(input: TokenStream) -> TokenStream {
    params::derive(input, quote::quote! { ::stave_core })
}

/// A telemetry table for a struct of `Tap` fields: a `TapId` constant per
/// field, `new(&Telemetry)` registering them, `ALL`, and a detached
/// `Default`. See `stave_core::telemetry`.
#[proc_macro_derive(Taps)]
pub fn derive_taps(input: TokenStream) -> TokenStream {
    taps::derive(input, quote::quote! { ::stave_core })
}

/// `Params`, for crates that depend on the `stave` umbrella only
/// (re-exported there as `Params`).
#[doc(hidden)]
#[proc_macro_derive(StaveParams, attributes(param))]
pub fn derive_stave_params(input: TokenStream) -> TokenStream {
    params::derive(input, quote::quote! { ::stave::__core })
}

/// `Taps`, for crates that depend on the `stave` umbrella only
/// (re-exported there as `Taps`).
#[doc(hidden)]
#[proc_macro_derive(StaveTaps)]
pub fn derive_stave_taps(input: TokenStream) -> TokenStream {
    taps::derive(input, quote::quote! { ::stave::__core })
}
