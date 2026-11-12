//! `#[derive(Taps)]` — a struct of `Tap` fields becomes a telemetry table:
//! one `TapId` constant per field (`SPIN` for `spin`), a constructor that
//! registers every field, `ALL` (every id, in field order) and a
//! detached `Default`.
//!
//! ```ignore
//! #[derive(Taps)]
//! pub struct HeirloomTaps { pub spin: Tap, pub level: Tap }
//!
//! let taps = HeirloomTaps::new(&telemetry);  // registers "spin", "level"
//! taps.spin.set(angle);                      // realtime
//! telemetry.value(HeirloomTaps::SPIN);       // a reader, by id
//! ```
//!
//! The tap's wire name is the field name. A name longer than a telemetry
//! slot holds is a compile error at the field; so is a field that is not
//! a `Tap` (at the generated `Tap::register` call).

use proc_macro::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

/// `core` is the path of the stave model crate where the deriving crate
/// sees it (`::stave_core`, or `::stave::__core` through the umbrella).
pub(crate) fn derive(input: TokenStream, core: proc_macro2::TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    expand(ast, core)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand(ast: DeriveInput, core: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, syn::Error> {
    let Data::Struct(data) = &ast.data else {
        return Err(syn::Error::new(ast.ident.span(), "#[derive(Taps)] requires a struct"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(ast.ident.span(), "#[derive(Taps)] requires named fields"));
    };
    if !ast.generics.params.is_empty() {
        return Err(syn::Error::new(ast.generics.span(), "#[derive(Taps)] takes no generics"));
    }

    let t = quote! { #core::telemetry };
    let mut consts = Vec::new();
    let mut checks = Vec::new();
    let mut ids = Vec::new();
    let mut registers = Vec::new();
    let mut detached = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let name = ident.to_string();
        let name = name.strip_prefix("r#").unwrap_or(&name).to_string();
        if name == "all" {
            return Err(syn::Error::new(
                ident.span(),
                "a tap named `all` would collide with the generated `ALL` list",
            ));
        }
        let konst = format_ident!("{}", name.to_uppercase(), span = ident.span());
        let span = field.span();
        let doc = format!("The `{name}` tap's id.");
        consts.push(quote_spanned! {span=>
            #[doc = #doc]
            pub const #konst: #t::TapId = #t::TapId::new(#name);
        });
        let msg = format!(
            "tap name `{name}` is longer than a telemetry slot holds (NAME_BYTES - 1 bytes)"
        );
        checks.push(quote_spanned! {span=>
            const _: () = assert!(#name.len() < #t::NAME_BYTES, #msg);
        });
        ids.push(quote! { Self::#konst });
        let ty_span = field.ty.span();
        registers.push(quote_spanned! {ty_span=>
            #ident: #t::Tap::register(telemetry, Self::#konst)
        });
        detached.push(quote_spanned! {ty_span=> #ident: #t::Tap::detached() });
    }

    let ty = &ast.ident;
    Ok(quote! {
        #(#checks)*

        impl #ty {
            #(#consts)*

            /// Every tap's id, in field order.
            pub const ALL: &'static [#t::TapId] = &[#(#ids),*];

            /// Register every tap on `telemetry` (in `Plugin::new`, not on
            /// the audio thread).
            pub fn new(telemetry: &#t::Telemetry) -> Self {
                Self { #(#registers),* }
            }
        }

        impl ::core::default::Default for #ty {
            /// Every tap detached: writes are dropped.
            fn default() -> Self {
                Self { #(#detached),* }
            }
        }
    })
}
