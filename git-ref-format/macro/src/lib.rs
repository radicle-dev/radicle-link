// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[macro_use]
extern crate proc_macro_error;

use std::convert::TryInto;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, LitStr};

use git_ref_format_core::{refspec::PatternStr, Component, Error, RefStr};

/// Create a [`git_ref_format_core::RefString`] from a string literal.
///
/// The string is validated at compile time, and an unsafe conversion is
/// emitted.
#[proc_macro_error]
#[proc_macro]
pub fn refname(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let val = lit.value();

    let parsed: Result<&RefStr, Error> = val.as_str().try_into();
    match parsed {
        Ok(safe) => {
            let safe: &str = safe.as_str();
            let expand = quote! {
                unsafe {
                    use ::std::mem::transmute;
                    use ::git_ref_format::RefString;

                    transmute::<_, RefString>(#safe.to_owned())
                }
            };
            TokenStream::from(expand)
        },

        Err(e) => {
            abort!(lit.span(), "invalid refname literal: {}", e);
        },
    }
}

/// Create a [`git_ref_format_core::Component`] from a string literal.
///
/// The string is validated at compile time, and an unsafe conversion is
/// emitted.
#[proc_macro_error]
#[proc_macro]
pub fn component(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let val = lit.value();

    let name: Result<&RefStr, Error> = val.as_str().try_into();
    match name {
        Ok(name) => {
            let comp: Option<Component> = name.into();
            match comp {
                Some(safe) => {
                    let safe: &str = safe.as_ref().as_str();
                    let expand = quote! {
                        unsafe {
                            use ::std::{borrow::Cow, mem::transmute};
                            use ::git_ref_format::{Component, RefStr, RefString};

                            let inner: RefString = transmute(#safe.to_owned());
                            let cow: Cow<'static, RefStr> = Cow::Owned(inner);
                            transmute::<_, Component>(cow)
                        }
                    };

                    TokenStream::from(expand)
                },

                None => {
                    abort!(lit.span(), "component contains a '/'");
                },
            }
        },

        Err(e) => {
            abort!(lit.span(), "invalid refname literal: {}", e);
        },
    }
}

/// Create a [`git_ref_format_core::refspec::PatternString`] from a string
/// literal.
///
/// The string is validated at compile time, and an unsafe conversion is
/// emitted.
#[proc_macro_error]
#[proc_macro]
pub fn pattern(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let val = lit.value();

    let parsed: Result<&PatternStr, Error> = val.as_str().try_into();
    match parsed {
        Ok(safe) => {
            let safe: &str = safe.as_str();
            let expand = quote! {
                unsafe {
                    use ::std::mem::transmute;
                    use ::git_ref_format::refspec::PatternString;

                    transmute::<_, PatternString>(#safe.to_owned())
                }
            };
            TokenStream::from(expand)
        },

        Err(e) => {
            abort!(lit.span(), "invalid refspec pattern literal: {}", e);
        },
    }
}
