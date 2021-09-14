// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use log::{log_enabled, Level};
use tracing::subscriber::set_global_default as set_subscriber;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

/// Initialise logging / tracing
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
    if env_logger::builder().try_init().is_ok() {
        let mut builder = FmtSubscriber::builder()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
            )
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
