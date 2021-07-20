// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{self, FutureExt as _},
    stream::{self, StreamExt as _},
    Stream,
};
use futures_timer::Delay;
use rand::Rng as _;

use super::{Hpv, Shuffle};
use crate::net::{protocol::info::PeerInfo, quic::MAX_IDLE_TIMEOUT};

pub enum Periodic<A> {
    RandomPromotion { candidates: Vec<PeerInfo<A>> },
    Shuffle(Shuffle<A>),
    Tickle,
}

#[tracing::instrument(skip(hpv))]
pub(super) fn periodic_tasks<Rng, Addr>(hpv: Hpv<Rng, Addr>) -> impl Stream<Item = Periodic<Addr>>
where
    Rng: rand::Rng + Clone + 'static,
    Addr: Clone + Debug + PartialEq + Send + Sync + 'static,
{
    let params = hpv.params();

    let shuffle = Interval::new(params.shuffle_interval, Duration::from_secs(5)).filter_map({
        let hpv = hpv.clone();
        move |_| {
            let p = hpv.shuffle().map(Periodic::Shuffle);
            if p.is_none() {
                tracing::warn!("nothing to shuffle");
            }
            future::ready(p)
        }
    });

    let promote =
        Interval::new(params.promote_interval, Duration::from_secs(5)).filter_map(move |_| {
            let candidates = hpv.choose_passive_to_promote();
            if candidates.is_empty() {
                tracing::warn!("nothing to promote");
                future::ready(None)
            } else {
                future::ready(Some(Periodic::RandomPromotion { candidates }))
            }
        });

    let tickle = Interval::new(MAX_IDLE_TIMEOUT.div_f32(2.0), Duration::from_secs(5))
        .filter_map(|_| future::ready(Some(Periodic::Tickle)));

    // Wrapping the `select` calls is the most effective to combine the three
    // interval streams into one. All other means (select macro, select_all)
    // incur significant overhead.
    stream::select(stream::select(promote, shuffle), tickle)
}

struct Interval {
    delay: Delay,
    duration: Duration,
    jitter: Duration,
}

impl Interval {
    fn new(duration: Duration, jitter: Duration) -> Self {
        Self {
            delay: Delay::new(duration),
            duration,
            jitter,
        }
    }
}

impl futures::Stream for Interval {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(()) = self.delay.poll_unpin(cx) {
            let mut rng = rand::thread_rng();
            let jitter = Duration::from_secs(rng.gen_range(0, self.jitter.as_secs()));
            let delay = if rng.gen() {
                self.duration.saturating_add(jitter)
            } else {
                self.duration.saturating_sub(jitter)
            };
            self.get_mut().delay.reset(delay);

            return Poll::Ready(Some(()));
        }

        Poll::Pending
    }
}
