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

use std::{
    convert::{identity, TryFrom},
    fmt::{self, Debug},
    str::FromStr,
};

use git_ext::error::{is_exists_err, is_not_found_err};
use std_ext::result::ResultExt as _;

use super::{AsRefspec, Refspec};

pub struct Remote<Url> {
    /// The file path to the git monorepo.
    pub url: Url,
    /// Name of the remote, e.g. `"rad"`, `"origin"`.
    pub name: String,
    /// If the fetch spec is provided then the remote is created with an initial
    /// fetchspec, otherwise it is just a plain remote.
    pub fetch_specs: Vec<Box<dyn AsRefspec>>,
    /// The set of push specs to add upon creation.
    pub push_specs: Vec<Box<dyn AsRefspec>>,
}

impl<Url> Debug for Remote<Url>
where
    Url: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Remote")
            .field("url", &self.url)
            .field("name", &self.name)
            .field(
                "fetch_specs",
                &self
                    .fetch_specs
                    .iter()
                    .map(|spec| spec.as_refspec())
                    .collect::<Vec<_>>(),
            )
            .field(
                "push_specs",
                &self
                    .push_specs
                    .iter()
                    .map(|spec| spec.as_refspec())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl<Url> Remote<Url> {
    /// Create a `"rad"` remote with a single fetch spec.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::marker::PhantomData;
    /// use librad::{
    ///     git_ext::RefLike,
    ///     git::{
    ///         local::url::LocalUrl,
    ///         types::{
    ///             namespace::Namespace,
    ///             remote::Remote,
    ///             FlatRef,
    ///             Force,
    ///             NamespacedRef
    ///         },
    ///         Urn,
    ///     },
    ///     keys::SecretKey,
    /// };
    ///
    /// let peer_id = SecretKey::new().into();
    /// let urn = Urn::new(git2::Oid::hash_object(git2::ObjectType::Commit, b"geez").unwrap().into());
    ///
    /// // The working copy heads, i.e. `refs/heads/*`.
    /// let working_copy_heads: FlatRef<RefLike, _> = FlatRef::heads(PhantomData, None);
    ///
    /// // The monorepo heads, i.e. `refs/namespaces/<id>/refs/heads/*`.
    /// let monorepo_heads = NamespacedRef::heads(Namespace::from(&urn), None);
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
            fetch_specs: fetch_spec.into().into_iter().collect(),
            push_specs: vec![],
        }
    }

    /// Create a new `Remote` with the given `url` and `name`, while making the
    /// `fetch_specs` and `push_specs` empty.
    pub fn new(url: Url, name: String) -> Self {
        Self {
            url,
            name,
            fetch_specs: vec![],
            push_specs: vec![],
        }
    }

    pub fn with_refspec(mut self, refspec: Box<dyn AsRefspec>) -> Self {
        self.fetch_specs = vec![refspec];
        self
    }

    /// Add a series of push specs to the remote.
    pub fn add_pushes<I>(&mut self, specs: I)
    where
        I: IntoIterator<Item = Box<dyn AsRefspec>>,
    {
        for spec in specs {
            self.push_specs.push(spec)
        }
    }

    /// Persist the remote in the `repo`'s config.
    ///
    /// If a remote with the same name already exists, previous values of the
    /// configuration keys `url`, `fetch`, and `push` will be overwritten.
    /// Note that this means that _other_ configuration keys are left
    /// untouched, if present.
    pub fn save(&self, repo: &git2::Repository) -> Result<(), git2::Error>
    where
        Url: ToString,
    {
        let url = self.url.to_string();

        repo.remote(&self.name, &url)
            .and(Ok(()))
            .or_matches::<git2::Error, _, _>(is_exists_err, || Ok(()))?;

        {
            let mut config = repo.config()?;
            config
                .remove_multivar(&format!("remote.{}.url", self.name), ".*")
                .or_matches::<git2::Error, _, _>(is_not_found_err, || Ok(()))?;
            config
                .remove_multivar(&format!("remote.{}.fetch", self.name), ".*")
                .or_matches::<git2::Error, _, _>(is_not_found_err, || Ok(()))?;
            config
                .remove_multivar(&format!("remote.{}.push", self.name), ".*")
                .or_matches::<git2::Error, _, _>(is_not_found_err, || Ok(()))?;
        }

        repo.remote_set_url(&self.name, &url)?;

        for spec in self.fetch_specs.iter() {
            repo.remote_add_fetch(&self.name, &spec.as_refspec())?;
        }
        for spec in self.push_specs.iter() {
            repo.remote_add_push(&self.name, &spec.as_refspec())?;
        }

        debug_assert!(repo.find_remote(&self.name).is_ok());

        Ok(())
    }

    /// Find a persisted remote by name.
    pub fn find<Name>(repo: &git2::Repository, name: Name) -> Result<Option<Self>, git2::Error>
    where
        Url: FromStr,
        <Url as FromStr>::Err: Debug,

        Name: AsRef<str> + Into<String>,
    {
        let git_remote = repo
            .find_remote(name.as_ref())
            .map(Some)
            .or_matches::<git2::Error, _, _>(is_not_found_err, || Ok(None))?;

        match git_remote {
            None => Ok(None),
            Some(remote) => {
                let url = remote.url().unwrap().parse().unwrap();
                let fetch_specs = remote
                    .fetch_refspecs()?
                    .into_iter()
                    .filter_map(identity)
                    .filter_map(|spec| Refspec::try_from(spec).ok().map(|spec| spec.boxed()))
                    .collect();
                let push_specs = remote
                    .push_refspecs()?
                    .into_iter()
                    .filter_map(identity)
                    .filter_map(|spec| Refspec::try_from(spec).ok().map(|spec| spec.boxed()))
                    .collect();

                Ok(Some(Self {
                    url,
                    name: name.into(),
                    fetch_specs,
                    push_specs,
                }))
            },
        }
    }
}

impl<Url> AsRef<Url> for Remote<Url> {
    fn as_ref(&self) -> &Url {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::TryFrom, io, marker::PhantomData};

    use git_ext as ext;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        git::{
            local::url::LocalUrl,
            types::{
                namespace::{AsNamespace, Namespace},
                FlatRef,
                Force,
                NamespacedRef,
            },
            Urn,
        },
        keys::SecretKey,
        peer::PeerId,
    };
    use librad_test::tempdir::WithTmpDir;

    lazy_static! {
        static ref PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
            167, 44, 200, 200, 213, 81, 154, 10, 55, 187, 241, 156, 54, 52, 39, 112, 217, 179, 101,
            43, 167, 22, 230, 111, 42, 226, 79, 33, 126, 97, 51, 208
        ]));
        static ref URN: Urn = Urn::new(git_ext::Oid::from(
            git2::Oid::hash_object(git2::ObjectType::Commit, b"meow-meow").unwrap()
        ));
    }

    #[test]
    fn can_create_remote() {
        WithTmpDir::new::<_, io::Error>(|path| {
            let repo = git2::Repository::init(path).expect("failed to init repo");

            let heads: FlatRef<ext::RefLike, _> = FlatRef::heads(PhantomData, None);
            let namespaced_heads = NamespacedRef::heads(Namespace::from(&*URN), None);

            let fetch = heads
                .clone()
                .refspec(namespaced_heads.clone(), Force::True)
                .boxed();
            let push = namespaced_heads.refspec(heads, Force::False).boxed();

            {
                let url = LocalUrl::from_urn(URN.clone(), *PEER_ID);
                let mut remote = Remote::rad_remote(url, fetch);
                remote.add_pushes(vec![push].into_iter());
                remote.save(&repo).expect("failed to persist the remote");
            }

            let remote = Remote::<LocalUrl>::find(&repo, "rad")
                .unwrap()
                .expect("should exist");

            assert_eq!(
                remote
                    .fetch_specs
                    .iter()
                    .map(|spec| spec.as_refspec())
                    .collect::<Vec<_>>(),
                vec![format!(
                    "+refs/namespaces/{}/refs/heads/*:refs/heads/*",
                    Namespace::from(&*URN).into_namespace()
                )],
            );

            assert_eq!(
                remote
                    .push_specs
                    .iter()
                    .map(|spec| spec.as_refspec())
                    .collect::<Vec<_>>(),
                vec![format!(
                    "refs/heads/*:refs/namespaces/{}/refs/heads/*",
                    Namespace::from(&*URN).into_namespace()
                )],
            );

            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn check_remote_fetch_spec() -> Result<(), git2::Error> {
        let url = LocalUrl::from_urn(URN.clone(), *PEER_ID);
        let name = format!("lyla@{}", *PEER_ID);
        let heads: FlatRef<PeerId, _> = FlatRef::heads(PhantomData, *PEER_ID);
        let heads = heads.with_name(refspec_pattern!("heads/*"));
        let remotes: FlatRef<ext::RefLike, _> =
            FlatRef::heads(PhantomData, ext::RefLike::try_from(name.as_str()).unwrap());
        let remote = Remote {
            url,
            name: name.clone(),
            fetch_specs: vec![remotes.refspec(heads, Force::True).boxed()],
            push_specs: vec![],
        };

        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path())?;
        remote.save(&repo)?;
        let git_remote = repo.find_remote(&name)?;
        let fetch_refspecs = git_remote.fetch_refspecs()?;

        assert_eq!(
            fetch_refspecs
                .iter()
                .filter_map(identity)
                .collect::<Vec<_>>(),
            vec![format!("+refs/remotes/{}/heads/*:refs/remotes/{}/*", *PEER_ID, name).as_str()]
        );

        Ok(())
    }
}
