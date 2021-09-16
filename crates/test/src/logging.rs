// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use log::{log_enabled, Level};
use tracing::subscriber::set_global_default as set_subscriber;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

/// Initialise logging / tracing
///
/// Note that this will capture logs, so they can be output as part of the test
/// output. Use `RUST_LOG` with care, as this may create unwanted memory
/// pressure. Note, however, that if `RUST_LOG` is not set, we set the level to
/// `error` by default in order to surface errors on CI.
///
/// The `TRACING_FMT` environment variable can be used to control the log
/// formatting. Supported values:
///
/// * "pretty": [`tracing_subscriber::fmt::format::Pretty`]
/// * "compact": [`tracing_subscriber::fmt::format::Compact`]
/// * "json": [`tracing_subscriber::fmt::format::Json`]
///
/// If the variable is not set, or set to any other value, the
/// [`tracing_subscriber::fmt::format::Full`] format is used.
pub fn init() {
    if env_logger::builder().is_test(true).try_init().is_ok() {
        if env::var("RUST_LOG").is_err() {
            env::set_var("RUST_LOG", "debug");
        }

        let mut builder = FmtSubscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .with_test_writer();
        if log_enabled!(target: "librad", Level::Trace) {
            builder = builder.with_thread_ids(true);
        } else if env::var("TRACING_FMT").is_err() {
            let default_format = if env::var("CI").is_ok() {
                "compact"
            } else {
                "pretty"
            };
            env::set_var("TRACING_FMT", default_format);
        }

        match env::var("TRACING_FMT").ok().as_deref() {
            Some("pretty") => set_subscriber(builder.pretty().finish()),
            Some("compact") => set_subscriber(builder.compact().finish()),
            Some("json") => set_subscriber(builder.json().flatten_event(true).finish()),
            _ => set_subscriber(builder.finish()),
        }
        .expect("setting tracing subscriber failed")
    }
}
