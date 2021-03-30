// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::info::PeerAdvertisement;

mod rpc;
pub use rpc::{Error, Request, Response};

pub mod xor;
pub use xor::Xor;
