// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, sync::Arc};

use rustls::{ClientSession, ServerSession, Session};

use librad::{
    net::tls::{make_client_config, make_server_config},
    PeerId,
    SecretKey,
};

#[test]
fn test_can_handshake() {
    let client_key = SecretKey::new();
    let server_key = SecretKey::new();

    let server_id = PeerId::from(&server_key).to_string();

    let client_config = Arc::new(make_client_config(client_key).unwrap());
    let sni = webpki::DNSNameRef::try_from_ascii_str(&server_id).unwrap();
    let mut client_session = ClientSession::new(&client_config, sni);

    let server_config = Arc::new(make_server_config(server_key).unwrap());
    let mut server_session = ServerSession::new(&server_config);

    do_handshake(&mut client_session, &mut server_session)
}

fn do_handshake(client: &mut ClientSession, server: &mut ServerSession) {
    while server.is_handshaking() || client.is_handshaking() {
        transfer(client, server);
        server.process_new_packets().unwrap();
        transfer(server, client);
        client.process_new_packets().unwrap();
    }
}

fn transfer(left: &mut dyn Session, right: &mut dyn Session) {
    let mut buf = [0u8; 262_144];

    while left.wants_write() {
        let sz = {
            let into_buf: &mut dyn io::Write = &mut &mut buf[..];
            left.write_tls(into_buf).unwrap()
        };

        if sz == 0 {
            break;
        }

        let mut offs = 0;
        loop {
            let from_buf: &mut dyn io::Read = &mut &buf[offs..sz];
            offs += right.read_tls(from_buf).unwrap();
            if sz == offs {
                break;
            }
        }
    }
}
