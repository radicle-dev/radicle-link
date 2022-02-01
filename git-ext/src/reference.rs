// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

pub mod check;

mod iter;
pub use iter::{ReferenceNames, References};

pub mod name;
pub use name::{OneLevel, Qualified, RefLike, RefspecPattern};

pub fn peeled(head: git2::Reference) -> Option<(String, git2::Oid)> {
    head.name()
        .and_then(|name| head.target().map(|target| (name.to_owned(), target)))
}

pub fn refined((name, oid): (&str, git2::Oid)) -> Result<(OneLevel, crate::Oid), name::Error> {
    let name = RefLike::try_from(name)?;
    Ok((OneLevel::from(name), oid.into()))
}
