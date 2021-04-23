// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

/// A Reduced Entropy SecretKey a.k.a a Risky SecretKey.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct RESKey {
    key: SecretKey,
    seed: [u8; 32],
}

impl fmt::Debug for RESKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RESKey")
            .field("key", &"***".to_string())
            .field("seed", &self.seed)
            .finish()
    }
}

impl fmt::Display for RESKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key)
    }
}

impl Deref for RESKey {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}
