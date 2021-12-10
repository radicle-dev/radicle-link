// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub trait Metrics {
    type Snapshot;

    /// Record that a message has been received.
    ///
    /// The `hop_count` is `Some` unless the message is sent by an old client.
    fn record_message(&self, hop_count: Option<usize>);

    /// Record that the received message has been seen before (and was therefore
    /// ignored).
    fn record_seen(&self);

    fn snapshot(&self) -> Self::Snapshot;
}

impl Metrics for () {
    type Snapshot = ();

    fn record_message(&self, _: Option<usize>) {}
    fn record_seen(&self) {}

    fn snapshot(&self) -> Self::Snapshot {}
}
