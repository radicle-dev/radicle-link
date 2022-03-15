// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub mod rpc;
pub use rpc::send_rpc;

pub mod request_response;
pub use request_response::{multi_response, request, single_response};
