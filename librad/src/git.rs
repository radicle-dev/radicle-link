// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub mod identities;
pub mod include;
pub mod local;
pub mod p2p;
pub mod refs;

pub mod storage;
pub use storage::Storage;

pub mod tracking;
pub mod types;
pub mod util;

mod sealed;

pub use crate::identities::git::Urn;

/// Initialise the git backend.
///
/// **SHOULD** be called before all accesses to git functionality.
pub fn init() {
    use libc::c_int;
    use libgit2_sys as raw_git;
    use std::sync::Once;

    static INIT: Once = Once::new();

    unsafe {
        INIT.call_once(|| {
            let ret =
                raw_git::git_libgit2_opts(raw_git::GIT_OPT_SET_MWINDOW_FILE_LIMIT as c_int, 256);
            if ret < 0 {
                panic!(
                    "error setting libgit2 option: {}",
                    git2::Error::last_error(ret).unwrap()
                )
            }
        })
    }
}
