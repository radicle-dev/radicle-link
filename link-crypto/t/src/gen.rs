// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, ops::Deref};

use proptest::prelude::*;
use rand::{rngs::SmallRng, SeedableRng as _};
use zeroize::Zeroize;

use link_crypto::{PeerId, PublicKey, SecretKey};

pub fn gen_peer_id() -> impl Strategy<Value = PeerId> {
    gen_secret_key().prop_map(PeerId::from)
}

pub fn gen_peers() -> impl Strategy<Value = (PeerId, Vec<PeerId>)> {
    gen_peer_id().prop_flat_map(move |local| {
        prop::collection::vec(gen_peer_id(), 1..20).prop_map(move |remotes| {
            (
                local,
                remotes
                    .into_iter()
                    .filter(|remote| *remote != local)
                    .collect(),
            )
        })
    })
}

pub fn gen_secret_key() -> impl Strategy<Value = RESKey> {
    any::<[u8; 32]>().prop_map(RESKey::from_seed)
}

pub fn gen_public_key() -> impl Strategy<Value = PublicKey> {
    gen_secret_key().prop_map(|sk| sk.public())
}

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
