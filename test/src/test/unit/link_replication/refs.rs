// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_crypto::PeerId;
use once_cell::sync::Lazy;

mod parsed;
mod scoped;

static PEER: Lazy<PeerId> = Lazy::new(|| {
    "hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo"
        .parse()
        .unwrap()
});
