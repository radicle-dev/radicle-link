// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use data::BoundedVec;
use minicbor::{Decode, Encode};
use typenum::U16;

use crate::peer::PeerId;

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Encode, Decode)]
#[repr(u8)]
pub enum Capability {
    #[n(0)]
    Reserved = 0,
}

pub type PeerInfo<Addr> = GenericPeerInfo<Addr, PeerAdvertisement<Addr>>;
pub type PartialPeerInfo<Addr> = GenericPeerInfo<Addr, Option<PeerAdvertisement<Addr>>>;

impl<Addr> PartialPeerInfo<Addr> {
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

impl<Addr> From<PeerInfo<Addr>> for PartialPeerInfo<Addr> {
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

impl<Addr> From<PartialPeerInfo<Addr>> for (PeerId, Vec<Addr>) {
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

impl<Addr> From<PeerInfo<Addr>> for (PeerId, Vec<Addr>) {
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

#[derive(Debug, Clone, PartialEq, Encode)]
#[cbor(array)]
pub struct GenericPeerInfo<Addr, T> {
    #[n(0)]
    pub peer_id: PeerId,

    #[n(1)]
    pub advertised_info: T,

    #[n(2)]
    pub seen_addrs: BoundedVec<U16, Addr>,
}

impl<Addr> GenericPeerInfo<Addr, PeerAdvertisement<Addr>> {
    pub fn addrs(&self) -> impl Iterator<Item = &Addr> {
        self.seen_addrs
            .iter()
            .chain(&self.advertised_info.listen_addrs)
    }
}

// XXX: derive fails to add the trait bound on Addr
impl<'__b777, Addr: minicbor::Decode<'__b777>, T: minicbor::Decode<'__b777>>
    minicbor::Decode<'__b777> for GenericPeerInfo<Addr, T>
{
    fn decode(
        __d777: &mut minicbor::Decoder<'__b777>,
    ) -> Result<GenericPeerInfo<Addr, T>, minicbor::decode::Error> {
        let mut peer_id: Option<PeerId> = None;
        let mut advertised_info: Option<T> = None;
        let mut seen_addrs: Option<BoundedVec<U16, Addr>> = None;
        if let Some(__len777) = __d777.array()? {
            for __i777 in 0..__len777 {
                match __i777 {
                    0 => peer_id = Some(minicbor::Decode::decode(__d777)?),
                    1 => advertised_info = Some(minicbor::Decode::decode(__d777)?),
                    2 => seen_addrs = Some(radicle_data::bounded::decode_truncate(__d777)?),
                    _ => __d777.skip()?,
                }
            }
        } else {
            let mut __i777 = 0;
            while minicbor::data::Type::Break != __d777.datatype()? {
                match __i777 {
                    0 => peer_id = Some(minicbor::Decode::decode(__d777)?),
                    1 => advertised_info = Some(minicbor::Decode::decode(__d777)?),
                    2 => seen_addrs = Some(radicle_data::bounded::decode_truncate(__d777)?),
                    _ => __d777.skip()?,
                }
                __i777 += 1
            }
            __d777.skip()?
        }
        Ok(GenericPeerInfo {
            peer_id: if let Some(x) = peer_id {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(
                    0,
                    "GenericPeerInfo::peer_id",
                ));
            },
            advertised_info: if let Some(x) = advertised_info {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(
                    1,
                    "GenericPeerInfo::advertised_info",
                ));
            },
            seen_addrs: if let Some(x) = seen_addrs {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(
                    2,
                    "GenericPeerInfo::seen_addrs",
                ));
            },
        })
    }
}
#[derive(Debug, Clone, PartialEq, Encode)]
#[cbor(array)]
pub struct PeerAdvertisement<Addr> {
    #[n(0)]
    pub listen_addrs: BoundedVec<U16, Addr>,

    #[n(2)]
    pub capabilities: BTreeSet<Capability>,
}

// XXX: derive fails to add the trait bound on Addr
impl<'__b777, Addr: minicbor::Decode<'__b777>> minicbor::Decode<'__b777>
    for PeerAdvertisement<Addr>
{
    fn decode(
        __d777: &mut minicbor::Decoder<'__b777>,
    ) -> Result<PeerAdvertisement<Addr>, minicbor::decode::Error> {
        let mut listen_addrs: Option<BoundedVec<U16, Addr>> = None;
        let mut capabilities: Option<BTreeSet<Capability>> = None;
        if let Some(__len777) = __d777.array()? {
            for __i777 in 0..__len777 {
                match __i777 {
                    0 => listen_addrs = Some(radicle_data::bounded::decode_truncate(__d777)?),
                    2 => capabilities = Some(minicbor::Decode::decode(__d777)?),
                    _ => __d777.skip()?,
                }
            }
        } else {
            let mut __i777 = 0;
            while minicbor::data::Type::Break != __d777.datatype()? {
                match __i777 {
                    0 => listen_addrs = Some(radicle_data::bounded::decode_truncate(__d777)?),
                    2 => capabilities = Some(minicbor::Decode::decode(__d777)?),
                    _ => __d777.skip()?,
                }
                __i777 += 1
            }
            __d777.skip()?
        }
        Ok(PeerAdvertisement {
            listen_addrs: if let Some(x) = listen_addrs {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(
                    0,
                    "PeerAdvertisement::listen_addrs",
                ));
            },
            capabilities: if let Some(x) = capabilities {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(
                    2,
                    "PeerAdvertisement::capabilities",
                ));
            },
        })
    }
}
impl<Addr> PeerAdvertisement<Addr> {
    pub fn new(listen_addr: Addr) -> Self {
        Self {
            listen_addrs: BoundedVec::singleton(listen_addr),
            capabilities: BTreeSet::default(),
        }
    }
}
