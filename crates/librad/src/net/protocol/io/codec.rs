// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use crate::net::{
    codec::CborCodec,
    protocol::{broadcast, membership},
};

pub type Codec<T> = CborCodec<T, T>;

pub type Gossip<T> = Codec<broadcast::Message<SocketAddr, T>>;
pub type Membership = Codec<membership::Message<SocketAddr>>;
