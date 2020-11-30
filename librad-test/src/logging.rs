// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use tracing_subscriber::{EnvFilter, FmtSubscriber};

/// Initialise logging / tracing
///
/// Note that this will capture logs, so they can be output as part of the test
/// output. Use `RUST_LOG` with care, as this may create unwanted memory
/// pressure. Note, however, that if `RUST_LOG` is not set, we set the level to
/// `error` by default in order to surface errors on CI.
pub fn init() {
    if env_logger::builder().is_test(true).try_init().is_ok() {
        if env::var("RUST_LOG").is_err() {
            env::set_var("RUST_LOG", "error");
        }

        let subscriber = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .with_target(true)
            .compact()
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting tracing default failed");
    }
}
