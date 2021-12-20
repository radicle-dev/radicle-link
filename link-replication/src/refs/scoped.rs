// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Ref rewriting utilities.
//!
//! Note that this is an internal API, exported mainly for testing. In
//! particular, ref name parameters are generally expected to be pre-validated
//! in some way, and should never be empty.

use std::{borrow::Cow, ops::Deref};

use bstr::{BStr, BString, ByteVec as _};
use either::{
    Either,
    Either::{Left, Right},
};
use link_crypto::PeerId;

use super::{is_separator, Prefix};

/// A ref which optionally is relative to a namespace.
///
/// The fully qualified name can be obtained lazily using
/// [`Namespaced::qualified`].
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

/// Ensure that the ref `name` is a remote tracking branch.
///
/// If `name` starts with `refs/remotes/`, this is the identity function.
/// Otherwise, `refs/remotes/<remote_id>/` is prepended. This will handle `name`
/// being prefixed by `refs/`; eg. `refs/heads/main` will be rewritten to
///
///     refs/remotes/<remote_id>/heads/main
///
/// not
///     refs/remotes/<remote_id>/refs/heads/main
///
/// Note that if `name` is not prefixed, it is inserted verbatim. Thus it must
/// still include the category (ie. `heads/main`, not `main`).
pub fn remote_tracking<'a>(
    remote_id: &PeerId,
    name: impl Into<Cow<'a, BStr>>,
) -> RemoteTracking<'a> {
    use super::component::REFS;

    let mut name = name.into();
    if !name.starts_with(Prefix::Remotes.as_bytes()) {
        let name = name.to_mut();
        if name.starts_with(REFS) {
            name.insert_str(REFS.len() + 1, format!("remotes/{}/", remote_id));
        } else {
            name.insert_str(0, format!("refs/remotes/{}/", remote_id));
        }
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

/// Ensure that `name` is not a remote tracking branch.
///
/// Essentially removes `refs/remotes/*/` from `name`. Returns `None` if the
/// result would be the empty string.
pub fn owned<'a>(name: impl Into<Cow<'a, BStr>>) -> Option<Owned<'a>> {
    use super::component::*;

    let name = name.into();
    match name.splitn(4, is_separator).collect::<Vec<_>>()[..] {
        [REFS, REMOTES, _, rest] => (!rest.is_empty()).then(|| {
            let mut name = BString::from(REFS);
            name.insert_char(REFS.len(), '/');
            name.insert_str(REFS.len() + 1, rest);
            Owned(name.into())
        }),
        [REFS, REMOTES] | [REFS, REMOTES, _] => None,
        _ => Some(Owned(name)),
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

/// Conditionally ensure `name` is either a remote tracking branch or not.
///
/// If the `wanted_id` is equal to the `remote_id`, the result is not a remote
/// tracking branch, otherwise it is. For example, given the name:
///
///     refs/heads/main
///
/// If `wanted_id == remote_id`, the result is:
///
///     refs/heads/main
///
/// Otherwise
///
///     refs/remotes/<wanted_id>/heads/main
///
/// This is used to determine the right 'scope' of a ref when fetching from
/// `remote_id`. `name` should generally not be a remote tracking branch itself,
/// as that information is stripped.
pub fn scoped<'a, 'b>(
    wanted_id: &'a PeerId,
    remote_id: &PeerId,
    name: impl Into<Cow<'b, BStr>>,
) -> Scoped<'a, 'b> {
    let own = owned(name).expect("BUG: `scoped` should receive valid remote tracking branches");
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
