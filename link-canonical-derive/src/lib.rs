// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    parse_macro_input,
    parse_quote,
    spanned::Spanned,
    Data,
    DataEnum,
    DataStruct,
    DeriveInput,
    Fields,
    GenericParam,
    Generics,
    Ident,
    Index,
    Variant,
};

mod internals;
use internals::{attr::Rules, case};

#[proc_macro_derive(ToCjson, attributes(cjson))]
pub fn cjson_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let rules = match Rules::from_input(&input) {
        Ok(rules) => rules,
        Err(err) => panic!("{}", err),
    };

    // Used in the quasi-quotation below as `#name`.
    let name = &input.ident;

    let generics = add_trait_bounds(input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Generate an expression to sum up the heap size of each field.
    let cjson = cjson(&input.ident, &input.data, &rules);

    let expanded = quote! {
        // The generated impl.
        impl #impl_generics link_canonical::json::ToCjson for #name #ty_generics #where_clause {
            fn into_cjson(self) -> link_canonical::json::Value {
                #cjson
            }
        }
    };

    // Hand the output tokens back to the compiler.
    proc_macro::TokenStream::from(expanded)
}

// Add a bound `T: ToCjson` to every type parameter T.
fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param
                .bounds
                .push(parse_quote!(link_canonical::json::ToCjson));
        }
    }
    generics
}

fn cjson(ident: &Ident, data: &Data, rules: &Rules) -> TokenStream {
    match *data {
        Data::Struct(ref data) => cjson_struct(ident, data, rules),
        Data::Enum(ref data) => cjson_enum(ident, data),
        Data::Union(_) => unimplemented!(),
    }
}

/// Generate the `TokenStream` for a `struct` to form a
/// `link_canonical::json::Value`.
///
/// # Named Fields
///
/// If the `struct` has named fields, we first alias them using `let`
/// statements. For example, if we have `Foo { x: u64 }`, then a code block will
/// be generated that will look like:
///
/// ```rust,ignore
/// let x = self.x;
/// ```
///
/// All fields are collected to form a `BTreeMap` within a `Value::Object`. This
/// forms the inner object, which is inserted as the value of the outer object
/// with the `struct`'s name as the key. For example:
///
/// ```json
/// { "Foo": { "x": 42 } }
/// ```
///
/// # Unnamed Fields
///
/// Similar to named fields, we first alias the fields by their position. For
/// example, if we had `Foo(bool, u64)`, then the code block will look like:
///
/// ```rust,ignore
/// let __field0 = self.0;
/// let __field1 = self.1;
/// ```
///
/// All fields are collected to form a `BTreeSet` within a `Value::Array`. This
/// forms the inner object, which is inserted as the value of the outer object
/// with the `struct`'s name as the key. For example:
///
/// ```json
/// { "Foo": [true, 42] }
/// ```
///
/// # Unit Fields
///
/// These are simply output as `Value::Null`. For example, if we had a `struct
/// Foo;`, this would look like:
///
/// ```json
/// null
/// ```
fn cjson_struct(ident: &Ident, data: &DataStruct, rules: &Rules) -> TokenStream {
    match data.fields {
        Fields::Named(ref fields) => {
            let names = fields
                .named
                .iter()
                .cloned()
                .map(|field| field.ident.unwrap());
            let alias = names.clone().map(|name| {
                quote! { let #name = self.#name; }
            });
            let imp = cjson_named_fields(ident, names, rules);
            quote! {
                #(#alias)*
                #imp
            }
        },
        Fields::Unnamed(ref fields) => {
            let names = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, field)| Ident::new(&format!("__field{}", i), field.span()));
            let alias = names.clone().enumerate().map(|(i, name)| {
                let i = Index::from(i);
                quote! { let #name = self.#i; }
            });
            let imp = cjson_unnamed_fields(ident, names.clone());
            quote! {
                #(#alias)*
                #imp
            }
        },
        Fields::Unit => {
            quote! { link_canonical::json::Value::Null }
        },
    }
}

/// Generate the `TokenStream` for a `enum` to form a
/// `link_canonical::json::Value`.
///
/// # Named Fields
///
/// If the `enum` has named fields, we match on their identifiers.
/// All fields are collected to form a `BTreeMap` within a `Value::Object`. This
/// forms the inner object, which is inserted as the value of the outer object
/// with the `struct`'s name as the key. For example:
///
/// ```rust,ignore
/// enum Foo {
///   Bar { x: u64 },
///   Baz(bool),
///   Quux,
/// }
/// ```
///
/// ```json
/// { "Bar": { "x": 42 } }
/// ```
///
/// # Unnamed Fields
///
/// Similar to named fields, we match on the variant, but we need to assign the
/// fields names. These are named `__field<n>` for each successive field.
/// All fields are collected to form a `BTreeSet` within a `Value::Array`. This
/// forms the inner object, which is inserted as the value of the outer object
/// with the `struct`'s name as the key. For example:
///
/// ```rust,ignore
/// enum Foo {
///   Bar { x: u64 },
///   Baz(bool),
///   Quux,
/// }
/// ```
///
/// ```json
/// { "Baz": [true] }
/// ```
///
/// # Unit Fields
///
/// These are simply output as the name of the variant as string. For example:
///
/// ```rust,ignore
/// enum Foo {
///   Bar { x: u64 },
///   Baz(bool),
///   Quux,
/// }
/// ```
///
/// ```json
/// "Quux"
/// ```
fn cjson_enum(ident: &Ident, data: &DataEnum) -> TokenStream {
    let arms = data.variants.iter().map(|v| cjson_variant(ident, v));

    quote! { match self { #(#arms),* } }
}

fn cjson_named_fields(
    ident: &Ident,
    names: impl Iterator<Item = Ident>,
    rules: &Rules,
) -> TokenStream {
    let kvs = names.map(|name| {
        let cased = case::convert(&format!("{}", name), rules.casing);
        quote! { (#cased, link_canonical::json::ToCjson::into_cjson(#name)) }
    });
    quote! {
    use std::iter::FromIterator as _;
        let kvs = vec![#(#kvs),*].into_iter();
    let mut val = link_canonical::json::Map::new();
        let inner = link_canonical::json::Value::Object(link_canonical::json::Map::from_iter(
         kvs.map(|(key, value)| {
         (link_canonical::Cstring::from(key), value)
             })));
         val.insert(link_canonical::Cstring::from(stringify!(#ident)), inner);
         link_canonical::json::Value::Object(val)
    }
}

fn cjson_unnamed_fields(ident: &Ident, names: impl Iterator<Item = Ident>) -> TokenStream {
    let vs = names.map(|name| {
        quote! {
        link_canonical::json::ToCjson::into_cjson(#name)
        }
    });
    quote! {

                                     use std::iter::FromIterator as _;
    let mut val = link_canonical::json::Map::new();

                         let vs = vec![#(#vs),*].into_iter();
        let inner = link_canonical::json::Value::Array(link_canonical::json::Array::from_iter(vs));
    val.insert(link_canonical::Cstring::from(stringify!(#ident)), inner);
         link_canonical::json::Value::Object(val)
    }
}

fn cjson_variant(ident: &Ident, variant: &Variant) -> TokenStream {
    let name = &variant.ident;
    match &variant.fields {
        Fields::Named(ref fields) => {
            let named = fields.named.iter().cloned().map(|f| f.ident.unwrap());
            let body = cjson_named_fields(&variant.ident, named.clone(), &Rules::new());
            quote! { #ident::#name { #(#named),* } => { #body } }
        },
        Fields::Unnamed(ref fields) => {
            let named = (0..fields.unnamed.len())
                .map(|i| Ident::new(&format!("__field{}", i), Span::call_site()));
            let body = cjson_unnamed_fields(&variant.ident, named.clone());
            quote! { #ident::#name ( #(#named),* ) => { #body } }
        },
        Fields::Unit => {
            quote! {
            #ident::#name => link_canonical::json::Value::String(link_canonical::Cstring::from(
                stringify!(#name),
            ))
            }
        },
    }
}
