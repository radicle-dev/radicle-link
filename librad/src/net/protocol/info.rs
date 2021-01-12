// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, convert::TryFrom, option::NoneError};

use minicbor::{Decode, Encode};

use crate::peer::PeerId;

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Encode, Decode)]
#[repr(u8)]
pub enum Capability {
    #[n(0)]
    Reserved = 0,
}

pub type PeerInfo<Addr> = GenericPeerInfo<Addr, PeerAdvertisement<Addr>>;
pub type PartialPeerInfo<Addr> = GenericPeerInfo<Addr, Option<PeerAdvertisement<Addr>>>;

impl<Addr> PartialPeerInfo<Addr>
where
    Addr: Clone + Ord,
{
    pub fn sequence(self) -> Option<PeerInfo<Addr>> {
        let PartialPeerInfo {
            peer_id,
            advertised_info,
            seen_addrs,
        } = self;
        advertised_info.map(|advertised_info| PeerInfo {
            peer_id,
            advertised_info,
            seen_addrs,
        })
    }
}

impl<Addr> TryFrom<PartialPeerInfo<Addr>> for PeerInfo<Addr>
where
    Addr: Clone + Ord,
{
    type Error = NoneError;

    fn try_from(part: PartialPeerInfo<Addr>) -> Result<Self, Self::Error> {
        Ok(part.sequence()?)
    }
}

impl<Addr> From<PeerInfo<Addr>> for PartialPeerInfo<Addr>
where
    Addr: Clone + Ord,
{
    fn from(
        PeerInfo {
            peer_id,
            advertised_info,
            seen_addrs,
        }: PeerInfo<Addr>,
    ) -> Self {
        Self {
            peer_id,
            advertised_info: Some(advertised_info),
            seen_addrs,
        }
    }
}

impl<Addr> From<PartialPeerInfo<Addr>> for (PeerId, Vec<Addr>)
where
    Addr: Clone + Ord,
{
    fn from(info: PartialPeerInfo<Addr>) -> Self {
        (
            info.peer_id,
            info.advertised_info
                .into_iter()
                .flat_map(|ad| ad.listen_addrs.into_iter())
                .chain(info.seen_addrs)
                .collect(),
        )
    }
}

impl<Addr> From<PeerInfo<Addr>> for (PeerId, Vec<Addr>)
where
    Addr: Clone + Ord,
{
    fn from(info: PeerInfo<Addr>) -> Self {
        (
            info.peer_id,
            info.advertised_info
                .listen_addrs
                .into_iter()
                .chain(info.seen_addrs)
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct GenericPeerInfo<Addr, T>
where
    Addr: Clone + Ord,
{
    #[n(0)]
    pub peer_id: PeerId,

    #[n(1)]
    pub advertised_info: T,

    #[n(2)]
    pub seen_addrs: BTreeSet<Addr>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct PeerAdvertisement<Addr>
where
    Addr: Clone + Ord,
{
    #[n(0)]
    pub listen_addrs: BTreeSet<Addr>,

    #[n(2)]
    pub capabilities: BTreeSet<Capability>,
}

impl<Addr> PeerAdvertisement<Addr>
where
    Addr: Clone + Ord,
{
    pub fn new(listen_addr: Addr) -> Self {
        Self {
            listen_addrs: vec![listen_addr].into_iter().collect(),
            capabilities: BTreeSet::default(),
        }
    }
}
