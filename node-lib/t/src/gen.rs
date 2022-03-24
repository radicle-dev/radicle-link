// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use link_crypto_test::gen::gen_peer_id;
use link_identities_test::gen::urn::{gen_oid, gen_urn};
use node_lib::{
    api::{announce, messages, request_pull},
    Seed,
};
use proptest::{collection, prelude::*};
use test_helpers::std_net::gen_socket_addr;

pub fn user_agent() -> impl Strategy<Value = messages::UserAgent> {
    any::<String>().prop_map(|s| s.into())
}

pub fn request_id() -> impl Strategy<Value = messages::RequestId> {
    any::<Vec<u8>>().prop_map(|s| s.into())
}

pub fn request_mode() -> impl Strategy<Value = messages::RequestMode> {
    prop_oneof! {
        Just(messages::RequestMode::ReportProgress),
        Just(messages::RequestMode::FireAndForget),
    }
}

pub fn announce() -> impl Strategy<Value = announce::Announce> {
    gen_oid(git2::ObjectType::Commit)
        .prop_flat_map(move |rev| gen_urn().prop_map(move |urn| announce::Announce { urn, rev }))
}

pub fn request_pull(addrs: Vec<SocketAddr>) -> impl Strategy<Value = request_pull::RequestPull> {
    gen_peer_id().prop_flat_map(move |peer| {
        (gen_urn(), Just(addrs.clone())).prop_map(move |(urn, addrs)| request_pull::RequestPull {
            urn,
            peer,
            addrs,
        })
    })
}

pub fn request_payload() -> impl Strategy<Value = messages::RequestPayload> {
    prop_oneof![
        announce().prop_map(messages::RequestPayload::from),
        collection::vec(gen_socket_addr(), 1..3)
            .prop_flat_map(move |addrs| request_pull(addrs))
            .prop_map(messages::RequestPayload::from)
    ]
}

prop_compose! {
    pub fn request()
        (user_agent in user_agent(),
         mode in request_mode(),
         payload in request_payload())
        -> messages::Request {
        messages::Request{
            user_agent,
            mode,
            payload,
        }

    }
}

pub fn response_payload() -> impl Strategy<Value = messages::ResponsePayload> {
    prop_oneof! {
        any::<String>().prop_map(messages::ResponsePayload::Progress),
        any::<String>().prop_map(messages::ResponsePayload::Error),
        Just(messages::ResponsePayload::Success),
    }
}

prop_compose! {
    pub fn response()
        (payload in response_payload(),
         id in request_id())
        -> messages::Response {
        messages::Response{
            payload,
            request_id: id,
        }
    }
}

pub fn seed() -> impl Strategy<Value = Seed<String>> {
    gen_peer_id().prop_map(move |peer| Seed {
        peer,
        addrs: "localhost".to_string(),
        label: None,
    })
}
