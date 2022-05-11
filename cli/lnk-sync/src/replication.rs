// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{iter::FromIterator, net::SocketAddr};

use either::Either;
use serde::Serialize;

use git_ref_format::RefString;
use librad::{
    git::Urn,
    git_ext as ext,
    net::{
        peer::{client, Client},
        quic::ConnectPeer,
        replication,
    },
    PeerId,
    Signer,
};
use lnk_clib::seed::Seed;

pub(super) async fn replicate<S, E>(
    client: &Client<S, E>,
    urn: Urn,
    seed: Seed<Vec<SocketAddr>>,
) -> Result<Success, client::error::Replicate>
where
    S: Signer + Clone,
    E: ConnectPeer + Clone + Send + Sync + 'static,
{
    Ok(client
        .replicate(seed.clone(), urn.clone(), None)
        .await?
        .into())
}

// A version of the `replication::Success` type that can be serialized
#[derive(Clone, Debug, Serialize)]
pub struct Success {
    references: References,
    rejected: Rejected,
    tracked: Tracked,
    created: Created,
    requires_confirmation: bool,
}

impl From<replication::Success> for Success {
    fn from(s: replication::Success) -> Self {
        let created = s.urns_created().map(|urn| urn.into()).collect();
        let references = s.applied.updated.into_iter().collect();
        let rejected = s.applied.rejected.into_iter().collect();
        let tracked = s
            .tracked
            .into_iter()
            .fold(Tracked::default(), |mut tracked, t| {
                match t {
                    Either::Left(peer) => tracked.direct.push(peer),
                    Either::Right(urn) => tracked.indirect.push(urn.into()),
                };
                tracked
            });
        let requires_confirmation = s.requires_confirmation;
        Self {
            references,
            rejected,
            tracked,
            created,
            requires_confirmation,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Created {
    urns: Vec<Urn>,
}

impl FromIterator<Urn> for Created {
    fn from_iter<T: IntoIterator<Item = Urn>>(iter: T) -> Self {
        Self {
            urns: iter.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Tracked {
    indirect: Vec<Urn>,
    direct: Vec<PeerId>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Rejected {
    direct: Vec<Direct>,
    symbolic: Vec<Symbolic>,
    pruned: Vec<RefString>,
}

impl<'a> FromIterator<link_replication::Update<'a>> for Rejected {
    fn from_iter<T: IntoIterator<Item = link_replication::Update<'a>>>(iter: T) -> Self {
        iter.into_iter().fold(Self::default(), |mut rej, update| {
            match update {
                link_replication::Update::Direct { name, target, .. } => rej.direct.push(Direct {
                    name: name.into(),
                    target: target.into(),
                }),
                link_replication::Update::Symbolic { name, target, .. } => {
                    rej.symbolic.push(Symbolic {
                        name: name.into(),
                        target: target.name.strip_namespace().into(),
                    })
                },
                link_replication::Update::Prune { name, .. } => rej.pruned.push(name.into()),
            }
            rej
        })
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct References {
    updated: Updates,
    pruned: Vec<RefString>,
}

impl FromIterator<link_replication::Updated> for References {
    fn from_iter<T: IntoIterator<Item = link_replication::Updated>>(iter: T) -> Self {
        iter.into_iter().fold(Self::default(), |mut refs, update| {
            match update {
                link_replication::Updated::Direct { name, target } => {
                    refs.updated.direct.push(Direct {
                        name,
                        target: target.into(),
                    })
                },
                link_replication::Updated::Symbolic { name, target } => {
                    refs.updated.symbolic.push(Symbolic { name, target })
                },
                link_replication::Updated::Prune { name } => refs.pruned.push(name),
            }
            refs
        })
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Updates {
    direct: Vec<Direct>,
    symbolic: Vec<Symbolic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Direct {
    name: RefString,
    target: ext::Oid,
}

#[derive(Clone, Debug, Serialize)]
pub struct Symbolic {
    name: RefString,
    target: RefString,
}
