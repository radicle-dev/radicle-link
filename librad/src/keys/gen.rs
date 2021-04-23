// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use proptest::prelude::*;

use super::{risky::RESKey, PublicKey};

pub fn gen_secret_key() -> impl Strategy<Value = RESKey> {
    any::<()>().prop_map(|()| RESKey::new())
}

pub fn gen_public_key() -> impl Strategy<Value = PublicKey> {
    gen_secret_key().prop_map(|sk| sk.public())
}
