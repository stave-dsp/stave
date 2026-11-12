//! `ui!` — true-XML markup for the stave-ui primitive layer.
//!
//! Grammar:
//! ```text
//! element   := "<" Name attr* ( "/>" | ">" child* "</" Name ">" )
//! attr      := ident "=" ( literal | "{" expr "}" | ident )   // bare ident: enum variant or flag
//! child     := element | string-literal | "{" expr "}"
//! ```
//! Primitives (capitalized, fixed set): `Column`, `Row`, `Stack`,
//! `Grid cols=N`, `Box`, `Spacer`, `Text`, `Glyph name=".."`,
//! `Shape kind=Rect|RRect|Circle|Line`. TEXT IS FIRST-CLASS: glyphs
//! render only through `<Text>`; components never take display
//! strings as props.
//!
//! Everything else is a COMPONENT: `<Power param={&p.bypass}/>`
//! expands to `Power { param: &p.bypass }.node()` (the `Component`
//! trait must be in scope). Layout attrs from the whitelist below
//! apply as modifiers to ANY element, components included.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, Expr, Ident, Lit, Token};

/// Attrs that are LAYOUT modifiers on every element (never component
/// props). Kept in one place — the macro's whole opinion.
const LAYOUT_ATTRS: &[&str] = &[
    "w",
    "h",
    "pad",
    "gap",
    "margin",
    "bg",
    "radius",
    "align",
    "justify",
    "wrap",
    "class",
    "fill",
    "tag",
    "border",
    "shadow",
    "gap_y",
    "show_when",
    "show_view",
];

enum AttrVal {
    Lit(Lit),
    Expr(Expr),
    /// Bare ident — enum shorthand (`align=Center`) or flag (`wrap`).
    Word(Ident),
    Flag,
}

struct Attr {
    name: Ident,
    val: AttrVal,
}

enum Child {
    El(Element),
    Str(syn::LitStr),
    Expr(Expr),
}

struct Element {
    name: Ident,
    attrs: Vec<Attr>,
    children: Vec<Child>,
}

impl Parse for Element {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<Token![<]>()?;
        let name: Ident = input.parse()?;
        let mut attrs = Vec::new();
        loop {
            if input.peek(Token![/]) {
                input.parse::<Token![/]>()?;
                input.parse::<Token![>]>()?;
                return Ok(Element {
                    name,
                    attrs,
                    children: Vec::new(),
                });
            }
            if input.peek(Token![>]) {
                input.parse::<Token![>]>()?;
                break;
            }
            let a_name: Ident = input.parse()?;
            if input.peek(Token![=]) {
                input.parse::<Token![=]>()?;
                if input.peek(syn::token::Brace) {
                    let content;
                    syn::braced!(content in input);
                    attrs.push(Attr {
                        name: a_name,
                        val: AttrVal::Expr(content.parse()?),
                    });
                } else if input.peek(syn::Ident) {
                    attrs.push(Attr {
                        name: a_name,
                        val: AttrVal::Word(input.parse()?),
                    });
                } else {
                    attrs.push(Attr {
                        name: a_name,
                        val: AttrVal::Lit(input.parse()?),
                    });
                }
            } else {
                attrs.push(Attr {
                    name: a_name,
                    val: AttrVal::Flag,
                });
            }
        }
        let mut children = Vec::new();
        loop {
            if input.peek(Token![<]) && input.peek2(Token![/]) {
                input.parse::<Token![<]>()?;
                input.parse::<Token![/]>()?;
                let close: Ident = input.parse()?;
                if close != name {
                    return Err(syn::Error::new(
                        close.span(),
                        format!("closing </{close}> does not match <{name}>"),
                    ));
                }
                input.parse::<Token![>]>()?;
                break;
            }
            if input.peek(Token![<]) {
                children.push(Child::El(input.parse()?));
            } else if input.peek(syn::LitStr) {
                children.push(Child::Str(input.parse()?));
            } else if input.peek(syn::token::Brace) {
                let content;
                syn::braced!(content in input);
                children.push(Child::Expr(content.parse()?));
            } else {
                return Err(input.error("expected child element, string literal, or {expr}"));
            }
        }
        Ok(Element {
            name,
            attrs,
            children,
        })
    }
}

fn num(lit: &Lit) -> TokenStream2 {
    match lit {
        Lit::Int(i) => {
            let v: f64 = i.base10_parse().unwrap_or(0.0);
            quote!(#v)
        }
        Lit::Float(f) => {
            let v: f64 = f.base10_parse().unwrap_or(0.0);
            quote!(#v)
        }
        other => other.to_token_stream(),
    }
}

/// One layout attr → one modifier call.
fn modifier(a: &Attr) -> TokenStream2 {
    let n = a.name.to_string();
    match (n.as_str(), &a.val) {
        ("w", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.width(::stave_ui::Dim::Px(#v)))
        }
        ("h", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.height(::stave_ui::Dim::Px(#v)))
        }
        ("w", AttrVal::Expr(e)) => quote!(.width(#e)),
        ("h", AttrVal::Expr(e)) => quote!(.height(#e)),
        ("w", AttrVal::Word(w)) if w == "Fill" => quote!(.width(::stave_ui::Dim::Fill(1.0))),
        ("h", AttrVal::Word(w)) if w == "Fill" => quote!(.height(::stave_ui::Dim::Fill(1.0))),
        ("pad", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.pad(#v))
        }
        ("pad", AttrVal::Expr(e)) => quote!(.pad_edges(#e)),
        ("gap", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.gap(#v))
        }
        ("margin", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.margin(#v))
        }
        ("margin", AttrVal::Expr(e)) => quote!(.margin_edges(#e)),
        ("bg", AttrVal::Expr(e)) => quote!(.bg(#e)),
        ("radius", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.radius(#v))
        }
        ("radius", AttrVal::Expr(e)) => quote!(.radius(#e)),
        ("align", AttrVal::Word(w)) => quote!(.align(::stave_ui::Align::#w)),
        ("justify", AttrVal::Word(w)) => quote!(.justify(::stave_ui::Justify::#w)),
        ("wrap", AttrVal::Flag) => quote!(.wrap()),
        ("class", AttrVal::Expr(e)) => quote!(.text_style(#e)),
        ("fill", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.fill(#v))
        }
        ("tag", AttrVal::Lit(l)) => quote!(.tag(#l)),
        // Tab pages: `show_when={(&p.algo, 0)}` — tuple form like
        // border/shadow; needs the kit's `ShowWhen` trait in scope.
        ("show_when", AttrVal::Expr(e)) => quote!(.show_when_t(#e)),
        ("show_view", AttrVal::Expr(e)) => quote!(.show_view_t(#e)),
        ("border", AttrVal::Expr(e)) => quote!(.border_t(#e)),
        ("shadow", AttrVal::Expr(e)) => quote!(.shadow_t(#e)),
        ("gap_y", AttrVal::Lit(l)) => {
            let v = num(l);
            quote!(.gap_y(#v))
        }
        _ => syn::Error::new(a.name.span(), format!("unsupported layout attr `{n}` form"))
            .to_compile_error(),
    }
}

fn gen(el: &Element) -> TokenStream2 {
    let name = el.name.to_string();
    let (layout, props): (Vec<&Attr>, Vec<&Attr>) = el
        .attrs
        .iter()
        .partition(|a| LAYOUT_ATTRS.contains(&a.name.to_string().as_str()));
    let mods: Vec<TokenStream2> = layout.iter().map(|a| modifier(a)).collect();
    let kids: Vec<TokenStream2> = el
        .children
        .iter()
        .map(|c| match c {
            Child::El(e) => gen(e),
            Child::Expr(e) => e.to_token_stream(),
            Child::Str(s) => syn::Error::new(
                s.span(),
                "string children belong inside <Text> — Text is the glyph primitive",
            )
            .to_compile_error(),
        })
        .collect();
    let base = match name.as_str() {
        "Column" => quote!(::stave_ui::column(::std::vec![#(#kids),*])),
        "Row" => quote!(::stave_ui::row(::std::vec![#(#kids),*])),
        "Stack" => quote!(::stave_ui::stack(::std::vec![#(#kids),*])),
        "Box" => quote!(::stave_ui::boxed(::std::vec![#(#kids),*])),
        "Spacer" => quote!(::stave_ui::spacer()),
        "Grid" => {
            let cols = el
                .attrs
                .iter()
                .find(|a| a.name == "cols")
                .map(|a| match &a.val {
                    AttrVal::Lit(Lit::Int(i)) => {
                        let v: u16 = i.base10_parse().unwrap_or(1);
                        quote!(#v)
                    }
                    AttrVal::Expr(e) => e.to_token_stream(),
                    _ => quote!(1u16),
                })
                .unwrap_or(quote!(1u16));
            quote!(::stave_ui::grid(#cols, ::std::vec![#(#kids),*]))
        }
        "Text" => {
            let content = match el.children.as_slice() {
                [Child::Str(s)] => s.to_token_stream(),
                [Child::Expr(e)] => e.to_token_stream(),
                _ => syn::Error::new(
                    el.name.span(),
                    "<Text> takes exactly one child: a string literal or {expr}",
                )
                .to_compile_error(),
            };
            quote!(::stave_ui::text(#content))
        }
        "Glyph" => {
            let g = el
                .attrs
                .iter()
                .find(|a| a.name == "name")
                .map(|a| match &a.val {
                    AttrVal::Lit(l) => l.to_token_stream(),
                    AttrVal::Expr(e) => e.to_token_stream(),
                    _ => quote!(""),
                })
                .unwrap_or_else(|| {
                    syn::Error::new(el.name.span(), "<Glyph> needs name=\"..\"").to_compile_error()
                });
            quote!(::stave_ui::glyph(#g))
        }
        "Shape" => {
            let k = el
                .attrs
                .iter()
                .find(|a| a.name == "kind")
                .map(|a| match &a.val {
                    AttrVal::Word(w) => quote!(::stave_ui::Shape::#w),
                    AttrVal::Expr(e) => e.to_token_stream(),
                    _ => quote!(::stave_ui::Shape::Rect),
                })
                .unwrap_or(quote!(::stave_ui::Shape::Rect));
            quote!(::stave_ui::shape(#k))
        }
        // Component: struct literal from the prop attrs + .node().
        _ => {
            let ident = &el.name;
            // Grid's cols / Glyph's name / Shape's kind never reach
            // here (matched above); every non-layout attr is a prop.
            let fields: Vec<TokenStream2> = props
                .iter()
                .map(|a| {
                    let k = &a.name;
                    match &a.val {
                        AttrVal::Lit(l) => quote!(#k: #l),
                        AttrVal::Expr(e) => quote!(#k: #e),
                        AttrVal::Word(w) => quote!(#k: #w),
                        AttrVal::Flag => quote!(#k: true),
                    }
                })
                .collect();
            if !el.children.is_empty() {
                return syn::Error::new(
                    el.name.span(),
                    "components are self-closing in v1; pass trees via {expr} props",
                )
                .to_compile_error();
            }
            quote!(#ident { #(#fields),* }.node())
        }
    };
    quote!(#base #(#mods)*)
}

/// XML markup → a `stave_ui::Node` expression tree.
pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let el = parse_macro_input!(input as Element);
    gen(&el).into()
}
