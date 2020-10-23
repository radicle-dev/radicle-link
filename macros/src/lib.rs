// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#![feature(proc_macro_diagnostic)]

use std::convert::TryFrom;

use proc_macro::TokenStream;
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
#[proc_macro]
pub fn reflike(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);

    match RefLike::try_from(lit.value()) {
        Ok(safe) => {
            let safe_str = safe.as_str();
            let expand = quote! {
                unsafe {
                    ::std::mem::transmute::<::std::path::PathBuf, ::radicle_git_ext::RefLike>(
                        ::std::convert::From::from(#safe_str)
                    )
                }
            };

            TokenStream::from(expand)
        },

        Err(e) => {
            lit.span()
                .unwrap()
                .error(format!("invalid RefLike literal: {}", e))
                .emit();

            TokenStream::from(quote! { unimplemented!() })
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
#[proc_macro]
pub fn refspec_pattern(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);

    match RefspecPattern::try_from(lit.value()) {
        Ok(safe) => {
            let safe_str = safe.as_str();
            let expand = quote! {
                unsafe {
                    ::std::mem::transmute::<::std::path::PathBuf, ::radicle_git_ext::RefspecPattern>(
                        ::std::convert::From::from(#safe_str)
                    )
                }
            };

            TokenStream::from(expand)
        },

        Err(e) => {
            lit.span()
                .unwrap()
                .error(format!("invalid RefspecPattern literal: {}", e))
                .emit();

            TokenStream::from(quote! { unimplemented!() })
        },
    }
}
