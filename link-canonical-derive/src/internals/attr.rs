// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use syn::{Attribute, DeriveInput, Lit, Meta, MetaNameValue, NestedMeta};

use crate::internals::case::Case;

pub const CJSON: &str = "cjson";
pub const RENAME_ALL: &str = "rename_all";

#[derive(Clone, Debug)]
pub struct Rules {
    pub casing: Option<Case>,
}

impl Rules {
    pub fn new() -> Self {
        Rules { casing: None }
    }

    pub fn from_input(input: &DeriveInput) -> Result<Self, &'static str> {
        let mut rules = Rules::new();
        let metas = input.attrs.iter().flat_map(get_meta_items);

        for meta in metas {
            match meta {
                NestedMeta::Meta(Meta::NameValue(m)) if m.path.is_ident(RENAME_ALL) => {
                    let casing = rename_all_rule(&m)?;
                    rules.casing = Some(casing);
                },
                _ => {},
            }
        }
        Ok(rules)
    }
}

pub fn get_meta_items(attr: &Attribute) -> Vec<NestedMeta> {
    if !attr.path.is_ident(CJSON) {
        return Vec::new();
    }

    match attr.parse_meta() {
        Ok(Meta::List(meta)) => meta.nested.into_iter().collect(),
        Ok(_) => {
            panic!("expected #[cjson(...)]")
        },
        Err(err) => {
            panic!("{}", err)
        },
    }
}

pub fn rename_all_rule(meta: &MetaNameValue) -> Result<Case, &'static str> {
    match &meta.lit {
        Lit::Str(casing) => casing.value().parse(),
        _ => Err("TODO"),
    }
}
