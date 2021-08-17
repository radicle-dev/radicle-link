// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use librad::{PeerId, SecStr, SecretKey};
use radicle_keystore::sign::ed25519;

#[derive(Clone)]
pub struct Signer {
    pub(super) key: SecretKey,
}

impl From<Signer> for PeerId {
    fn from(signer: Signer) -> Self {
        signer.key.into()
    }
}

impl Signer {
    pub fn new<R: io::Read>(mut r: R) -> Result<Self, io::Error> {
        use radicle_keystore::SecretKeyExt;

        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes)?;

        let sbytes: SecStr = bytes.into();
        match SecretKey::from_bytes_and_meta(sbytes, &()) {
            Ok(key) => Ok(Self { key }),
            Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
        }
    }
}

#[async_trait]
impl ed25519::Signer for Signer {
    type Error = std::convert::Infallible;

    fn public_key(&self) -> ed25519::PublicKey {
        self.key.public_key()
    }

    async fn sign(&self, data: &[u8]) -> Result<ed25519::Signature, Self::Error> {
        <SecretKey as ed25519::Signer>::sign(&self.key, data).await
    }
}
