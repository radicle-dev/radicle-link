// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, convert::Infallible, str::FromStr};

use futures::{pin_mut, StreamExt as _};
use radicle_git_ext::FromMultihashError;
use thiserror::Error;
use tracing::{error, info, instrument, trace};

use librad::{
    git::{tracking, Urn},
    net::{
        peer::{event::upstream::Gossip, Peer, PeerInfo, ProtocolEvent},
        protocol::{
            broadcast::PutResult::Uninteresting,
            gossip::Payload,
            request_pull,
            RequestPullGuard,
        },
    },
    PeerId,
    Signer,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tracker {
    /// Track any `Urn` or `PeerId`, regardless of a tracking entry being
    /// present or not.
    Everything,
    /// Track only the selected `Urn` or `PeerId`.
    ///
    /// Use [`Tracker::selected`] for constructing this variant.
    Selected(Selected),
}

impl request_pull::Guard for Tracker {
    type Error = Infallible;

    type Output = bool;

    fn guard(&self, peer: &PeerId, urn: &Urn) -> Result<Self::Output, Self::Error> {
        match self {
            Self::Everything => Ok(true),
            Self::Selected(selected) => selected.guard(peer, urn),
        }
    }
}

/// A set of selected `Urn` and `PeerId`s for tracking.
///
/// Since a `Selection` can be a `Selection::Peer`, `Selection::Urn`,
/// or `Selection::Pair`, the construction of a `Selected`, via
/// [`Selected::new`], will deduplicate any `PeerId`s or `Urn`s and the
/// [`Pair`] will take preferrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected(Vec<Selection>);

impl request_pull::Guard for Selected {
    type Error = Infallible;

    type Output = bool;

    fn guard(&self, peer: &PeerId, urn: &Urn) -> Result<Self::Output, Self::Error> {
        Ok(self.0.iter().any(|s| s.guard(peer, urn).unwrap()))
    }
}

impl Selected {
    pub fn new(
        peers: impl IntoIterator<Item = PeerId>,
        urns: impl IntoIterator<Item = Urn>,
        pairs: impl IntoIterator<Item = Pair>,
    ) -> Self {
        let mut selected = Vec::new();
        let mut peers = peers.into_iter().collect::<BTreeSet<PeerId>>();
        let mut urns = urns.into_iter().collect::<BTreeSet<Urn>>();
        for pair in pairs {
            peers.remove(&pair.peer);
            urns.remove(&pair.urn);
            selected.push(Selection::from(pair))
        }

        selected.extend(peers.into_iter().map(Selection::from));
        selected.extend(urns.into_iter().map(Selection::from));
        Self(selected)
    }

    /// [`Selection::Peer`]s only.
    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.0.iter().filter_map(|s| match s {
            Selection::Peer(peer) => Some(peer),
            _ => None,
        })
    }

    /// [`Selection::Urn`]s only.
    pub fn urns(&self) -> impl Iterator<Item = &Urn> {
        self.0.iter().filter_map(|s| match s {
            Selection::Urn(urn) => Some(urn),
            _ => None,
        })
    }

    /// [`Selection::Pair`]s only.
    pub fn pairs(&self) -> impl Iterator<Item = &Pair> {
        self.0.iter().filter_map(|s| match s {
            Selection::Pair(pair) => Some(pair),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Peer(PeerId),
    Urn(Urn),
    Pair(Pair),
}

impl request_pull::Guard for Selection {
    type Error = Infallible;

    type Output = bool;

    fn guard(&self, peer: &PeerId, urn: &Urn) -> Result<Self::Output, Self::Error> {
        match self {
            Selection::Peer(s) => Ok(s == peer),
            Selection::Urn(s) => Ok(s == urn),
            Selection::Pair(pair) => Ok(&pair.peer == peer && &pair.urn == urn),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    peer: PeerId,
    urn: Urn,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("tracking pair must be of the form `<peer>,<urn>`")]
    Malformed,
    #[error(transparent)]
    Peer(#[from] librad::crypto::peer::conversion::Error),
    #[error(transparent)]
    Urn(#[from] librad::identities::urn::error::FromStr<FromMultihashError>),
}

impl FromStr for Pair {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(',') {
            None => Err(ParseError::Malformed),
            Some((peer, urn)) => Ok(Self {
                peer: peer.parse()?,
                urn: urn.parse::<Urn>()?.with_path(None),
            }),
        }
    }
}

impl From<PeerId> for Selection {
    fn from(peer: PeerId) -> Self {
        Self::Peer(peer)
    }
}

impl From<Urn> for Selection {
    fn from(urn: Urn) -> Self {
        Self::Urn(urn)
    }
}

impl From<(PeerId, Urn)> for Selection {
    fn from((peer, urn): (PeerId, Urn)) -> Self {
        Pair { peer, urn }.into()
    }
}

impl From<Pair> for Selection {
    fn from(pair: Pair) -> Self {
        Self::Pair(pair)
    }
}

impl Selection {
    fn is_tracked(&self, peer_id: &PeerId, urn: &Urn) -> bool {
        match self {
            Self::Peer(s) => s == peer_id,
            Self::Urn(s) => s == urn,
            Self::Pair(pair) => &pair.peer == peer_id && &pair.urn == urn,
        }
    }
}

impl Tracker {
    pub fn selected(
        peers: impl IntoIterator<Item = PeerId>,
        urns: impl IntoIterator<Item = Urn>,
        pairs: impl IntoIterator<Item = Pair>,
    ) -> Self {
        Self::Selected(Selected::new(peers, urns, pairs))
    }

    fn is_tracked(&self, peer_id: &PeerId, urn: &Urn) -> bool {
        match self {
            Self::Everything => true,
            Self::Selected(Selected(s)) => s.iter().any(|s| s.is_tracked(peer_id, urn)),
        }
    }
}

#[instrument(name = "tracking subroutine", skip(peer, tracker))]
pub async fn routine<S, G>(peer: Peer<S, G>, tracker: Tracker) -> anyhow::Result<()>
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    let events = peer.subscribe();
    pin_mut!(events);

    while let Some(res) = events.next().await {
        match res {
            Ok(ProtocolEvent::Gossip(gossip)) => {
                let Gossip::Put {
                    payload: Payload { urn, .. },
                    provider:
                        PeerInfo {
                            peer_id,
                            seen_addrs,
                            ..
                        },
                    result,
                } = *gossip;

                if result != Uninteresting || !tracker.is_tracked(&peer_id, &urn) {
                    continue;
                }

                let go = async {
                    let updated = peer
                    .using_storage({
                        let urn = urn.clone();
                        move |storage| -> anyhow::Result<bool> {
                            match tracking::track(
                                storage,
                                &urn,
                                Some(peer_id),
                                tracking::Config::default(),
                                tracking::policy::Track::MustNotExist,
                            )? {
                                Ok(reference) => {
                                    trace!(name=%reference.name, target=%reference.target, "created tracking entry");
                                    Ok(true)
                                },
                                Err(err) => {
                                    trace!(err = %err, "tracking policy error");
                                    Ok(false)
                                }
                            }
                        }
                    })
                    .await??;

                    // Skip explicit replication if the peer is already tracked.
                    if updated {
                        let addr_hints = seen_addrs.iter().copied().collect::<Vec<_>>();
                        peer.client()?
                            .replicate((peer_id, addr_hints), urn.clone(), None)
                            .await?;
                    }

                    Ok::<_, anyhow::Error>(updated)
                };

                match go.await {
                    Ok(true) => info!("tracked project {} from {}", urn, peer_id),
                    Ok(false) => info!("already tracked {} from {}", urn, peer_id),
                    Err(err) => error!(?err, "tracking failed for {} from {}", urn, peer_id),
                }
            },

            Ok(_) => {},
            Err(err) => {
                error!(?err, "event error");
            },
        }
    }

    Ok(())
}
