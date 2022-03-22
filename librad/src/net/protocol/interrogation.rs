// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use typenum::Unsigned as _;

use crate::identities::xor;

use super::info::PeerAdvertisement;

mod rpc;
pub use rpc::{Error, Request, Response};

pub const FRAMED_BUFSIZ: usize = xor::MaxFingerprints::USIZE * 3;
