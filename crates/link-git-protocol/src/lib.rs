// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[macro_use]
extern crate async_trait;

use bstr::ByteSlice as _;
use git_repository::protocol::transport::client;
use versions::Version;

pub mod fetch;
pub mod ls;
pub mod packwriter;
pub mod take;
pub mod transport;
pub mod upload_pack;

pub use fetch::{fetch, ObjectId, Ref};
pub use ls::ls_refs;
pub use packwriter::PackWriter;
pub use upload_pack::upload_pack;

fn remote_git_version(caps: &client::Capabilities) -> Option<Version> {
    let agent = caps.capability("agent").and_then(|cap| {
        cap.value()
            .and_then(|bs| bs.to_str().map(|s| s.to_owned()).ok())
    })?;
    Version::new(agent.strip_prefix("git/")?)
}
