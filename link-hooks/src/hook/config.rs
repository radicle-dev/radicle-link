// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    /// Configuration for the set of [`super::Hooks`]
    pub hook: Hook,
}

#[derive(Clone, Copy, Debug)]
pub struct Hook {
    /// The buffer size for the hook's internal channel.
    pub buffer: usize,
    /// The duration to wait for a hook to complete after the
    /// end-of-transmission message before it is forcefully killed.
    pub timeout: Duration,
}

impl Default for Hook {
    fn default() -> Self {
        Self {
            buffer: 10,
            timeout: Duration::from_secs(2),
        }
    }
}
