//! `#[derive(Params)]` — generates the `stave_core::params::Params`
//! table impl for a struct of parameter fields.
//!
//! Design: the DECLARATION lives in the field constructors
//! (`FloatParam::new(id, ...)` builders), where ranges and units are
//! ordinary expressions; the derive only generates the iteration glue.
//! This is deliberate — an attribute grammar would re-encode what
//! builders already express, worse.
//!
//! ```ignore
//! #[derive(Params)]
//! struct ForgeParams {
//!     drive: FloatParam,
//!     character: EnumParam<Character>,
//!     #[param(group)]
//!     routes: [RouteParams<Source, Target>; 4], // a nested table, flattened in order
//!     #[param(skip)]
//!     scratch: Vec<f32>,          // not a param; excluded from the table
//! }
//! ```
//!
//! Every non-skipped field must implement `AnyParam`; a field that
//! doesn't is a compile error at the generated `&self.field as &dyn
//! AnyParam` coercion — pointing at the offending field.
//!
//! Id-collision detection cannot happen at macro time (ids are runtime
//! constructor arguments by design); wrappers call
//! `Params::validate()` at instance creation, which fails loudly on
//! the FIRST instantiation in development.

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    expand(ast).unwrap_or_else(|e| e.to_compile_error().into())
}

fn expand(ast: DeriveInput) -> Result<TokenStream, syn::Error> {
    let Data::Struct(data) = &ast.data else {
        return Err(syn::Error::new(
            ast.ident.span(),
            "#[derive(Params)] requires a struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            ast.ident.span(),
            "#[derive(Params)] requires named fields",
        ));
    };

    // Fields in declaration order: a param is one slot, a group its own
    // table's count. Lookup walks them, subtracting what each spans.
    let mut counts = Vec::new();
    let mut lookups = Vec::new();
    for field in &fields.named {
        let kind = field_kind(field)?;
        if kind == Kind::Skip {
            continue;
        }
        let ident = field.ident.as_ref().expect("named field");
        // quote_spanned pins a type error to the field, not to the
        // derive attribute.
        let span = field.ty.span();
        if kind == Kind::Group {
            counts.push(quote_spanned! {span=>
                ::stave_core::params::Params::count(&self.#ident)
            });
            lookups.push(quote_spanned! {span=>
                let n = ::stave_core::params::Params::count(&self.#ident);
                if index < n {
                    return ::stave_core::params::Params::by_index(&self.#ident, index);
                }
                index -= n;
            });
        } else {
            counts.push(quote! { 1 });
            lookups.push(quote_spanned! {span=>
                if index == 0 {
                    return ::core::option::Option::Some(
                        &self.#ident as &dyn ::stave_core::params::AnyParam
                    );
                }
                index -= 1;
            });
        }
    }
    if counts.is_empty() {
        return Err(syn::Error::new(
            ast.ident.span(),
            "#[derive(Params)] on a struct with no param fields — use stave_core::params::NoParams instead",
        ));
    }

    let name = &ast.ident;
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();
    let expanded = quote! {
        impl #impl_generics ::stave_core::params::Params for #name #ty_generics #where_clause {
            fn count(&self) -> usize {
                0 #(+ #counts)*
            }
            fn by_index(
                &self,
                index: usize,
            ) -> ::core::option::Option<&dyn ::stave_core::params::AnyParam> {
                let mut index = index;
                #(#lookups)*
                let _ = index;
                ::core::option::Option::None
            }
        }
    };
    Ok(expanded.into())
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Kind {
    Param,
    /// `#[param(group)]`: a nested `Params` table, flattened in place.
    Group,
    /// `#[param(skip)]`: not in the table.
    Skip,
}

fn field_kind(field: &syn::Field) -> Result<Kind, syn::Error> {
    let mut kind = Kind::Param;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("param")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                kind = Kind::Skip;
                Ok(())
            } else if meta.path.is_ident("group") {
                kind = Kind::Group;
                Ok(())
            } else {
                Err(meta.error("unknown #[param(...)] option; `skip` or `group`"))
            }
        })?;
    }
    Ok(kind)
}
