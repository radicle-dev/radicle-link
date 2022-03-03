// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use git_ref_format::{check_ref_format, Error, Options};
use proptest::prelude::*;

use crate::gen;

mod name;
mod pattern;

proptest! {
    #[test]
    fn disallow_onelevel(input in gen::trivial(), allow_pattern in any::<bool>()) {
        assert_matches!(
            check_ref_format(Options {
                    allow_onelevel: false,
                    allow_pattern,
                },
                &input
            ),
            Err(Error::OneLevel)
        )
    }
}
