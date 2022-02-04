// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::os::unix::net::UnixListener;

use anyhow::Result;

use super::Sockets;

pub fn env() -> Result<Option<Sockets>> {
    todo!()
}
