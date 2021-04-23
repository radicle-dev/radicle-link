// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, ops::Deref};

use rand::{rngs::SmallRng, Rng as _, SeedableRng as _};
use zeroize::Zeroize;

use super::SecretKey;

use crate::peer::PeerId;

/// A Reduced Entropy SecretKey a.k.a a Risky SecretKey.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct RESKey {
    key: SecretKey,
    seed: [u8; 32],
}

impl RESKey {
    /// Produce a `RESKey` using [`SmallRng`]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut rng = SmallRng::from_entropy();
        let seed = rng.gen();
        Self::from_seed(seed)
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let key = SecretKey::from_seed(seed);
        Self { key, seed }
    }
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
    type Target = SecretKey;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}

impl AsRef<SecretKey> for RESKey {
    fn as_ref(&self) -> &SecretKey {
        &self.key
    }
}

impl From<&RESKey> for PeerId {
    fn from(risky: &RESKey) -> Self {
        PeerId::from(risky.key.clone())
    }
}

impl From<RESKey> for PeerId {
    fn from(risky: RESKey) -> Self {
        PeerId::from(risky.key.clone())
    }
}
