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
    git_ext::{self as ext, OneLevel, RefLike},
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
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.heads))
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
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.tags))
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
    Ok(Refs::load(storage, urn, peer)?.map(|refs| refs.notes))
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
    let namespace_prefix = reflike!("refs/namespaces").join(namespace);

    // TODO(finto): this is cribbed from refs, maybe we could deduplicate
    fn peeled(r: Result<git2::Reference, storage::Error>) -> Option<(String, git2::Oid)> {
        r.ok().and_then(|head| {
            head.name()
                .and_then(|name| head.target().map(|target| (name.to_owned(), target)))
        })
    }

    let refined =
        |(name, oid): (String, git2::Oid)| -> Result<(OneLevel, ext::Oid), refs::stored::Error> {
            let name = RefLike::try_from(
                name.strip_prefix(namespace_prefix.as_str())
                    .unwrap_or(&name),
            )?;
            Ok((OneLevel::from(name), oid.into()))
        };

    let reference = match peer {
        None => namespace_prefix.clone(),
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
