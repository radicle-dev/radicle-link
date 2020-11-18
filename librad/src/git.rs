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

pub mod fetch;
pub mod identities;
pub mod include;
pub mod local;
pub mod p2p;
pub mod refs;
pub mod replication;
pub mod storage;
pub mod tracking;
pub mod trailer;
pub mod types;

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
