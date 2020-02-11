use std::net::ToSocketAddrs;

use quinn::EndpointError;
use quinn_h3::{
    client::{Builder as ClientBuilder, Client},
    server::{Builder as ServerBuilder, IncomingConnection},
};

use crate::{keys::device, net::quic};

pub fn make_client(key: &device::Key) -> Result<Client, EndpointError> {
    ClientBuilder::with_quic_config(quinn::ClientConfigBuilder::new(quic::make_client_config(
        key,
    )))
    .build()
}

pub fn make_server<S: ToSocketAddrs>(
    key: &device::Key,
    listen_addrs: S,
) -> Result<IncomingConnection, EndpointError> {
    let mut builder = ServerBuilder::with_quic_config(quinn::ServerConfigBuilder::new(
        quic::make_server_config(key),
    ));
    builder
        .listen(listen_addrs)
        .map_err(EndpointError::Socket)?;

    // Note: there's nothing one can do with the first element of the tuple
    // (`Server`), so just drop it for now
    builder.build().map(|(_, incoming)| incoming)
}
