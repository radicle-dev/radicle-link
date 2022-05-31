// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io, path::Path};

pub use link_hooks::{
    hook::{self, Hook, Hooks, Notification, Process as _},
    Data,
    Track,
};
use tokio::process::Child;

use crate::paths::Paths;

pub const DATA: &str = "urn_changed";
pub const TRACK: &str = "tracking_changed";

/// Start the set of [`Hooks`] located under [`Paths::hooks_dir`].
///
/// Each hook must be a binary executable. If the executable deals with
/// changes to a [`crate::git::Urn`], then the hook should live under
/// the `hooks/urn_changed` directory. If the executable deals with
/// changes to a [`crate::git::tracking`], then the hook should live
/// under the `hooks/tracking_changed` directory.
///
/// # Usage
///
/// To start the processing of the hooks, call [`Hooks::run`] with a
/// [`futures::Stream`] of [`Notification`]s.
///
/// If the [`Notification`] is a [`Notification::Data`] then it will
/// be sent to all the `urn_changed` hooks.
///
/// If the [`Notification`] is a [`Notification::Track`] then it will
/// be sent to all the `tracking_changed` hooks.
pub async fn hooks(paths: &Paths, config: hook::Config) -> io::Result<Hooks<Child>> {
    let hooks_dir = paths.hooks_dir();
    let data_hooks = load(hooks_dir.join(DATA)).await?;
    let track_hooks = load(hooks_dir.join(TRACK)).await?;
    Ok(Hooks::new(config, data_hooks, track_hooks))
}

async fn load(dir: impl AsRef<Path>) -> io::Result<Vec<Hook<Child>>> {
    let dir = dir.as_ref();
    let mut hooks = Vec::new();
    for entry in fs::read_dir(dir)? {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_file() => {
                    hooks.push(Hook::spawn::<_, String>(entry.path(), None).await?)
                },
                Ok(file_type) => {
                    tracing::warn!(file_type = ?file_type, "skipping hook entry that is not a file")
                },
                Err(err) => {
                    tracing::warn!(directory = %dir.display(), err = %err, "skipping hook entry, could not resolve file type")
                },
            },
            Err(err) => {
                tracing::warn!(directory = %dir.display(), err = %err, "skipping hook entry")
            },
        }
    }

    Ok(hooks)
}
