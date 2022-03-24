// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_identities_test::gen::urn::{gen_oid, gen_urn};
use linkd_lib::api::messages;
use proptest::prelude::*;

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

prop_compose! {
    pub fn request_payload()
        (rev in gen_oid(git2::ObjectType::Commit),
         urn in gen_urn())
        -> messages::RequestPayload {
        messages::RequestPayload::Announce{
            rev: rev.into(),
            urn,
        }
    }
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
