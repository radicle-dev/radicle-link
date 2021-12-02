// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, ops::Deref};

use bstr::{BStr, BString, ByteVec as _};
use either::{
    Either,
    Either::{Left, Right},
};
use link_crypto::PeerId;

use super::{is_separator, Prefix};

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Namespaced<'a> {
    pub namespace: Option<Cow<'a, BStr>>,
    pub refname: Cow<'a, BStr>,
}

impl Namespaced<'_> {
    pub fn qualified(&self) -> BString {
        const PREFIX: &str = "refs/namespaces/";

        let mut name = BString::from(self.refname.as_ref());
        if let Some(ns) = &self.namespace {
            name.insert_str(0, PREFIX);
            name.insert_str(PREFIX.len(), ns.as_ref());
            name.insert_char(PREFIX.len() + ns.len(), '/');
        }
        name
    }

    pub fn into_owned(self) -> Namespaced<'static> {
        Namespaced {
            namespace: self.namespace.map(|ns| Cow::Owned(ns.into_owned())),
            refname: Cow::Owned(self.refname.into_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct RemoteTracking<'a>(Cow<'a, BStr>);

pub fn remote_tracking<'a>(
    remote_id: &PeerId,
    name: impl Into<Cow<'a, BStr>>,
) -> RemoteTracking<'a> {
    let mut name = name.into();
    if !name.starts_with(Prefix::Remotes.as_bytes()) {
        let name = name.to_mut();
        if name.starts_with(b"refs/") {
            name.drain(0.."refs/".len());
        }
        name.insert_str(0, format!("refs/remotes/{}/", remote_id))
    }
    RemoteTracking(name)
}

impl Deref for RemoteTracking<'_> {
    type Target = BStr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<BStr> for RemoteTracking<'_> {
    fn as_ref(&self) -> &BStr {
        self
    }
}

impl<'a> From<RemoteTracking<'a>> for Cow<'a, BStr> {
    fn from(rt: RemoteTracking<'a>) -> Self {
        rt.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Owned<'a>(Cow<'a, BStr>);

pub fn owned<'a>(name: impl Into<Cow<'a, BStr>>) -> Owned<'a> {
    use super::component::*;

    let name = name.into();
    match name.splitn(4, is_separator).collect::<Vec<_>>()[..] {
        [REFS, REMOTES, _, rest] => {
            let mut name = BString::from(REFS);
            name.insert_char(REFS.len(), '/');
            name.insert_str(REFS.len() + 1, rest);
            Owned(name.into())
        },
        _ => Owned(name),
    }
}

impl Deref for Owned<'_> {
    type Target = BStr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<BStr> for Owned<'_> {
    fn as_ref(&self) -> &BStr {
        self
    }
}

impl<'a> From<Owned<'a>> for Cow<'a, BStr> {
    fn from(o: Owned<'a>) -> Self {
        o.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Scoped<'a, 'b> {
    scope: &'a PeerId,
    name: Either<Owned<'b>, RemoteTracking<'b>>,
}

pub fn scoped<'a, 'b>(
    wanted_id: &'a PeerId,
    remote_id: &PeerId,
    name: impl Into<Cow<'b, BStr>>,
) -> Scoped<'a, 'b> {
    let own = owned(name);
    Scoped {
        scope: wanted_id,
        name: if wanted_id == remote_id {
            Left(own)
        } else {
            Right(remote_tracking(wanted_id, own))
        },
    }
}

impl AsRef<BStr> for Scoped<'_, '_> {
    fn as_ref(&self) -> &BStr {
        self.name.as_ref().either(AsRef::as_ref, AsRef::as_ref)
    }
}

impl<'b> From<Scoped<'_, 'b>> for Cow<'b, BStr> {
    fn from(s: Scoped<'_, 'b>) -> Self {
        s.name.either(Into::into, Into::into)
    }
}
