// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting tracing default failed");
    }
}
