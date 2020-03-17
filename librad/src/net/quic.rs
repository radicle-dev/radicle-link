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
