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

use std::ops::Range;

use git_ext::{is_exists_err, is_not_found_err, RefLike};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use crate::{git::p2p::url::GitUrlRef, peer::PeerId, signer::Signer};

use super::storage2::Storage;

pub use crate::identities::git::Urn;

#[derive(Debug, Error)]
pub enum Error {
    #[error("can't track oneself")]
    SelfReferential,

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub fn track<S>(storage: &Storage<S>, urn: &Urn, peer: PeerId) -> Result<bool, Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let local_peer = storage.peer_id();

    if &peer == local_peer {
        return Err(Error::SelfReferential);
    }

    let remote_name = tracking_remote_name(urn, &peer);
    let url = GitUrlRef::from_urn(urn, local_peer, &peer, &[]);

    let was_created = storage
        .as_raw()
        .remote(&remote_name, &url.to_string())
        .map(|_| true)
        .or_matches::<Error, _, _>(is_exists_err, || Ok(false))?;

    if was_created {
        // Remove default fetchspec, as it is almost always invalid (we compute
        // the fetchspecs ourselves). We also don't want libgit2 to prune the
        // remote.
        // FIXME: go through `&mut storage2::Config`
        let mut config = storage.as_raw().config()?;
        config.remove_multivar(&remote_name, ".*")?;
    }

    Ok(was_created)
}

pub fn untrack<S>(storage: &Storage<S>, urn: &Urn, peer: PeerId) -> Result<bool, Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let remote_name = tracking_remote_name(urn, &peer);
    let was_removed = storage
        .as_raw()
        .remote_delete(&remote_name)
        .map(|()| true)
        .or_matches::<Error, _, _>(is_not_found_err, || Ok(false))?;

    // Prune all remote branches
    // FIXME: proper globbing in storage
    let prune = storage.as_raw().references_glob(
        reflike!("refs/namespaces")
            .join(urn)
            .join(reflike!("refs/remotes"))
            .join(peer)
            .with_pattern_suffix(refspec_pattern!("*"))
            .as_str(),
    )?;

    for branch in prune {
        branch?.delete()?;
    }

    Ok(was_removed)
}

pub fn tracked<S>(storage: &Storage<S>, urn: &Urn) -> Result<Tracked, Error>
where
    S: Signer,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    Ok(Tracked::collect(storage.as_raw(), urn)?)
}

/// Iterator over the 1st degree tracked peers.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Tracked {
    remotes: git2::string_array::StringArray,
    range: Range<usize>,
    prefix: String,
}

impl Tracked {
    fn collect(repo: &git2::Repository, context: &Urn) -> Result<Self, git2::Error> {
        let remotes = repo.remotes()?;
        let range = 0..remotes.len();
        let prefix = format!("{}/", RefLike::from(context));
        Ok(Self {
            remotes,
            range,
            prefix,
        })
    }
}

impl Iterator for Tracked {
    type Item = PeerId;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.range.next().and_then(|i| self.remotes.get(i));
        match next {
            None => None,
            Some(name) => {
                let peer = name
                    .strip_prefix(&self.prefix)
                    .and_then(|peer| peer.parse().ok());
                peer.or_else(|| self.next())
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

fn tracking_remote_name(urn: &Urn, peer: &PeerId) -> String {
    format!("{}/{}", RefLike::from(urn), peer)
}
