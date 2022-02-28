// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, convert::TryFrom as _, fmt};

use thiserror::Error;

use librad::{
    git::{
        refs::{self, Refs},
        storage::{self, glob, ReadOnly, ReadOnlyStorage},
        types::Namespace,
        Urn,
    },
    git_ext::{self as ext, reference, OneLevel},
    reflike,
    refspec_pattern,
    PeerId,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Storage(#[from] storage::Error),
}

pub fn heads<S, P>(
    storage: &S,
    urn: &Urn,
    peer: P,
) -> Result<Option<BTreeMap<OneLevel, ext::Oid>>, Error>
where
    S: AsRef<ReadOnly>,
    P: Into<Option<PeerId>> + fmt::Debug,
{
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.heads().collect()))
}

pub fn tags<S, P>(
    storage: &S,
    urn: &Urn,
    peer: P,
) -> Result<Option<BTreeMap<OneLevel, ext::Oid>>, Error>
where
    S: AsRef<ReadOnly>,
    P: Into<Option<PeerId>> + fmt::Debug,
{
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.tags().collect()))
}

pub fn notes<S, P>(
    storage: &S,
    urn: &Urn,
    peer: P,
) -> Result<Option<BTreeMap<OneLevel, ext::Oid>>, Error>
where
    S: AsRef<ReadOnly>,
    P: Into<Option<PeerId>> + fmt::Debug,
{
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.notes().collect()))
}

/// List all reference names and Oids under
/// `refs/namespaces/<namespace>/{remotes/<peer>/}refs/<category>/*`
pub fn category<S, P>(
    storage: &S,
    urn: &Urn,
    peer: P,
    category: ext::RefLike,
) -> Result<BTreeMap<OneLevel, ext::Oid>, Error>
where
    S: AsRef<ReadOnly>,
    P: Into<Option<PeerId>>,
{
    let peer = peer.into();
    let namespace = Namespace::from(urn);
    let namespace_prefix = format!("refs/namespaces/{}/", namespace);

    let peeled = |head: Result<git2::Reference, _>| -> Option<(String, git2::Oid)> {
        head.ok().and_then(reference::peeled)
    };
    let refined =
        |(name, oid): (String, git2::Oid)| -> Result<(OneLevel, ext::Oid), refs::stored::Error> {
            Ok(reference::refined((
                name.strip_prefix(&namespace_prefix).unwrap_or(&name),
                oid,
            ))?)
        };

    let namespace_prefix =
        ext::RefLike::try_from(namespace_prefix.clone()).map_err(refs::stored::Error::from)?;
    let reference = match peer {
        None => namespace_prefix,
        Some(peer) => namespace_prefix.join(reflike!("refs/remotes")).join(peer),
    };
    let reference = reference
        .join(reflike!("refs"))
        .join(category)
        .with_pattern_suffix(refspec_pattern!("*"));
    Ok(storage
        .as_ref()
        .references_glob(glob::RefspecMatcher::from(reference))?
        .filter_map(peeled)
        .map(refined)
        .collect::<Result<_, _>>()?)
}
