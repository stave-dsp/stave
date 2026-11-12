//! stave's procedural macros, re-exported where they are used:
//! `#[derive(Params)]` by `stave_core::params`.
//! (Proc-macros must live in a crate of their own.)

use proc_macro::TokenStream;

mod params;

/// The `stave_core::params::Params` table for a struct of parameters.
#[proc_macro_derive(Params, attributes(param))]
pub fn derive_params(input: TokenStream) -> TokenStream {
    params::derive(input)
}
