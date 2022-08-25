// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::net::protocol::info::{PeerAdvertisement, PeerInfo};

#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub enum Message<Addr> {
    #[n(0)]
    #[cbor(array)]
    Join {
        #[n(0)]
        info: PeerAdvertisement<Addr>,
    },

    #[n(1)]
    #[cbor(array)]
    ForwardJoin {
        #[n(0)]
        joined: PeerInfo<Addr>,
        #[n(1)]
        ttl: usize,
    },

    #[n(2)]
    #[cbor(array)]
    Neighbour {
        #[n(0)]
        info: PeerAdvertisement<Addr>,
        #[n(1)]
        prio: Priority,
    },

    #[n(3)]
    #[cbor(array)]
    Shuffle {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        peers: Vec<PeerInfo<Addr>>,
        #[n(2)]
        ttl: usize,
    },

    #[n(4)]
    #[cbor(array)]
    ShuffleReply {
        #[n(0)]
        peers: Vec<PeerInfo<Addr>>,
    },

    #[n(5)]
    #[cbor(array)]
    Disconnect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Priority {
    Normal,
    High,
}

// A joint brainfart of @kim and @twittner previously optimised away
// composability for the savings of 1 byte:
//
// `minicbor` prior to 0.9 would encode `()` as nothing at all, and so the
// priority was defined as `Option<()>` with `Some(())` denoting "high" and
// `None` denoting "normal".
//
// That is obviously stupid, and minicbor 0.9 makes a breaking change to rectify
// it. Unfortunately, we need to keep the previous encoding until enough peers
// are deployed which understand the more conventional `index_only` encoding.

impl minicbor::Encode for Priority {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        if let Priority::Normal = &self {
            e.null()?;
        }

        Ok(())
    }
}

impl<'b> minicbor::Decode<'b> for Priority {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        use minicbor::{data::Type, decode::Error};

        if Type::Null == d.datatype()? {
            d.skip()?;
            Ok(Self::Normal)
        } else if Type::U32 == d.datatype()? {
            match d.u32()? {
                0 => Ok(Self::Normal),
                1 => Ok(Self::High),
                x => Err(Error::UnknownVariant(x)),
            }
        } else {
            Ok(Self::High)
        }
    }
}
