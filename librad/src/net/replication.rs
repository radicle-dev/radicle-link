// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Adapter for the replication backend.
//!
//! When the crate is compiled with the `replication-v3` feature, the
//! `link-replication` backend is enabled. Note that the types, even though
//! similarly named, are different, as are the semantics. While `replication-v3`
//! is being stabilised it should be possible to swap the backends for smoke
//! testing, provided the default parameters are used and the return types are
//! not inspected.

#[cfg(not(feature = "replication-v3"))]
mod v2;
#[cfg(not(feature = "replication-v3"))]
pub use v2::{error, Config, IdStatus, Mode, Replication, Success};

#[cfg(feature = "replication-v3")]
mod v3;
#[cfg(feature = "replication-v3")]
pub use v3::{error, Config, Replication, Success};
