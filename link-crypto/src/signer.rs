// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//!

use std::error::Error;

use keystore::sign;

use crate::{keys, peer::PeerId};

/// A blanket trait over [`sign::Signer`] that can be shared safely among
/// threads.
/// NOTE: might be redundant since `.sign` became sync
pub trait Signer: sign::Signer + Send + Sync + dyn_clone::DynClone + 'static {
    fn sign_blocking(&self, data: &[u8]) -> Result<sign::Signature, <Self as sign::Signer>::Error> {
        self.sign(data)
    }
}

// Here be Dragons...

/// A boxed [`Error`] that is used as the associated `Error` type for
/// [`BoxedSigner`].
pub struct BoxedSignError {
    error: Box<dyn Error + Send + Sync + 'static>,
}

impl BoxedSignError {
    /// Turn any [`Error`] into a `BoxedSignError`.
    pub fn from_std_error<T>(other: T) -> Self
    where
        T: Error + Send + Sync + 'static,
    {
        BoxedSignError {
            error: Box::new(other),
        }
    }
}

impl std::fmt::Debug for BoxedSignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.error)
    }
}

impl std::fmt::Display for BoxedSignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for BoxedSignError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self)
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        Some(self)
    }
}

/// A dynamic [`Signer`] where the associated error is a [`BoxedSignError`].
/// This allows us to dynamically send around something that is a `Signer`. This
/// is important for `librad`'s `git::local::transport`.
pub struct BoxedSigner {
    signer: Box<dyn Signer<Error = BoxedSignError>>,
}

impl BoxedSigner {
    /// Create a new `BoxedSigner` from any [`Signer`].
    pub fn new<S>(signer: S) -> Self
    where
        S: Signer<Error = BoxedSignError>,
    {
        BoxedSigner {
            signer: Box::new(signer),
        }
    }

    pub fn peer_id(&self) -> PeerId {
        keys::PublicKey::from(self.signer.public_key()).into()
    }
}

impl Clone for BoxedSigner {
    fn clone(&self) -> Self {
        BoxedSigner {
            signer: dyn_clone::clone_box(&*self.signer),
        }
    }
}

impl Signer for BoxedSigner {
    fn sign_blocking(&self, data: &[u8]) -> Result<sign::Signature, <Self as sign::Signer>::Error> {
        self.signer.sign_blocking(data)
    }
}

#[async_trait]
impl sign::Signer for BoxedSigner {
    type Error = BoxedSignError;

    fn public_key(&self) -> sign::PublicKey {
        self.signer.public_key()
    }

    fn sign(&self, data: &[u8]) -> Result<sign::Signature, Self::Error> {
        self.signer.sign(data)
    }
}

impl Signer for keys::SecretKey {
    fn sign_blocking(&self, data: &[u8]) -> Result<sign::Signature, <Self as sign::Signer>::Error> {
        let sig = crate::SecretKey::sign(self, data);
        Ok(sign::Signature(sig.into()))
    }
}

impl From<keys::SecretKey> for BoxedSigner {
    fn from(key: keys::SecretKey) -> Self {
        Self::from(SomeSigner { signer: key })
    }
}

impl rustls::sign::SigningKey for BoxedSigner {
    fn choose_scheme(
        &self,
        offered: &[rustls::SignatureScheme],
    ) -> Option<Box<dyn rustls::sign::Signer>> {
        if offered
            .iter()
            .any(|s| matches!(s, rustls::SignatureScheme::ED25519))
        {
            Some(Box::new(self.clone()))
        } else {
            None
        }
    }

    fn algorithm(&self) -> rustls::internal::msgs::enums::SignatureAlgorithm {
        rustls::internal::msgs::enums::SignatureAlgorithm::ED25519
    }
}

impl rustls::sign::Signer for BoxedSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::TLSError> {
        self.sign_blocking(message)
            .map(|sign::Signature(sig)| Vec::from(sig))
            .map_err(|e| rustls::TLSError::General(e.to_string()))
    }

    fn get_scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ED25519
    }
}

/// An implementation of `sign::Signer` will have a concrete associated `Error`.
/// If we would like to use it as a `BoxedSigner` then we need to create an
/// implementation of `sign::Signer` which uses `BoxedSignError`.
///
/// We can do this generically over any `S` that implements `sign::Signer` with
/// and associated `Error` that implementat `std::error::Error`.
#[derive(Clone)]
pub struct SomeSigner<S> {
    pub signer: S,
}

impl<S: Signer + Clone> Signer for SomeSigner<S> {
    fn sign_blocking(&self, data: &[u8]) -> Result<sign::Signature, <Self as sign::Signer>::Error> {
        self.signer
            .sign_blocking(data)
            .map_err(BoxedSignError::from_std_error)
    }
}

impl<S> From<SomeSigner<S>> for BoxedSigner
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: Error + Send + Sync + 'static,
{
    fn from(other: SomeSigner<S>) -> Self {
        BoxedSigner::new(other)
    }
}

#[async_trait]
impl<S> sign::Signer for SomeSigner<S>
where
    S: sign::Signer + Clone + Send + Sync + 'static,
    S::Error: Error + Send + Sync + 'static,
{
    type Error = BoxedSignError;

    fn public_key(&self) -> sign::PublicKey {
        self.signer.public_key()
    }

    fn sign(&self, data: &[u8]) -> Result<sign::Signature, Self::Error> {
        sign::Signer::sign(&self.signer, data).map_err(BoxedSignError::from_std_error)
    }
}
