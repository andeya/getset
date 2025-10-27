/*!
Getset2 is a derive macro, which is inspired by [getset](https://crates.io/crates/getset),
is designed for generating the most basic getters and setters on struct fields.

```rust
use getset2::Getset2;

#[derive(Default, Getset2)]
#[getset2(get_ref, set_with)]
pub struct Foo<T>
where
    T: Copy + Clone + Default,
{
    /// Doc comments are supported!
    /// Multiline, even.
    #[getset2(set, get_mut, skip(get_ref))]
    private: T,

    /// Doc comments are supported!
    /// Multiline, even.
    #[getset2(
        get_copy(pub, const),
        set(pub = "crate"),
        get_mut(pub = "super"),
        set_with(pub = "self", const)
    )]
    public: T,

    #[getset2(skip)]
    skip: (),
}

impl<T: Copy + Clone + Default> Foo<T> {
    fn private(&self) -> &T {
        &self.private
    }
    fn skip(&self) {
        self.skip
    }
}

// cargo expand --example simple

```
*/

use quote::quote;

use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_error2::{abort, abort_call_site, proc_macro_error};
use syn::{
    meta::ParseNestedMeta, parse_macro_input, Attribute, DataStruct, DeriveInput, LitStr,
    Visibility,
};

use crate::generate::{GenMode, GenParams};

mod generate;

#[proc_macro_derive(Getset2, attributes(getset2))]
#[proc_macro_error]
pub fn getset2(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    produce(&ast, &new_global_gen_params_list(&ast.attrs)).into()
}

fn new_global_gen_params_list(attrs: &[Attribute]) -> Vec<GenParams> {
    let mut list = vec![];
    let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident("getset2")) else {
        return list;
    };
    let (a, skip_list) = parse_attr(attr);
    if !skip_list.is_empty() {
        abort!(
            attr,
            "The attribute of the structure do not support `skip` ident."
        )
    }
    list.extend_from_slice(&a);
    if list.iter().any(|p| p.mode == GenMode::GetCopy) {
        list.retain_mut(|p| p.mode != GenMode::GetRef);
    }
    list
}

fn parse_attr(attr: &Attribute) -> (Vec<GenParams>, HashSet<GenMode>) {
    let mut skip_list: HashSet<GenMode> = HashSet::new();
    let mut params_list = vec![];
    let mut had_ref_copy = false;
    let _ = attr.parse_nested_meta(|sub_attr| {
        match &sub_attr.path {
            p if p.is_ident("skip") => {
                skip_list.extend(parse_skip_attr(&sub_attr, attr).iter());
            }
            p if p.is_ident("get_ref") => {
                if !had_ref_copy {
                    params_list.push(new_gen_params(GenMode::GetRef, &sub_attr, attr));
                    had_ref_copy = true
                }
            }
            p if p.is_ident("get_copy") => {
                if !had_ref_copy {
                    params_list.push(new_gen_params(GenMode::GetCopy, &sub_attr, attr));
                    had_ref_copy = true
                }
            }
            p if p.is_ident("get_mut") => {
                params_list.push(new_gen_params(GenMode::GetMut, &sub_attr, attr))
            }
            p if p.is_ident("set") => {
                params_list.push(new_gen_params(GenMode::Set, &sub_attr, attr))
            }
            p if p.is_ident("set_with") => {
                params_list.push(new_gen_params(GenMode::SetWith, &sub_attr, attr))
            }
            _ => abort!(attr, "Invalid attribute {}", quote! {attr}),
        }
        Ok(())
    });
    params_list.retain(|p| !skip_list.contains(&p.mode));
    (params_list, skip_list)
}

fn parse_skip_attr(meta: &ParseNestedMeta<'_>, attr: &Attribute) -> Vec<GenMode> {
    if meta.input.is_empty() {
        return GenMode::list().to_vec();
    }
    let mut list = vec![];
    let _ = meta.parse_nested_meta(|pp| {
        match &pp.path {
            p if p.is_ident("get_ref") => list.push(GenMode::GetRef),
            p if p.is_ident("get_copy") => list.push(GenMode::GetCopy),
            p if p.is_ident("get_mut") => list.push(GenMode::GetMut),
            p if p.is_ident("set") => list.push(GenMode::Set),
            p if p.is_ident("set_with") => list.push(GenMode::SetWith),
            _ => abort!(
                attr,
                "The `skip` in the attributes is invalid {}",
                quote! {attr}
            ),
        }
        Ok(())
    });
    list
}

fn new_gen_params(gen_mode: GenMode, p: &ParseNestedMeta<'_>, attr: &Attribute) -> GenParams {
    let mut vis = None;
    let mut is_const = None;
    let mut as_ref = None;
    let mut into = None;
    let _ = p.parse_nested_meta(|pp| {
        let (_vis, _is_const, _as_ref, _into) = parse_vis_meta(&pp, attr);
        if let Some(x) = _vis {
            vis = Some(x);
        }
        if let Some(x) = _is_const {
            is_const = Some(x);
        }
        if let Some(x) = _as_ref {
            as_ref = Some(x);
        }
        if let Some(x) = _into {
            into = Some(x);
        }
        Ok(())
    });
    GenParams {
        mode: gen_mode,
        vis,
        is_const,
        as_ref,
        into,
    }
}

fn parse_vis_meta(
    p: &ParseNestedMeta<'_>,
    attr: &Attribute,
) -> (Option<Visibility>, Option<bool>, Option<bool>, Option<bool>) {
    match &p.path {
        x if x.is_ident("const") => (None, Some(true), None, None),
        x if x.is_ident("as_ref") => (None, None, Some(true), None),
        x if x.is_ident("into") => (None, None, None, Some(true)),
        x if x.is_ident("pub") => match p.value() {
            Ok(v) => match v.parse::<LitStr>() {
                Ok(vv) => (
                    Some(match vv.value().as_str() {
                        "crate" => syn::parse_str("pub(crate)").unwrap(),
                        "super" => syn::parse_str("pub(crate)").unwrap(),
                        "self" => syn::parse_str("pub(self)").unwrap(),
                        x => abort!(attr, "Invalid visibility found: pub = \"{}\"", x),
                    }),
                    None,
                    None,
                    None,
                ),
                Err(e) => abort!(attr, "Invalid visibility found1: {}", e),
            },
            Err(_e) => (Some(syn::parse_str("pub").unwrap()), None, None, None),
        },
        _ => (None, None, None, None),
    }
}

fn produce(ast: &DeriveInput, global_params_list: &[GenParams]) -> TokenStream2 {
    let name = &ast.ident;
    let generics = &ast.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Is it a struct?
    if let syn::Data::Struct(DataStruct { ref fields, .. }) = ast.data {
        let generated = fields
            .iter()
            .map(|f| generate::implement(f, global_params_list));

        quote! {
            impl #impl_generics #name #ty_generics #where_clause {
                #(#generated)*
            }
        }
    } else {
        // Nope. This is an Enum. We cannot handle these!
        abort_call_site!("#[derive(Getset2)] is only defined for structs!");
    }
}
