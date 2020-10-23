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

use super::AsRefspec;

pub struct Remote<Url> {
    /// The file path to the git monorepo.
    pub url: Url,
    /// Name of the remote, e.g. `"rad"`, `"origin"`.
    pub name: String,
    /// If the fetch spec is provided then the remote is created with an initial
    /// fetchspec, otherwise it is just a plain remote.
    pub fetch_spec: Option<Box<dyn AsRefspec>>,
    /// The set of push specs to add upon creation.
    pub push_specs: Vec<Box<dyn AsRefspec>>,
}

impl<Url> Remote<Url> {
    /// Create a `"rad"` remote with a single fetch spec.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::marker::PhantomData;
    /// use librad::{
    ///     git_ext as ext,
    ///     git::{
    ///         local::url::LocalUrl,
    ///         types::{remote::Remote, FlatRef, Force, NamespacedRef},
    ///     },
    ///     hash::Hash,
    ///     keys::SecretKey,
    ///     uri::{Path, Protocol, RadUrn},
    /// };
    ///
    /// let peer_id = SecretKey::new().into();
    /// let id = Hash::hash(b"geez");
    ///
    /// // The RadUrn pointing some project
    /// let urn = RadUrn::new(
    ///     id.clone(),
    ///     Protocol::Git,
    ///     Path::parse("").unwrap(),
    /// );
    ///
    /// // The working copy heads, i.e. `refs/heads/*`.
    /// let working_copy_heads: FlatRef<ext::RefLike, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // The monorepo heads, i.e. `refs/namespaces/<id>/refs/heads/*`.
    /// let monorepo_heads = NamespacedRef::heads(id, None);
    ///
    /// // Setup the fetch and push refspecs.
    /// let fetch = working_copy_heads
    ///         .clone()
    ///         .refspec(monorepo_heads.clone(), Force::True)
    ///         .boxed();
    /// let push = monorepo_heads.refspec(working_copy_heads, Force::False).boxed();
    ///
    /// // We point the remote to `LocalUrl` which will be of the form `rad://<peer id>@<id>.git`.
    /// let url = LocalUrl::from_urn(urn, peer_id);
    ///
    /// // Setup the `Remote`.
    /// let mut remote = Remote::rad_remote(url, fetch);
    /// remote.add_pushes(vec![push].into_iter());
    /// ```
    pub fn rad_remote<Ref>(url: Url, fetch_spec: Ref) -> Self
    where
        Ref: Into<Option<Box<dyn AsRefspec>>>,
    {
        Self {
            url,
            name: "rad".to_string(),
            fetch_spec: fetch_spec.into(),
            push_specs: vec![],
        }
    }

    /// Create a new `Remote` with the given `url` and `name`, while making the
    /// `fetch_spec` and `push_specs` empty.
    pub fn new(url: Url, name: String) -> Self {
        Self {
            url,
            name,
            fetch_spec: None,
            push_specs: vec![],
        }
    }

    /// Add a series of push specs to the remote.
    pub fn add_pushes<I>(&mut self, specs: I)
    where
        I: Iterator<Item = Box<dyn AsRefspec>>,
    {
        for spec in specs {
            self.push_specs.push(spec)
        }
    }

    /// Create the [`git2::Remote`] and add the specs.
    pub fn create<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Remote<'a>, git2::Error>
    where
        Url: ToString,
    {
        let _ = match &self.fetch_spec {
            Some(fetch_spec) => {
                repo.remote_with_fetch(&self.name, &self.url.to_string(), &fetch_spec.as_refspec())
            },
            None => repo.remote(&self.name, &self.url.to_string()),
        }?;

        for spec in self.push_specs.iter() {
            repo.remote_add_push(&self.name, &spec.as_refspec())?;
        }

        // To ensure that the push spec is persisted we need to call `find_remote` here.
        // Otherwise, `remote_add_push` doesn't affect the "loaded remotes".
        repo.find_remote(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use std::{io, marker::PhantomData};

    use git_ext as ext;

    use super::*;
    use crate::{
        git::{local::url::LocalUrl, types::*},
        hash::Hash,
        keys::SecretKey,
        peer::PeerId,
    };
    use librad_test::tempdir::WithTmpDir;

    #[test]
    fn can_create_remote() {
        WithTmpDir::new::<_, io::Error>(|path| {
            let repo = git2::Repository::init(path).expect("failed to init repo");

            let id = Hash::hash(b"geez");
            let heads: FlatRef<ext::RefLike, _> = FlatRef::heads(PhantomData, None);
            let namespaced_heads = NamespacedRef::heads(id, None);

            let fetch = heads
                .clone()
                .refspec(namespaced_heads.clone(), Force::True)
                .boxed();
            let push = namespaced_heads.refspec(heads, Force::False).boxed();


            let mut remote = Remote::rad_remote(path.display(), fetch);
            remote.add_pushes(vec![push].into_iter());
            let git_remote = remote.create(&repo).expect("failed to create the remote");

            assert_eq!(
                git_remote
                    .fetch_refspecs().expect("failed to get the push refspecs")
                    .iter()
                    .collect::<Vec<Option<&str>>>(),
                vec![Some("+refs/namespaces/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/refs/heads/*:refs/heads/*")],
            );

            assert_eq!(
                git_remote
                    .push_refspecs().expect("failed to get the push refspecs")
                    .iter()
                    .collect::<Vec<Option<&str>>>(),
                vec![Some("refs/heads/*:refs/namespaces/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/refs/heads/*")],
            );

            Ok(())
        }).unwrap();
    }

    #[test]
    fn check_remote_fetch_spec() -> Result<(), git2::Error> {
        let key = SecretKey::new();
        let peer_id = PeerId::from(key);
        let namespace = Hash::hash(b"meow-meow");
        let url = LocalUrl {
            repo: namespace,
            local_peer_id: peer_id,
        };
        let name = format!("lyla@{}", peer_id);
        let heads: FlatRef<PeerId, _> = FlatRef::heads(PhantomData, peer_id);
        let heads = heads.with_name(ext::RefspecPattern::try_from("heads/*").unwrap());
        let remotes: FlatRef<ext::RefLike, _> =
            FlatRef::heads(PhantomData, ext::RefLike::try_from(name.as_str()).unwrap());
        let remote = Remote {
            url,
            name: name.clone(),
            fetch_spec: Some(remotes.refspec(heads, Force::True).boxed()),
            push_specs: vec![],
        };

        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path())?;
        let remote = remote.create(&repo)?;
        let fetch_refspecs = remote.fetch_refspecs()?;

        assert_eq!(
            fetch_refspecs.iter().collect::<Vec<_>>(),
            vec![Some(
                format!("+refs/remotes/{}/heads/*:refs/remotes/{}/*", peer_id, name).as_str()
            )]
        );

        Ok(())
    }
}
