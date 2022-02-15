// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use bstr::BString;
use git_ref_format::RefString;
use link_crypto::PeerId;
use link_git::protocol::{ObjectId, Ref};

use crate::ids::Urn;

pub use git_ref_format::Qualified;

mod lit;
pub use lit::*;

pub mod parsed;
pub use parsed::{parse, Parsed};

mod scoped;
pub use scoped::{
    namespaced,
    owned,
    remote_tracking,
    scoped,
    Namespaced,
    Owned,
    RemoteTracking,
    Scoped,
};

pub fn into_unpacked(r: Ref) -> (BString, ObjectId) {
    match r {
        Ref::Direct { path, object, .. }
        | Ref::Peeled {
            path, tag: object, ..
        }
        | Ref::Symbolic { path, object, .. } => (path, object),
    }
}

pub fn from_peer_id(p: &PeerId) -> RefString {
    RefString::try_from(p.default_encoding()).expect("peer id is a valid refname")
}

pub fn from_urn<U: Urn>(urn: &U) -> RefString {
    RefString::try_from(urn.encode_id()).expect("urn is a valid refname")
}

pub fn rad_ids<U: Urn>(urn: &U) -> Qualified<'_> {
    (lit::Refs, lit::Rad, lit::Ids, from_urn(urn).head()).into()
}
