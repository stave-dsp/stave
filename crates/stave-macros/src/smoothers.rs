//! `#[derive(Smoothers)]` — a struct of `Smoother` fields becomes a
//! group that is built, prepared and ramped together: one smoother per
//! field, each from the param of the same name in the params struct.
//!
//! ```ignore
//! #[derive(Smoothers)]
//! #[smoothers(params = BandParams, frame = BandSettings)]
//! struct BandSmoothers { threshold: Smoother, ratio: Smoother }
//!
//! let mut s = BandSmoothers::new(&params);      // Plugin::new
//! s.prepare(env);                               // Plugin::prepare
//! s.snap(&params);                              // jump, no glide
//! let mut ramps = s.ramp(block, &params);       // Plugin::process
//! let t = ramps.threshold.next();               // one field…
//! let settings: BandSettings = ramps.next();    // …or every field (`frame`)
//! ```
//!
//! The ramps are a generated struct, `{Name}Ramps<'s>`, with a `Ramp`
//! field per smoother (same names, same visibility as the group). With
//! `frame = T`, `{Name}Ramps::next()` returns a `T` built field for field
//! from each ramp's next value, so `T` has an `f32` field per smoother.

use proc_macro::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Path};

/// `core` is the path of the stave model crate where the deriving crate
/// sees it (`::stave_core`, or `::stave::__core` through the umbrella).
pub(crate) fn derive(input: TokenStream, core: proc_macro2::TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    expand(ast, core).unwrap_or_else(|e| e.to_compile_error()).into()
}

struct Options {
    params: Path,
    frame: Option<Path>,
}

fn options(ast: &DeriveInput) -> Result<Options, syn::Error> {
    let mut params = None;
    let mut frame = None;
    for attr in ast.attrs.iter().filter(|a| a.path().is_ident("smoothers")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("params") {
                params = Some(meta.value()?.parse::<Path>()?);
                Ok(())
            } else if meta.path.is_ident("frame") {
                frame = Some(meta.value()?.parse::<Path>()?);
                Ok(())
            } else {
                Err(meta.error("unknown #[smoothers(...)] option; `params = Type` or `frame = Type`"))
            }
        })?;
    }
    let params = params.ok_or_else(|| {
        syn::Error::new(
            ast.ident.span(),
            "#[derive(Smoothers)] needs `#[smoothers(params = YourParams)]`: the struct its smoothers read",
        )
    })?;
    Ok(Options { params, frame })
}

fn expand(ast: DeriveInput, core: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, syn::Error> {
    let Data::Struct(data) = &ast.data else {
        return Err(syn::Error::new(ast.ident.span(), "#[derive(Smoothers)] requires a struct"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(ast.ident.span(), "#[derive(Smoothers)] requires named fields"));
    };
    if !ast.generics.params.is_empty() {
        return Err(syn::Error::new(ast.generics.span(), "#[derive(Smoothers)] takes no generics"));
    }
    if fields.named.is_empty() {
        return Err(syn::Error::new(ast.ident.span(), "#[derive(Smoothers)] on a struct with no smoothers"));
    }
    let Options { params, frame } = options(&ast)?;

    let p = quote! { #core::params };
    let mut news = Vec::new();
    let mut prepares = Vec::new();
    let mut snaps = Vec::new();
    let mut ramp_fields = Vec::new();
    let mut ramps = Vec::new();
    let mut nexts = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let vis = &field.vis;
        // Errors land on the field: a non-`Smoother` field fails at its
        // `Smoother::new`, a missing param at `params.<field>`.
        let span = field.ty.span();
        news.push(quote_spanned! {span=> #ident: #p::Smoother::new(&params.#ident) });
        prepares.push(quote_spanned! {span=> #p::Smoother::prepare(&mut self.#ident, env); });
        snaps.push(quote_spanned! {span=>
            #p::Smoother::snap(&mut self.#ident, #p::AnyParam::value(&params.#ident));
        });
        let doc = format!("This block's `{ident}` values.");
        ramp_fields.push(quote! { #[doc = #doc] #vis #ident: #p::Ramp<'s> });
        ramps.push(quote_spanned! {span=> #ident: #p::Smoother::ramp(&mut self.#ident, block, &params.#ident) });
        nexts.push(quote! { #ident: self.#ident.next() });
    }

    let name = &ast.ident;
    let vis = &ast.vis;
    let ramps_name = format_ident!("{}Ramps", name);
    let ramps_doc = format!("One block of [`{name}`]'s ramps: each field's `next()` once per frame.");
    let frame_next = frame.map(|frame| {
        quote! {
            impl #ramps_name<'_> {
                /// Every field's next value, as one frame.
                #[inline]
                #[allow(clippy::should_implement_trait)]
                pub fn next(&mut self) -> #frame {
                    #frame { #(#nexts),* }
                }
            }
        }
    });
    Ok(quote! {
        #[doc = #ramps_doc]
        #vis struct #ramps_name<'s> {
            #(#ramp_fields),*
        }

        impl #name {
            /// One smoother per field, each gliding as its param declares,
            /// starting at the param's value. May allocate.
            pub fn new(params: &#params) -> Self {
                Self { #(#news),* }
            }

            /// Derive every smoother's rate-dependent state (in
            /// `Plugin::prepare`).
            pub fn prepare(&mut self, env: &#core::Environment) {
                #(#prepares)*
            }

            /// Jump every smoother to its param's value, without a glide.
            pub fn snap(&mut self, params: &#params) {
                #(#snaps)*
            }

            /// This block's ramps, honouring its timed changes.
            pub fn ramp<'s>(&'s mut self, block: &#core::Block<'s>, params: &#params) -> #ramps_name<'s> {
                #ramps_name { #(#ramps),* }
            }
        }

        #frame_next
    })
}
