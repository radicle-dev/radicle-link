// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use proptest::{array::uniform3, prelude::*};

use crate::node_lib::api::io::{request, response};

use node_lib::api::{io, io::Transport as _};

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
    fn test_response_round_trip(responses in uniform3(response())) {
        with_async_transport(|mut left, mut right| async move{
            let mut result = Vec::new();
            for response in &responses {
                left.send_response(response.clone()).await.unwrap();
            }
            while result.len() < 3 {
                let message = right.recv_response().await.unwrap();
                result.push(message.unwrap());
            }
            drop(left);
            assert!(right.recv_response().await.unwrap().is_none());
            assert_eq!(responses.to_vec(), result);
        })
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
