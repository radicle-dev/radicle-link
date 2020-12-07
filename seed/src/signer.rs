// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use radicle_keystore::sign::ed25519;

use librad::keys;

#[derive(Clone)]
pub struct Signer {
    pub(super) key: keys::SecretKey,
}

impl Signer {
    pub fn new<R: io::Read>(mut r: R) -> Result<Self, io::Error> {
        use radicle_keystore::SecretKeyExt;

        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes)?;

        let sbytes: keys::SecStr = bytes.into();
        match keys::SecretKey::from_bytes_and_meta(sbytes, &()) {
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
        <keys::SecretKey as ed25519::Signer>::sign(&self.key, data).await
    }
}

impl keys::AsPKCS8 for Signer {
    fn as_pkcs8(&self) -> Vec<u8> {
        self.key.as_pkcs8()
    }
}
