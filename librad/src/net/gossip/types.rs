// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashSet, hash::Hash};

use minicbor::{Decode, Encode};

use crate::peer::PeerId;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Encode, Decode)]
#[repr(u8)]
pub enum Capability {
    #[n(0)]
    Reserved = 0,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct PeerInfo<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    #[n(0)]
    pub peer_id: PeerId,
    #[n(1)]
    pub advertised_info: PeerAdvertisement<Addr>,
    #[n(2)]
    pub seen_addrs: HashSet<Addr>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct PeerAdvertisement<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    #[n(0)]
    pub listen_addr: Addr,

    #[n(1)]
    pub listen_port: u16,

    #[n(2)]
    pub capabilities: HashSet<Capability>,
}

impl<Addr> PeerAdvertisement<Addr>
where
    Addr: Clone + PartialEq + Eq + Hash,
{
    pub fn new(listen_addr: Addr, listen_port: u16) -> Self {
        Self {
            listen_addr,
            listen_port,
            capabilities: HashSet::default(),
        }
    }
}
