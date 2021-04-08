// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, str::FromStr};

use git_ext::{
    error::{is_exists_err, is_not_found_err},
    reference::{self, RefLike, RefspecPattern},
};
use nonempty::NonEmpty;
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    super::local::{self, transport::with_local_transport, url::LocalUrl},
    Fetchspec,
    Force,
    Pushspec,
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

#[derive(Debug)]
pub struct Remote<Url> {
    /// The file path to the git monorepo.
    pub url: Url,
    /// Name of the remote, e.g. `"rad"`, `"origin"`.
    pub name: RefLike,
    /// The set of fetch specs to add upon creation.
    ///
    /// **Note**: empty fetch specs do not denote the default fetch spec
    /// (`refs/heads/*:refs/remote/<name>/*`), but ... empty fetch specs.
    pub fetchspecs: Vec<Fetchspec>,
    /// The set of push specs to add upon creation.
    pub pushspecs: Vec<Pushspec>,
}

impl<Url> Remote<Url> {
    /// Create a `"rad"` remote with a single fetch spec.
    pub fn rad_remote<Ref, Spec>(url: Url, fetch_spec: Ref) -> Self
    where
        Ref: Into<Option<Spec>>,
        Spec: Into<Fetchspec>,
    {
        Self {
            url,
            name: reflike!("rad"),
            fetchspecs: fetch_spec.into().into_iter().map(Into::into).collect(),
            pushspecs: vec![],
        }
    }

    /// Create a new `Remote` with the given `url` and `name`, while making the
    /// `fetch_spec` and `pushspecs` empty.
    pub fn new<R>(url: Url, name: R) -> Self
    where
        R: Into<RefLike>,
    {
        Self {
            url,
            name: name.into(),
            fetchspecs: vec![],
            pushspecs: vec![],
        }
    }

    /// Override the fetch specs.
    pub fn with_fetchspecs<I>(self, specs: I) -> Self
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Into<Fetchspec>,
    {
        Self {
            fetchspecs: specs.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    /// Add a fetch spec.
    pub fn add_fetchspec(&mut self, spec: impl Into<Fetchspec>) {
        self.fetchspecs.push(spec.into())
    }

    /// Override the push specs.
    pub fn with_pushspecs<I>(self, specs: I) -> Self
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Into<Pushspec>,
    {
        Self {
            pushspecs: specs.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    /// Add a push spec.
    pub fn add_pushspec(&mut self, spec: impl Into<Pushspec>) {
        self.pushspecs.push(spec.into())
    }

    /// Persist the remote in the `repo`'s config.
    ///
    /// If a remote with the same name already exists, previous values of the
    /// configuration keys `url`, `fetch`, and `push` will be overwritten.
    /// Note that this means that _other_ configuration keys are left
    /// untouched, if present.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(skip(self, repo), fields(name = self.name.as_str()), err)]
    pub fn save(&mut self, repo: &git2::Repository) -> Result<(), git2::Error>
    where
        Url: ToString,
    {
        let url = self.url.to_string();
        repo.remote(self.name.as_str(), &url)
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

        repo.remote_set_url(self.name.as_str(), &url)?;

        for spec in self.fetchspecs.iter() {
            repo.remote_add_fetch(self.name.as_str(), &spec.to_string())?;
        }
        for spec in self.pushspecs.iter() {
            repo.remote_add_push(self.name.as_str(), &spec.to_string())?;
        }

        debug_assert!(repo.find_remote(self.name.as_str()).is_ok());

        Ok(())
    }

    /// Find a persisted remote by name.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(skip(repo), err)]
    pub fn find(repo: &git2::Repository, name: RefLike) -> Result<Option<Self>, FindError>
    where
        Url: FromStr,
        <Url as FromStr>::Err: std::error::Error + Send + Sync + 'static,
    {
        let git_remote = repo
            .find_remote(name.as_str())
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
                let fetchspecs = remote
                    .fetch_refspecs()?
                    .into_iter()
                    .flatten()
                    .map(Fetchspec::try_from)
                    .collect::<Result<_, _>>()?;
                let pushspecs = remote
                    .push_refspecs()?
                    .into_iter()
                    .flatten()
                    .map(Pushspec::try_from)
                    .collect::<Result<_, _>>()?;

                Ok(Some(Self {
                    url,
                    name,
                    fetchspecs,
                    pushspecs,
                }))
            },
        }
    }
}

/// What to push when calling `Remote::<LocalUrl>::push`.
#[derive(Debug)]
pub enum LocalPushspec {
    /// Read the matching refs from the repo at runtime.
    Matching {
        pattern: RefspecPattern,
        force: Force,
    },
    /// Use the provided [`Pushspec`]s.
    Specs(NonEmpty<Pushspec>),
    /// Use whatever is persistently configured for the [`Remote`].
    ///
    /// It is an error if the [`Remote`] is not persisted. If the remote **is**
    /// persisted, but has no explicit push spec, nothing will be pushed.
    Configured,
}

/// What to fetch when calling `Remote::<LocalUrl>::fetch`.
#[derive(Debug)]
pub enum LocalFetchspec {
    /// Use the provided [`Fetchspec`]s.
    Specs(NonEmpty<Fetchspec>),
    /// Use whatever is persistently configured for the [`Remote`].
    ///
    /// It is an error if the [`Remote`] is not persisted. If the remote **is**
    /// persisted, but has no explicit fetch spec, nothing will be fetched.
    Configured,
}

impl Remote<LocalUrl> {
    /// Get the remote repository's reference advertisement list.
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

    /// Push the provided [`LocalPushspec`].
    #[tracing::instrument(skip(self, repo, open_storage), err)]
    pub fn push<F>(
        &mut self,
        open_storage: F,
        repo: &git2::Repository,
        spec: LocalPushspec,
    ) -> Result<impl Iterator<Item = RefLike>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        use LocalPushspec::*;

        let res = match spec {
            Matching { pattern, force } => {
                let specs = {
                    let mut refs = repo.references_glob(pattern.as_str())?;
                    let mut specs = Vec::new();
                    for name in refs.names() {
                        if let Ok(refl) = RefLike::try_from(name?) {
                            specs.push(
                                Refspec {
                                    src: refl.clone(),
                                    dst: refl,
                                    force,
                                }
                                .to_string(),
                            )
                        }
                    }
                    Ok::<_, local::transport::Error>(specs)
                }?;
                self.push_internal(&specs, open_storage, |url| {
                    repo.remote_anonymous(&url.to_string())
                })
            },

            Specs(specs) => self.push_internal(
                &specs
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect::<Vec<_>>(),
                open_storage,
                |url| repo.remote_anonymous(&url.to_string()),
            ),

            Configured => self.with_tmp_copy(repo, |this| {
                this.push_internal(&[] as &[&str], open_storage, |url| {
                    repo.remote_set_url(this.name.as_str(), &url.to_string())?;
                    repo.find_remote(this.name.as_str())
                })
            })?,
        };

        Ok(res??.into_iter())
    }

    fn push_internal<'a, S, F, G>(
        &'a self,
        specs: &[S],
        open_storage: F,
        open_remote: G,
    ) -> Result<Result<Vec<RefLike>, git2::Error>, local::transport::Error>
    where
        S: AsRef<str> + git2::IntoCString + Clone + std::fmt::Debug,
        F: local::transport::CanOpenStorage + 'static,
        G: FnOnce(LocalUrl) -> Result<git2::Remote<'a>, git2::Error>,
    {
        with_local_transport(open_storage, self.url.clone(), |url| {
            let mut git_remote = open_remote(url)?;
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

            tracing::debug!("specs: {:?}", specs);
            git_remote.push(
                specs,
                Some(git2::PushOptions::new().remote_callbacks(callbacks)),
            )?;

            Ok(updated_refs)
        })
    }

    #[tracing::instrument(skip(self, repo, open_storage), err)]
    pub fn fetch<F>(
        &mut self,
        open_storage: F,
        repo: &git2::Repository,
        spec: LocalFetchspec,
    ) -> Result<impl Iterator<Item = (RefLike, git2::Oid)>, local::transport::Error>
    where
        F: local::transport::CanOpenStorage + 'static,
    {
        use LocalFetchspec::*;

        let res = match spec {
            Specs(specs) => self.fetch_internal(
                &specs
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect::<Vec<_>>(),
                open_storage,
                |url| repo.remote_anonymous(&url.to_string()),
            ),

            Configured => self.with_tmp_copy(repo, |this| {
                this.fetch_internal(&[] as &[&str], open_storage, |url| {
                    repo.remote_set_url(this.name.as_str(), &url.to_string())?;
                    repo.find_remote(this.name.as_str())
                })
            })?,
        };

        Ok(res??.into_iter())
    }

    fn fetch_internal<'a, S, F, G>(
        &self,
        specs: &[S],
        open_storage: F,
        open_remote: G,
    ) -> Result<Result<Vec<(RefLike, git2::Oid)>, git2::Error>, local::transport::Error>
    where
        S: AsRef<str> + git2::IntoCString + Clone,
        F: local::transport::CanOpenStorage + 'static,
        G: FnOnce(LocalUrl) -> Result<git2::Remote<'a>, git2::Error>,
    {
        with_local_transport(open_storage, self.url.clone(), |url| {
            let mut git_remote = open_remote(url)?;
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
                specs,
                Some(
                    git2::FetchOptions::new()
                        .prune(git2::FetchPrune::Off)
                        .update_fetchhead(false)
                        .download_tags(git2::AutotagOption::None)
                        .remote_callbacks(callbacks),
                ),
                None,
            )?;

            Ok(updated_refs)
        })
    }

    /// When using a persistent remote, we need to rewrite the URL to add the
    /// lookup index. However, we might have read the remote from an
    /// included config file, in which case `libgit2` bails out when trying
    /// to modify it. We work around this by creating a temporary persistent
    /// remote (in the local config), which we delete after we're done.
    ///
    /// # Safety
    ///
    /// This is not currently panic safe -- ie. if the closure panics, we might
    /// leak the temporary remote. Also, no effort is made to ensure a
    /// remote can safely be used concurrently.
    fn with_tmp_copy<F, A>(&mut self, repo: &git2::Repository, f: F) -> Result<A, git2::Error>
    where
        F: FnOnce(&mut Self) -> A,
    {
        let orig_name = self.name.clone();
        let orig_url = self.url.clone();
        self.name = reflike!("__tmp_").join(&self.name);
        tracing::debug!("creating temporary remote {}", self.name);
        self.save(repo).unwrap();
        let res = f(self);
        let deleted = repo.remote_delete(self.name.as_str());
        self.name = orig_name;
        self.url = orig_url;
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
    use std::{convert::TryFrom, io};

    use git_ext as ext;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        git::{
            local::url::LocalUrl,
            types::{AsNamespace, Force, Namespace, Reference, Refspec},
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

            let fetch = Refspec {
                src: refspec_pattern!("refs/heads/*"),
                dst: Reference::heads(Namespace::from(&*URN), None),
                force: Force::True,
            };
            let push = Refspec {
                src: reflike!("refs/heads/next"),
                dst: Reference::head(Namespace::from(&*URN), None, reflike!("next")),
                force: Force::False,
            };

            {
                let url = LocalUrl::from(URN.clone());
                let mut remote = Remote::rad_remote(url, fetch).with_pushspecs(Some(push));
                remote.save(&repo).expect("failed to persist the remote");
            }

            let remote = Remote::<LocalUrl>::find(&repo, reflike!("rad"))
                .unwrap()
                .expect("should exist");

            assert_eq!(
                remote
                    .fetchspecs
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect::<Vec<_>>(),
                vec![format!(
                    "+refs/heads/*:refs/namespaces/{}/refs/heads/*",
                    Namespace::from(&*URN).into_namespace()
                )],
            );

            assert_eq!(
                remote
                    .pushspecs
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect::<Vec<_>>(),
                vec![format!(
                    "refs/heads/next:refs/namespaces/{}/refs/heads/next",
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
        let name = ext::RefLike::try_from(format!("lyla@{}", *PEER_ID)).unwrap();

        let heads = Reference::heads(None, *PEER_ID);
        let remotes = reflike!("refs/remotes")
            .join(&name)
            .with_pattern_suffix(refspec_pattern!("*"));
        let mut remote = Remote {
            url,
            name: name.clone(),
            fetchspecs: vec![Refspec {
                src: heads,
                dst: remotes,
                force: Force::True,
            }
            .into()],
            pushspecs: vec![],
        };

        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path())?;
        remote.save(&repo)?;
        let git_remote = repo.find_remote(name.as_str())?;
        let fetch_refspecs = git_remote.fetch_refspecs()?;

        assert_eq!(
            fetch_refspecs.iter().flatten().collect::<Vec<_>>(),
            vec![format!("+refs/remotes/{}/heads/*:refs/remotes/{}/*", *PEER_ID, name).as_str()]
        );

        Ok(())
    }
}
