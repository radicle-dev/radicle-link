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

use git_ext::{
    error::{is_exists_err, is_not_found_err},
    reference::{self, RefLike},
};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    super::local::{self, transport::with_local_transport, url::LocalUrl},
    AsRefspec,
    Refspec,
};

#[derive(Debug, Error)]
pub enum FindError {
    #[error("missing {0}")]
    Missing(&'static str),

    #[error("failed to parse URL")]
    ParseUrl(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("failed to parse refspec")]
    Refspec(#[from] reference::name::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

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
    /// };
    ///
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
    /// // We point the remote to `LocalUrl` which will be of the form `rad://<id>.git`.
    /// let url = LocalUrl::from(urn);
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
    #[tracing::instrument(skip(self, repo), fields(name = self.name.as_str()), err)]
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
    #[tracing::instrument(skip(repo, name), fields(name = name.as_ref()), err)]
    pub fn find<Name>(repo: &git2::Repository, name: Name) -> Result<Option<Self>, FindError>
    where
        Url: FromStr,
        <Url as FromStr>::Err: std::error::Error + Send + Sync + 'static,

        Name: AsRef<str> + Into<String>,
    {
        let git_remote = repo
            .find_remote(name.as_ref())
            .map(Some)
            .or_matches::<FindError, _, _>(is_not_found_err, || Ok(None))?;

        match git_remote {
            None => Ok(None),
            Some(remote) => {
                let url = remote
                    .url()
                    .ok_or(FindError::Missing("url"))?
                    .parse()
                    .map_err(|e| FindError::ParseUrl(Box::new(e)))?;
                let fetch_specs = remote
                    .fetch_refspecs()?
                    .into_iter()
                    .filter_map(identity)
                    .map(|spec| Refspec::try_from(spec).map(|spec| spec.boxed()))
                    .collect::<Result<_, _>>()?;
                let push_specs = remote
                    .push_refspecs()?
                    .into_iter()
                    .filter_map(identity)
                    .map(|spec| Refspec::try_from(spec).map(|spec| spec.boxed()))
                    .collect::<Result<_, _>>()?;

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

impl Remote<LocalUrl> {
    #[tracing::instrument(skip(self, repo, open_storage), err)]
    pub fn remote_heads<F>(
        &mut self,
        open_storage: F,
        repo: &git2::Repository,
    ) -> Result<impl Iterator<Item = (RefLike, git2::Oid)>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        let heads: Result<Vec<(RefLike, git2::Oid)>, local::transport::Error> =
            with_local_transport(open_storage, self.url.clone(), |url| {
                let mut git_remote = repo.remote_anonymous(&url.to_string())?;
                git_remote.connect(git2::Direction::Fetch)?;
                let heads = git_remote
                    .list()?
                    .iter()
                    .filter_map(|remote_head| {
                        RefLike::try_from(remote_head.name())
                            .ok()
                            .map(|name| (name, remote_head.oid()))
                    })
                    .collect::<Vec<_>>();
                git_remote.disconnect()?;

                Ok(heads)
            })?;

        Ok(heads?.into_iter())
    }

    #[tracing::instrument(skip(self, repo, open_storage), err)]
    pub fn push<F>(
        &mut self,
        open_storage: F,
        repo: &git2::Repository,
    ) -> Result<impl Iterator<Item = RefLike>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        let res = self.with_tmp_copy(repo, |this| this.push_internal(open_storage, repo))???;
        Ok(res.into_iter())
    }

    fn push_internal<F>(
        &self,
        open_storage: F,
        repo: &git2::Repository,
    ) -> Result<Result<Vec<RefLike>, git2::Error>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        let res = with_local_transport(open_storage, self.url.clone(), |url| {
            // set indexed url
            repo.remote_set_url(&self.name, &url.to_string())?;

            let mut git_remote = repo.find_remote(&self.name)?;
            tracing::trace!(
                "pushspecs: {:?}",
                git_remote.push_refspecs()?.iter().collect::<Vec<_>>()
            );

            let mut updated_refs = Vec::new();
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.push_update_reference(|name, e| match e {
                None => {
                    tracing::trace!("push updated reference `{}`", name);
                    let refname = RefLike::try_from(name).map_err(|e| {
                        git2::Error::new(
                            git2::ErrorCode::InvalidSpec,
                            git2::ErrorClass::Net,
                            &format!("unable to parse reference `{}`: {}", name, e),
                        )
                    })?;
                    updated_refs.push(refname);

                    Ok(())
                },

                Some(err) => Err(git2::Error::from_str(&format!(
                    "remote rejected {}: {}",
                    name, err
                ))),
            });

            git_remote.push(
                &[] as &[&str],
                Some(git2::PushOptions::new().remote_callbacks(callbacks)),
            )?;

            Ok(updated_refs)
        });

        // reset original url
        repo.remote_set_url(&self.name, &self.url.to_string())?;
        res
    }

    #[tracing::instrument(skip(self, repo, open_storage), err)]
    pub fn fetch<F>(
        &mut self,
        open_storage: F,
        repo: &git2::Repository,
    ) -> Result<impl Iterator<Item = (RefLike, git2::Oid)>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        let res = self.with_tmp_copy(repo, |this| this.fetch_internal(open_storage, repo))???;
        Ok(res.into_iter())
    }

    #[tracing::instrument(skip(self, open_storage, repo), err)]
    fn fetch_internal<F>(
        &self,
        open_storage: F,
        repo: &git2::Repository,
    ) -> Result<Result<Vec<(RefLike, git2::Oid)>, git2::Error>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        let res = with_local_transport(open_storage, self.url.clone(), |url| {
            // set indexed url
            repo.remote_set_url(&self.name, &url.to_string()).unwrap();

            let mut git_remote = repo.find_remote(&self.name)?;
            tracing::trace!(
                "fetchspecs: {:?}",
                git_remote.fetch_refspecs()?.iter().collect::<Vec<_>>()
            );

            let mut updated_refs = Vec::new();
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.update_tips(|name, _old, new| {
                tracing::trace!("updated tip `{} -> {}`", name, new);
                if let Ok(refname) = RefLike::try_from(name) {
                    updated_refs.push((refname, new))
                }
                true
            });

            git_remote.fetch(
                &[] as &[&str],
                Some(
                    git2::FetchOptions::new()
                        .prune(git2::FetchPrune::On)
                        .update_fetchhead(false)
                        .download_tags(git2::AutotagOption::None)
                        .remote_callbacks(callbacks),
                ),
                None,
            )?;

            Ok(updated_refs)
        });

        // reset original url
        repo.remote_set_url(&self.name, &self.url.to_string())?;
        res
    }

    fn with_tmp_copy<F, A>(&mut self, repo: &git2::Repository, f: F) -> Result<A, git2::Error>
    where
        F: FnOnce(&mut Self) -> A,
    {
        let orig_name = self.name.clone();
        self.name = format!("__tmp_{}", self.name);
        tracing::debug!("creating temporary remote {}", self.name);
        self.save(repo).unwrap();
        let res = f(self);
        let deleted = repo.remote_delete(&self.name);
        self.name = orig_name;
        deleted?;
        Ok(res)
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
                let url = LocalUrl::from(URN.clone());
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
        let url = LocalUrl::from(URN.clone());
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
