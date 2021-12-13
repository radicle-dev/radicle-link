// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[macro_use]
extern crate proc_macro_error;

use std::convert::TryFrom;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, LitStr};

use radicle_git_ext::reference::name::{RefLike, RefspecPattern};

/// Create `RefLike` from a string literal.
///
/// The string is validated at compile time, and an unsafe conversion is
/// emitted.
///
/// ```rust
/// use radicle_macros::reflike;
///
/// assert_eq!("lolek/bolek", reflike!("lolek/bolek").as_str())
/// ```
#[proc_macro_error]
#[proc_macro]
pub fn reflike(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);

    match RefLike::try_from(lit.value()) {
        Ok(safe) => {
            let safe: &str = &*safe;
            let expand = quote! { unsafe { ::std::mem::transmute::<_, ::radicle_git_ext::RefLike>(#safe.to_owned()) }};
            TokenStream::from(expand)
        },

        Err(e) => {
            abort!(lit.span(), "invalid RefLike literal: {}", e);
        },
    }
}

/// Create a `RefspecPattern` from a string literal.
///
/// The string is validated at compile time, and an unsafe conversion is
/// emitted.
///
/// ```rust
/// use radicle_macros::refspec_pattern;
///
/// assert_eq!("refs/heads/*", refspec_pattern!("refs/heads/*").as_str())
/// ```
#[proc_macro_error]
#[proc_macro]
pub fn refspec_pattern(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);

    match RefspecPattern::try_from(lit.value()) {
        Ok(safe) => {
            let safe: &str = &*safe;
            let expand = quote! { unsafe { ::std::mem::transmute::<_, ::radicle_git_ext::RefspecPattern>(#safe.to_owned()) }};
            TokenStream::from(expand)
        },

        Err(e) => {
            abort!(lit.span(), "invalid RefspecPattern literal: {}", e);
        },
    }
}
