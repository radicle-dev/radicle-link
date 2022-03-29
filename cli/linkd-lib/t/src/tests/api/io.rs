// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use linkd_lib::api::{io, io::Transport as _, messages};
use proptest::{array::uniform3, prelude::*};

use crate::gen::{announce_response, request, request_pull_response};

proptest! {
    #[test]
    fn test_request_round_trip(requests in uniform3(request())) {
        with_async_transport(
            |mut left, mut right| async move  {
                let mut result = Vec::new();
                for request in &requests {
                    left.send_request(request.clone()).await.unwrap();
                }
                while result.len() < 3 {
                    let message = right.recv_request().await.unwrap();
                    result.push(message.unwrap());
                }
                drop(left);
                assert!(right.recv_request().await.unwrap().is_none());
                assert_eq!(requests.to_vec(), result);
            }
        )
    }

         #[test]
    fn test_response_round_trip_announce(responses in uniform3(announce_response())) {
        test_response_round_trip(&responses)
    }
        #[test]
    fn test_response_round_trip_request_pull(responses in uniform3(request_pull_response())) {
        test_response_round_trip(&responses)
    }
}

fn with_async_transport<
    F: FnOnce(io::SocketTransport, io::SocketTransport) -> FU,
    FU: futures::Future<Output = ()>,
>(
    f: F,
) {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let (left, right) = tokio::net::UnixStream::pair().unwrap();
            f(left.into(), right.into()).await
        })
}

fn test_response_round_trip<P>(responses: &[messages::Response<P>])
where
    P: messages::SendPayload + messages::RecvPayload + Clone + std::fmt::Debug + PartialEq,
{
    with_async_transport(|mut left, mut right| async move {
        let mut result = Vec::new();
        for response in responses {
            left.send_response(response.clone()).await.unwrap();
        }
        while result.len() < 3 {
            let message = right.recv_response::<P>().await.unwrap();
            result.push(message.unwrap());
        }
        drop(left);
        assert!(right.recv_response::<P>().await.unwrap().is_none());
        assert_eq!(responses.to_vec(), result);
    })
}
