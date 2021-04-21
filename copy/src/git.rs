// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom as _,
    path::{Path, PathBuf},
};

use nonempty::NonEmpty;

use librad::{
    git::{
        local::{
            transport::{self, CanOpenStorage},
            url::LocalUrl,
        },
        types::{
            remote::{LocalFetchspec, LocalPushspec, Remote},
            Fetchspec,
            Force,
            Refspec,
        },
    },
    git_ext::{self, OneLevel, Qualified, RefLike},
    identities::payload,
    internal::canonical::Cstring,
    reflike,
    refspec_pattern,
    std_ext::result::ResultExt as _,
};

lazy_static! {
    pub static ref DEFAULT_BRANCH: OneLevel = reflike!("main").into();
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Validation(#[from] validation::Error),

    #[error(transparent)]
    Ref(#[from] git_ext::name::StripPrefixError),

    #[error(transparent)]
    Transport(#[from] transport::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub fn determine_default_branch(
    payload: &payload::Project,
) -> Result<OneLevel, git_ext::name::Error> {
    payload
        .default_branch
        .clone()
        .map_or(Ok((*DEFAULT_BRANCH).clone()), |branch| {
            RefLike::try_from(branch.as_str()).map(OneLevel::from)
        })
}

/// Equips a repository with a rad remote for the given id. If the directory at
/// the given path is not managed by git yet we initialise it first.
pub fn setup_remote<F>(
    repo: &git2::Repository,
    open_storage: F,
    url: LocalUrl,
    default_branch: &OneLevel,
) -> Result<Remote<LocalUrl>, Error>
where
    F: CanOpenStorage + Clone + 'static,
{
    let _default_branch_ref = validation::branch(repo, default_branch)?;

    tracing::debug!("Creating rad remote");

    let fetchspec = Refspec {
        src: refspec_pattern!("refs/heads/*"),
        dst: refspec_pattern!("refs/remotes/rad/*"),
        force: Force::True,
    };
    let mut git_remote = validation::remote(repo, &url)?.map_or_else(
        || {
            let mut rad = Remote::rad_remote(url, fetchspec);
            rad.save(repo)?;
            Ok::<_, Error>(rad)
        },
        Ok,
    )?;
    for pushed in git_remote.push(
        open_storage.clone(),
        repo,
        LocalPushspec::Matching {
            pattern: refspec_pattern!("refs/heads/*"),
            force: Force::True,
        },
    )? {
        tracing::debug!("Pushed local branch `{}`", pushed);
    }

    // Ensure that we have the default branch fetched from the remote
    let _fetched = git_remote.fetch(
        open_storage,
        &repo,
        LocalFetchspec::Specs(NonEmpty::new(Fetchspec::from(Refspec {
            src: reflike!("refs/heads").join(default_branch.clone()),
            dst: reflike!("refs/remotes")
                .join(git_remote.name.clone())
                .join(default_branch.clone()),
            force: Force::False,
        }))),
    )?;
    Ok(git_remote)
}

pub fn init(
    path: PathBuf,
    description: &Option<Cstring>,
    default_branch: &OneLevel,
) -> Result<git2::Repository, Error> {
    tracing::debug!("Setting up new repository @ '{}'", path.display());
    let mut options = git2::RepositoryInitOptions::new();
    options.no_reinit(true);
    options.mkpath(true);
    options.description(description.as_ref().map_or("", |desc| desc.as_str()));
    options.initial_head(default_branch.as_str());

    git2::Repository::init_opts(path, &options).map_err(Error::from)
}

pub fn initial_commit(
    repo: &git2::Repository,
    default_branch: &OneLevel,
    signature: &git2::Signature<'static>,
) -> Result<(), Error> {
    // Now let's create an empty tree for this commit
    let tree_id = {
        let mut index = repo.index()?;

        // For our purposes, we'll leave the index empty for now.
        index.write_tree()?
    };
    {
        let tree = repo.find_tree(tree_id)?;
        // Normally creating a commit would involve looking up the current HEAD
        // commit and making that be the parent of the initial commit, but here this
        // is the first commit so there will be no parent.
        repo.commit(
            Some(&format!("refs/heads/{}", default_branch.as_str())),
            signature,
            signature,
            "Initial commit",
            &tree,
            &[],
        )?;
    }
    Ok(())
}

/// Set the upstream of the given branch to the given remote.
///
/// This writes to the `config` directly. The entry will look like the
/// following:
///
/// ```text
/// [branch "main"]
///     remote = rad
///     merge = refs/heads/main
/// ```
pub fn set_upstream<Url>(
    repo: &git2::Repository,
    remote: &Remote<Url>,
    branch: OneLevel,
) -> Result<(), Error> {
    let mut config = repo.config()?;
    let branch_remote = format!("branch.{}.remote", branch);
    let branch_merge = format!("branch.{}.merge", branch);
    config
        .remove_multivar(&branch_remote, ".*")
        .or_matches::<git2::Error, _, _>(git_ext::is_not_found_err, || Ok(()))?;
    config
        .remove_multivar(&branch_merge, ".*")
        .or_matches::<git2::Error, _, _>(git_ext::is_not_found_err, || Ok(()))?;
    config.set_multivar(&branch_remote, ".*", remote.name.as_str())?;
    config.set_multivar(&branch_merge, ".*", Qualified::from(branch).as_str())?;
    Ok(())
}

/// Clone a git repository to the `path` location, based off of the `remote`
/// provided.
///
/// # Errors
///   * if initialisation of the repository fails
///   * if branch or remote manipulation fails
pub fn clone<F>(
    path: &Path,
    storage: F,
    mut remote: Remote<LocalUrl>,
) -> Result<(git2::Repository, Remote<LocalUrl>), Error>
where
    F: CanOpenStorage + 'static,
{
    let repo = git2::Repository::init(path)?;
    remote.save(&repo)?;
    for (reference, oid) in remote.fetch(storage, &repo, LocalFetchspec::Configured)? {
        let msg = format!("Fetched `{}->{}`", reference, oid);
        tracing::debug!("{}", msg);

        let branch: git_ext::RefLike = OneLevel::from(reference).into();
        let branch = branch.strip_prefix(remote.name.clone())?;
        let branch = branch.strip_prefix(reflike!("heads")).unwrap_or(branch);
        let _remote_branch = repo.reference(
            reflike!("refs/remotes")
                .join(remote.name.clone())
                .join(branch.clone())
                .as_str(),
            oid,
            true,
            &msg,
        )?;
        let _local_branch = repo.reference(Qualified::from(branch).as_str(), oid, true, &msg);
    }

    Ok((repo, remote))
}

pub mod validation {
    use std::path::PathBuf;

    use librad::{
        git::{
            local::url::LocalUrl,
            types::remote::{self, Remote},
        },
        git_ext::{self, OneLevel},
        reflike,
        std_ext::result::ResultExt as _,
    };

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("missing branch `{branch}` in the repository found at `{repo_path}`")]
        MissingDefaultBranch {
            repo_path: PathBuf,
            branch: OneLevel,
        },

        #[error("a `rad` remote exists with the URL `{found}`, the expected URL for this project is `{expected}`. If you want to continue with creating this project you will need to remove the existing `rad` remote entry.")]
        UrlMismatch { expected: LocalUrl, found: LocalUrl },

        #[error(transparent)]
        Remote(#[from] remote::FindError),

        #[error(transparent)]
        Git(#[from] git2::Error),
    }

    pub fn branch<'a>(
        repo: &'a git2::Repository,
        default_branch: &OneLevel,
    ) -> Result<git2::Reference<'a>, Error> {
        repo.resolve_reference_from_short_name(default_branch.as_str())
            .or_matches(git_ext::is_not_found_err, || {
                Err(Error::MissingDefaultBranch {
                    repo_path: repo.path().to_path_buf(),
                    branch: default_branch.clone(),
                })
            })
    }

    pub fn remote(
        repo: &git2::Repository,
        url: &LocalUrl,
    ) -> Result<Option<Remote<LocalUrl>>, Error> {
        match Remote::<LocalUrl>::find(repo, reflike!("rad")) {
            Err(remote::FindError::ParseUrl(_)) => {
                tracing::warn!("an invalid URL was found loading `rad`, moving it to `rad_old`");
                repo.remote_rename("rad", "rad_old")?;
                Ok(None)
            },
            Err(err) => Err(Error::Remote(err)),
            Ok(Some(remote)) if remote.url != *url => Err(Error::UrlMismatch {
                expected: url.clone(),
                found: remote.url,
            }),
            Ok(remote) => Ok(remote),
        }
    }
}
