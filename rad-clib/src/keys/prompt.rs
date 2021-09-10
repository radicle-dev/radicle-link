// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use librad::{
    crypto::{
        keystore::{
            crypto::{KdfParams, Pwhash, SecretBoxError},
            file,
            pinentry::Prompt,
            Keystore as _,
        },
        BoxedSigner,
        IntoSecretKeyError,
    },
    profile::Profile,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    File(#[from] file::Error<SecretBoxError<std::io::Error>, IntoSecretKeyError>),
}

/// Create a [`Prompt`] for unlocking the key storage.
pub fn new() -> Pwhash<Prompt<'static>> {
    let prompt = Prompt::new("please enter your passphrase: ");
    Pwhash::new(prompt, KdfParams::recommended())
}

/// Create a [`Prompt`] for unlocking the key storage.
///
/// # Safety
///
/// The encryption of the file store will be weak but fast. So this not
/// intended for production use.
#[cfg(feature = "unsafe")]
pub fn unsafe_new() -> Pwhash<Prompt<'static>> {
    use librad::crypto::keystore::crypto::KDF_PARAMS_TEST;

    let prompt = Prompt::new("please enter your passphrase: ");
    Pwhash::new(prompt, *KDF_PARAMS_TEST)
}

/// Get the signer from the file store, decrypting the secret key by asking
/// for a passphrase via a prompt.
pub fn signer(profile: &Profile) -> Result<BoxedSigner, Error> {
    let store = super::file_storage(profile, new());
    let key = store.get_key()?.secret_key;
    Ok(key.into())
}

/// Get the signer from the file store, decrypting the secret key by asking
/// for a passphrase via a prompt.
///
/// # Safety
///
/// The encryption of the file store will be weak but fast. So this not
/// intended for production use.
#[cfg(feature = "unsafe")]
pub fn unsafe_signer(profile: &Profile) -> Result<BoxedSigner, Error> {
    let store = super::file_storage(profile, unsafe_new());
    let key = store.get_key()?.secret_key;
    Ok(key.into())
}
