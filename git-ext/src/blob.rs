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

use std::{borrow::Cow, path::Path};

use radicle_std_ext::result::ResultExt as _;
use thiserror::Error;

use crate::{error::is_not_found_err, revwalk};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    NotFound(#[from] NotFound),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

#[derive(Debug, Error)]
pub enum NotFound {
    #[error("blob with path {0} not found")]
    NoSuchBlob(String),

    #[error("branch {0} not found")]
    NoSuchBranch(String),

    #[error("the supplied git2::Reference does not have a target")]
    NoRefTarget,
}

pub enum Branch<'a> {
    Name(Cow<'a, str>),
    Ref(git2::Reference<'a>),
}

impl<'a> From<&'a str> for Branch<'a> {
    fn from(s: &'a str) -> Self {
        Self::Name(Cow::Borrowed(s))
    }
}

impl<'a> From<String> for Branch<'a> {
    fn from(s: String) -> Self {
        Self::Name(Cow::Owned(s))
    }
}

impl<'a> From<git2::Reference<'a>> for Branch<'a> {
    fn from(r: git2::Reference<'a>) -> Self {
        Self::Ref(r)
    }
}

pub enum Blob<'a> {
    Tip { branch: Branch<'a>, path: &'a Path },
    Init { branch: Branch<'a>, path: &'a Path },
}

impl<'a> Blob<'a> {
    pub fn get(self, git: &'a git2::Repository) -> Result<git2::Blob<'a>, Error> {
        match self {
            Self::Tip { branch, path } => {
                let reference = match branch {
                    Branch::Name(name) => {
                        git.find_reference(&name).or_matches(is_not_found_err, || {
                            Err(Error::NotFound(NotFound::NoSuchBranch(
                                name.to_owned().to_string(),
                            )))
                        })
                    },

                    Branch::Ref(reference) => Ok(reference),
                }?;
                let tree = reference.peel_to_tree()?;
                blob(git, tree, path)
            },

            Self::Init { branch, path } => {
                let start = match branch {
                    Branch::Name(name) => Ok(revwalk::Start::Ref(name.to_string())),
                    Branch::Ref(reference) => {
                        match (reference.target(), reference.symbolic_target()) {
                            (Some(oid), _) => Ok(revwalk::Start::Oid(oid)),
                            (_, Some(sym)) => Ok(revwalk::Start::Ref(sym.to_string())),
                            (_, _) => Err(Error::NotFound(NotFound::NoRefTarget)),
                        }
                    },
                }?;

                let revwalk = revwalk::FirstParent::new(git, start)?.reverse()?;
                match revwalk.into_iter().next() {
                    None => Err(Error::NotFound(NotFound::NoSuchBlob(
                        path.display().to_string(),
                    ))),
                    Some(oid) => {
                        let oid = oid?;
                        let tree = git.find_commit(oid)?.tree()?;
                        blob(git, tree, path)
                    },
                }
            },
        }
    }
}

fn blob<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
    path: &'a Path,
) -> Result<git2::Blob<'a>, Error> {
    let entry = tree.get_path(path).or_matches(is_not_found_err, || {
        Err(Error::NotFound(NotFound::NoSuchBlob(
            path.display().to_string(),
        )))
    })?;

    entry.to_object(repo)?.peel_to_blob().map_err(Error::from)
}
