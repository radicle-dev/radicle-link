// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

pub struct Announce {
    pub rpc_socket_path: PathBuf,
}

pub struct Config<S> {
    pub paths: librad::paths::Paths,
    pub signer: S,
    pub addr: Option<SocketAddr>,
    pub linger_timeout: Option<Duration>,
    pub announce: Option<Announce>,
}
