// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use radicle_git_ext::reference::check;

#[test]
fn disallow_onelevel() {
    assert_matches!(
        check::ref_format(
            check::Options {
                allow_onelevel: false,
                allow_pattern: false
            },
            "HEAD"
        ),
        Err(check::Error::OneLevel)
    )
}
