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

use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::{
    git::{
        p2p::url::GitUrl,
        refs::Refs,
        types::{AsRefspec, Force, Reference, Refspec},
    },
    hash::Hash,
    peer::PeerId,
    uri::RadUrn,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub struct Fetcher<'a> {
    url: GitUrl<Hash>,
    remote: git2::Remote<'a>,
}

impl<'a> Fetcher<'a> {
    pub fn new(repo: &'a git2::Repository, url: GitUrl<Hash>) -> Result<Self, Error> {
        let mut remote = repo.remote_anonymous(&url.to_string())?;
        remote.connect(git2::Direction::Fetch)?;

        Ok(Self { url, remote })
    }

    pub fn url(&self) -> &GitUrl<Hash> {
        &self.url
    }

    /// Prefetch any refs relevant for authenticating the remote repo.
    ///
    /// Only relevant for cloning: we want to verify the remote's view of the
    /// repo before attempting to fetch potentially large amounts of data.
    pub fn prefetch(&mut self) -> Result<(), Error> {
        tracing::debug!("Prefetching {}", self.url);

        let namespace = &self.url.repo;
        let remote_peer = self.url.remote_peer;

        let remote_id = Reference::rad_id(namespace.clone());
        let remote_self = Reference::rad_self(namespace.clone(), None);
        let remote_certifiers = Reference::rad_ids_glob(namespace.clone());

        // `refs/namespaces/<namespace>/refs/rad/id \
        // :refs/namespaces/<namespace>/refs/remotes/<remote_peer>/rad/id`
        //
        // `refs/namespaces/<namespace>/refs/rad/self \
        // :refs/namespaces/<namespace>/refs/remotes/<remote_peer>/rad/self`
        //
        // `refs/namespaces/<namespace>/refs/rad/ids/* \
        // :refs/namespaces/<namespace>/refs/remotes/<remote_peer>/rad/ids/*`
        let refspecs = [
            remote_id
                .set_remote(remote_peer)
                .refspec(remote_id, Force::False)
                .boxed(),
            remote_self
                .set_remote(remote_peer)
                .refspec(remote_self, Force::False)
                .boxed(),
            remote_certifiers
                .set_remote(remote_peer)
                .refspec(remote_certifiers, Force::False)
                .boxed(),
        ]
        .iter()
        .map(|spec| spec.as_refspec())
        .collect::<Vec<String>>();

        tracing::trace!(repo.clone.refspecs = ?refspecs);
        {
            let mut fetch_options = self.fetch_options();
            self.remote
                .fetch(&refspecs, Some(&mut fetch_options), None)?;
        }

        Ok(())
    }

    /// Fetch remote heads according to the remote's signed `rad/signed_refs`
    /// branch.
    ///
    /// Proceeds in three stages:
    ///
    /// 1. fetch the remote's view of `rad/signed_refs`
    /// 2. compare the signed refs against the advertised ones
    /// 3. fetch advertised refs ⋂ signed refs
    pub fn fetch<F, G, E>(
        &mut self,
        transitively_tracked: HashSet<PeerId>,
        rad_signed_refs_of: F,
        certifiers_of: G,
    ) -> Result<(), E>
    where
        F: Fn(PeerId) -> Result<Refs, E>,
        G: Fn(PeerId) -> Result<HashSet<RadUrn>, E>,
        E: From<git2::Error>,
    {
        let namespace = &self.url.repo;
        let remote_peer = self.url.remote_peer;

        let mut fetch_opts = self.fetch_options();

        // Fetch `rad/signed_refs` first
        {
            let refspecs = Refspec::rad_signed_refs(
                self.url.repo.clone(),
                remote_peer,
                transitively_tracked.clone().into_iter(),
            )
            .map(|spec| spec.as_refspec())
            .collect::<Vec<String>>();

            tracing::debug!(refspecs = ?refspecs, "Fetching rad/refs");

            self.remote.fetch(&refspecs, Some(&mut fetch_opts), None)?;
        }

        // Calculate the fetch heads based on the signed `rad/refs` -- any
        // advertised ref which doesn't match the signed value is simply
        // skipped. Note that we're currently limited by libgit2 managing the
        // refs advertisement (and lack of protocol v2 support): with a tad bit
        // more control over the fetch procedure, we could attempt to fetch the
        // refs exactly at the signed oids.
        {
            let remote_heads: HashMap<String, git2::Oid> = self
                .remote
                .list()?
                .iter()
                .map(|rhead| (rhead.name().to_owned(), rhead.oid()))
                .collect();

            let refspecs = Refspec::fetch_heads(
                namespace.clone(),
                remote_heads,
                transitively_tracked.into_iter(),
                remote_peer,
                rad_signed_refs_of,
                certifiers_of,
            )?
            .map(|spec| spec.as_refspec())
            .collect::<Vec<String>>();

            tracing::debug!(refspecs = ?refspecs, "Fetching refs/heads");
            self.remote.fetch(&refspecs, Some(&mut fetch_opts), None)?;
        }

        Ok(())
    }

    // TODO: allow users to supply callbacks
    fn fetch_options(&self) -> git2::FetchOptions<'a> {
        let mut cbs = git2::RemoteCallbacks::new();
        cbs.transfer_progress(|prog| {
            tracing::trace!("Fetch: received {} bytes", prog.received_bytes());
            true
        })
        .update_tips(|name, old, new| {
            tracing::debug!("Fetch: updating tip {}: {} -> {}", name, old, new);
            true
        });

        let mut fos = git2::FetchOptions::new();
        fos.prune(git2::FetchPrune::Off)
            .update_fetchhead(false)
            .download_tags(git2::AutotagOption::None)
            .remote_callbacks(cbs);

        fos
    }
}
