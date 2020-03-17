// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{net::SocketAddr, sync::Arc};

use quinn::{self, Endpoint, EndpointError, Incoming};

use crate::{keys::device, net::tls};

pub async fn make_endpoint(
    key: &device::Key,
    listen_addr: SocketAddr,
) -> Result<(Endpoint, Incoming), EndpointError> {
    let mut builder = Endpoint::builder();
    builder.default_client_config(make_client_config(key));
    builder.listen(make_server_config(key));

    builder.bind(&listen_addr)
}

pub fn make_client_config(key: &device::Key) -> quinn::ClientConfig {
    let mut quic_config = quinn::ClientConfigBuilder::default().build();
    let tls_config = Arc::new(tls::make_client_config(key));
    quic_config.crypto = tls_config;

    quic_config
}

pub fn make_server_config(key: &device::Key) -> quinn::ServerConfig {
    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    let tls_config = Arc::new(tls::make_server_config(key));
    quic_config.crypto = tls_config;

    quic_config
}
