// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use bstr::BString;
use link_git::protocol::{ObjectId, Ref};

mod lit;
pub use lit::{component, Prefix, RadId, RadSelf, Signed};

pub mod parsed;
pub use parsed::{parse, Parsed};

mod scoped;
pub use scoped::{owned, remote_tracking, scoped, Namespaced, Scoped};

pub const SEPARATOR: u8 = b'/';

pub fn is_separator(x: &u8) -> bool {
    x == &SEPARATOR
}

pub fn into_unpacked(r: Ref) -> (BString, ObjectId) {
    match r {
        Ref::Direct { path, object, .. }
        | Ref::Peeled {
            path, tag: object, ..
        }
        | Ref::Symbolic { path, object, .. } => (path, object),
    }
}
