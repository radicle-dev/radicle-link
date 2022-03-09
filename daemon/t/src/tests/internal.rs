// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::Duration;

use futures::{future, stream::StreamExt as _};
use radicle_daemon::{PeerEvent, RunConfig};
use test_helpers::logging;
use tokio::time::timeout;

use crate::common::Harness;

#[test]
fn can_observe_timers() -> Result<(), anyhow::Error> {
    logging::init();

    let mut harness = Harness::new();
    let mut alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    harness.enter(async move {
        let ticked = async_stream::stream! {
            loop { yield alice.events.recv().await }
        }
        .scan(0, |ticked, event| {
            let event = event.unwrap();
            if let PeerEvent::RequestTick = event {
                *ticked += 1;
            }

            future::ready(if *ticked >= 5 { None } else { Some(event) })
        })
        .collect::<Vec<_>>();
        tokio::pin!(ticked);
        timeout(Duration::from_secs(5), ticked).await?;

        Ok(())
    })
}
