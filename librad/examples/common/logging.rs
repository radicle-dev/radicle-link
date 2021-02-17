// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use log::{log_enabled, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

pub fn init() {
    if env_logger::builder().try_init().is_ok() {
        if env::var("RUST_LOG").is_err() {
            env::set_var("RUST_LOG", "info");
        }

        if log_enabled!(target: "librad", Level::Trace) {
            let subscriber = FmtSubscriber::builder()
                .with_env_filter(EnvFilter::from_default_env())
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("setting tracing default failed");
        } else {
            let subscriber = FmtSubscriber::builder()
                .with_env_filter(EnvFilter::from_default_env())
                .compact()
                .finish();

            tracing::subscriber::set_global_default(subscriber)
                .expect("setting tracing default failed");
        }
    }
}
