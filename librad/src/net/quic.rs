use std::sync::Arc;

use quinn;

use crate::{keys::device, net::tls};

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
