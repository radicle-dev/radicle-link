// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[cfg(all(unix, target_os = "macos"))]
mod macos;

#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
