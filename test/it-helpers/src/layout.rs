// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    borrow::Borrow,
    fmt::{self, Debug, Display},
};

use librad::{
    git::{
        storage::{ReadOnlyStorage as _, Storage},
        types::{Namespace, One, Reference},
        Urn,
    },
    git_ext as ext,
    PeerId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct References {
    commits: Vec<Commit>,
    id: RadId,
    selv: RadSelf,
    ids: Vec<Delegate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadId {
    pub name: Reference<One>,
    pub target: Option<ext::Oid>,
    pub exists: bool,
}

impl RadId {
    pub fn new(storage: &Storage, urn: &Urn, remote: PeerId) -> anyhow::Result<Self> {
        let rad_id = Reference::rad_id(Namespace::from(urn.clone())).with_remote(remote);
        Ok(storage.reference(&rad_id).map(|r| match r {
            Some(r) => Self {
                name: rad_id,
                target: r.target().map(ext::Oid::from),
                exists: true,
            },
            None => Self {
                name: rad_id,
                target: None,
                exists: false,
            },
        })?)
    }
}

impl Display for RadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadSelf {
    pub name: Reference<One>,
    pub exists: bool,
}

impl RadSelf {
    pub fn new(storage: &Storage, urn: &Urn, remote: PeerId) -> anyhow::Result<Self> {
        let rad_self = Reference::rad_self(Namespace::from(urn.clone()), remote);
        Ok(storage.reference(&rad_self).map(|r| Self {
            name: rad_self,
            exists: r.is_some(),
        })?)
    }
}

impl Display for RadSelf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegate {
    pub name: Reference<One>,
    pub exists: bool,
}

impl Delegate {
    pub fn new(
        storage: &Storage,
        urn: &Urn,
        delegate: &Urn,
        remote: PeerId,
    ) -> anyhow::Result<Self> {
        let rad_delegate =
            Reference::rad_delegate(Namespace::from(urn.clone()), delegate).with_remote(remote);
        let exists = storage.has_ref(&rad_delegate)?;
        Ok(Self {
            name: rad_delegate,
            exists,
        })
    }
}

impl Display for Delegate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub urn: Urn,
    pub oid: ext::Oid,
    pub exists: bool,
}

impl Commit {
    pub fn new(storage: &Storage, urn: Urn, oid: ext::Oid) -> anyhow::Result<Self> {
        let exists = storage.has_commit(&urn, oid)?;
        Ok(Self { urn, oid, exists })
    }
}

impl Display for Commit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.urn, self.oid)
    }
}

impl References {
    pub fn new<Oid, D, C>(
        storage: &Storage,
        urn: &Urn,
        remote: PeerId,
        delegates: D,
        commits: C,
    ) -> Result<Self, anyhow::Error>
    where
        Oid: Borrow<git2::Oid> + Debug,
        D: IntoIterator<Item = Urn>,
        C: IntoIterator<Item = (Urn, Oid)>,
    {
        let ids = delegates
            .into_iter()
            .map(|delegate| Delegate::new(storage, urn, &delegate, remote))
            .collect::<Result<_, _>>()?;
        let commits = commits
            .into_iter()
            .map(|(urn, oid)| {
                let oid: ext::Oid = (*oid.borrow()).into();
                Commit::new(storage, urn, oid)
            })
            .collect::<Result<_, _>>()?;

        Ok(Self {
            commits,
            id: RadId::new(storage, urn, remote)?,
            selv: RadSelf::new(storage, urn, remote)?,
            ids,
        })
    }

    pub fn rad_id(&self) -> &RadId {
        &self.id
    }

    pub fn rad_self(&self) -> &RadSelf {
        &self.selv
    }

    pub fn rad_ids(&self) -> &[Delegate] {
        &self.ids
    }

    pub fn missing_rad_ids(&self) -> impl Iterator<Item = &Delegate> {
        self.ids.iter().filter(|d| !d.exists)
    }

    pub fn commits(&self) -> &[Commit] {
        &self.commits
    }

    pub fn missing_commits(&self) -> impl Iterator<Item = &Commit> {
        self.commits.iter().filter(|commit| !commit.exists)
    }

    pub fn has_commit(&self, oid: &ext::Oid) -> bool {
        self.commits.iter().any(|commit| &commit.oid == oid)
    }
}
