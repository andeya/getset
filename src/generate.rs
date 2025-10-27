use crate::parse_attr;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use proc_macro_error2::abort;
use quote::quote;
use std::collections::HashSet;
use syn::{
    self, ext::IdentExt, spanned::Spanned, token::Const, Field, GenericArgument, PathArguments,
    Type, TypePath, Visibility,
};

use self::GenMode::{GetCopy, GetMut, GetRef, Set, SetWith};

#[derive(Clone)]
pub struct GenParams {
    pub mode: GenMode,
    pub vis: Option<Visibility>,
    pub is_const: Option<bool>,
    pub as_ref: Option<bool>,
    pub into: Option<bool>,
}

#[derive(PartialEq, Eq, Hash, Copy, Clone)]
pub enum GenMode {
    GetRef,
    GetCopy,
    GetMut,
    Set,
    SetWith,
}

impl GenMode {
    pub fn list() -> [GenMode; 5] {
        [
            GenMode::GetRef,
            GenMode::GetCopy,
            GenMode::GetMut,
            GenMode::Set,
            GenMode::SetWith,
        ]
    }

    pub fn prefix(self) -> &'static str {
        match self {
            GetRef | GetCopy | GetMut => "",
            Set => "set_",
            SetWith => "with_",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            GetRef | GetCopy | Set | SetWith => "",
            GetMut => "_mut",
        }
    }
}

pub fn implement(field: &Field, global_params: &[GenParams]) -> TokenStream2 {
    let mut ts = TokenStream2::new();
    let (mut params_list, skip_list) = if let Some(attr) = field
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("getset2"))
    {
        parse_attr(attr)
    } else {
        (global_params.to_vec(), HashSet::new())
    };
    let had_ref_copy = params_list
        .iter()
        .any(|p| matches!(p.mode, GenMode::GetRef | GenMode::GetCopy));
    for params in global_params {
        if (!skip_list.contains(&params.mode) && params_list.iter().all(|p| p.mode != params.mode))
            && (!had_ref_copy || !matches!(params.mode, GenMode::GetRef | GenMode::GetCopy))
        {
            params_list.push(params.clone());
        }
    }
    for mut params in params_list {
        params.vis = params.vis.or_else(|| Some(field.vis.clone()));
        ts.extend(gen_method(field, params));
    }
    ts
}

pub fn gen_method(field: &Field, params: GenParams) -> TokenStream2 {
    let field_name = field
        .ident
        .clone()
        .unwrap_or_else(|| abort!(field.span(), "Expected the field to have a name"));

    let fn_name = if params.mode.prefix().is_empty() && params.mode.suffix().is_empty() {
        field_name.clone()
    } else {
        Ident::new(
            &format!(
                "{}{}{}",
                params.mode.prefix(),
                field_name.unraw(),
                params.mode.suffix()
            ),
            Span::call_site(),
        )
    };
    let ty = field.ty.clone();

    let doc = field.attrs.iter().filter(|v| v.meta.path().is_ident("doc"));

    let visibility = params.vis;
    let const_kw: Option<Const> = params.is_const.and_then(|is_const| {
        if is_const {
            Some(Const::default())
        } else {
            None
        }
    });

    match params.mode {
        GenMode::GetRef if params.as_ref == Some(true) && is_option_or_result_type(&ty) => {
            let return_ty = get_as_ref_return_type(&ty);
            quote! {
                #(#doc)*
                #[inline(always)]
                #visibility #const_kw fn #fn_name(&self) -> #return_ty {
                    self.#field_name.as_ref()
                }
            }
        }
        GenMode::GetRef => {
            quote! {
                #(#doc)*
                #[inline(always)]
                #visibility #const_kw fn #fn_name(&self) -> &#ty {
                    &self.#field_name
                }
            }
        }
        GenMode::GetCopy => {
            quote! {
                #(#doc)*
                #[inline(always)]
                #visibility #const_kw fn #fn_name(&self) -> #ty {
                    self.#field_name
                }
            }
        }
        GenMode::GetMut => {
            quote! {
                #(#doc)*
                #[inline(always)]
                #visibility #const_kw fn #fn_name(&mut self) -> &mut #ty {
                    &mut self.#field_name
                }
            }
        }
        GenMode::Set => {
            if params.into == Some(true) {
                quote! {
                    #(#doc)*
                    #[inline(always)]
                    #visibility #const_kw fn #fn_name<I: Into<#ty>>(&mut self, val: I) -> &mut Self {
                        let val = val.into();
                        self.#field_name = val;
                        self
                    }
                }
            } else {
                quote! {
                    #(#doc)*
                    #[inline(always)]
                    #visibility #const_kw fn #fn_name(&mut self, val: #ty) -> &mut Self {
                        self.#field_name = val;
                        self
                    }
                }
            }
        }
        GenMode::SetWith => {
            if params.into == Some(true) {
                quote! {
                    #(#doc)*
                    #[inline(always)]
                    #visibility #const_kw fn #fn_name<I: Into<#ty>>(mut self, val: I) -> Self {
                        let val = val.into();
                        self.#field_name = val;
                        self
                    }
                }
            } else {
                quote! {
                    #(#doc)*
                    #[inline(always)]
                    #visibility #const_kw fn #fn_name(mut self, val: #ty) -> Self {
                        self.#field_name = val;
                        self
                    }
                }
            }
        }
    }
}

/// Check if the type is Option<T> or Result<T, E>
fn is_option_or_result_type(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        // Check for std::option::Option or std::result::Result
        let segments: Vec<_> = path.segments.iter().collect();
        if segments.len() == 3 {
            // Allow for std::option::Option or std::result::Result
            if segments[0].ident != "std" && segments[0].ident != "core" {
                return false;
            }

            if segments[1].ident == "option" && segments[2].ident == "Option" {
                return true;
            }
            if segments[1].ident == "result" && segments[2].ident == "Result" {
                return true;
            }
        } else if segments.len() == 1 {
            // Allow for direct imports: Option or Result
            if segments[0].ident == "Option" || segments[0].ident == "Result" {
                return true;
            }
        }
    }
    false
}

/// Generate the return type for as_ref() call
/// Option<T> -> Option<&T>
/// Result<T, E> -> Result<&T, &E>
fn get_as_ref_return_type(ty: &Type) -> TokenStream2 {
    if let Some(ts) = as_ref_option_type(ty) {
        return ts;
    }
    if let Some(ts) = as_ref_result_type(ty) {
        return ts;
    }
    // Fallback to original type reference if parsing fails
    quote! { &#ty }
}

fn as_ref_option_type(ty: &Type) -> Option<TokenStream2> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if segment.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(GenericArgument::Type(inner_ty)) = args.args.first() {
                        return Some(quote! { Option<&#inner_ty> });
                    }
                }
            }
        }
    }
    None
}

fn as_ref_result_type(ty: &Type) -> Option<TokenStream2> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if segment.ident == "Result" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    let args_vec: Vec<_> = args.args.iter().collect();
                    if args_vec.len() >= 2 {
                        if let (GenericArgument::Type(ok_ty), GenericArgument::Type(err_ty)) =
                            (&args_vec[0], &args_vec[1])
                        {
                            return Some(quote! { Result<&#ok_ty, &#err_ty> });
                        }
                    }
                }
            }
        }
    }
    None
}
