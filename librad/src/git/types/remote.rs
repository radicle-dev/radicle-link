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
    /// The set of fetch specs to add upon creation.
    pub fetch_spec: Box<dyn AsRefspec>,
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
    ///     git::{
    ///         local::url::LocalUrl,
    ///         types::{remote::Remote, FlatRef, Force, NamespacedRef},
    ///     },
    ///     hash::Hash,
    ///     uri::{Path, Protocol, RadUrn},
    /// };
    ///
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
    /// let working_copy_heads: FlatRef<String, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // The monorepo heads, i.e. `refs/namespaces/<id>/refs/heads/*`.
    /// let monorepo_heads = NamespacedRef::heads(id, None);
    ///
    /// // Setup the fetch and push refspecs.
    /// let fetch = working_copy_heads
    ///         .clone()
    ///         .refspec(monorepo_heads.clone(), Force::True)
    ///         .into_dyn();
    /// let push = monorepo_heads.refspec(working_copy_heads, Force::False).into_dyn();
    ///
    /// // We point the remote to `LocalUrl` which will be of the form `rad://<id>.git`.
    /// let url: LocalUrl = urn.into();
    ///
    /// // Setup the `Remote`.
    /// let mut remote = Remote::rad_remote(url, fetch);
    /// remote.add_pushes(vec![push].into_iter());
    /// ```
    pub fn rad_remote(url: Url, fetch_spec: Box<dyn AsRefspec>) -> Self {
        Self {
            url,
            name: "rad".to_string(),
            fetch_spec,
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
        let _ = repo.remote_with_fetch(
            &self.name,
            &self.url.to_string(),
            &self.fetch_spec.as_refspec(),
        )?;

        for spec in self.push_specs.iter() {
            repo.remote_add_push(&self.name, &spec.as_refspec())?;
        }

        // To ensure that the push spec is persisted we need to call `find_remote` here.
        // Otherwise, `remote_add_push` doesn't affect the "loaded remotes".
        repo.find_remote("rad")
    }
}

#[cfg(test)]
mod tests {
    use std::{io, marker::PhantomData};

    use super::*;
    use crate::{git::types::*, hash::Hash};
    use librad_test::tempdir::WithTmpDir;

    #[test]
    fn can_create_remote() {
        WithTmpDir::new::<_, io::Error>(|path| {
            let repo = git2::Repository::init(path).expect("failed to init repo");

            let id = Hash::hash(b"geez");
            let heads: FlatRef<String, _> = FlatRef::heads(PhantomData, None);
            let namespaced_heads = NamespacedRef::heads(id, None);

            let fetch = heads
                .clone()
                .refspec(namespaced_heads.clone(), Force::True)
                .into_dyn();
            let push = namespaced_heads.refspec(heads, Force::False).into_dyn();


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
}
