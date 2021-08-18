// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_canonical::Cstring;
use proptest::prelude::*;
use unicode_normalization::UnicodeNormalization as _;

pub fn gen_cstring() -> impl Strategy<Value = Cstring> {
    ".*".prop_map(|s| Cstring::from(s.nfc().collect::<String>()))
}
