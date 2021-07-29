// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    keys::{IntoSecretKeyError, PublicKey, SecretKey},
    profile::Profile,
    signer::BoxedSigner,
};
use radicle_keystore::{
    crypto::{Crypto, KdfParams, Pwhash, SecretBoxError},
    file,
    pinentry::Prompt,
    FileStorage,
    Keystore as _,
};

/// The filename for storing the secret key.
pub const LIBRAD_KEY_FILE: &str = "librad.key";

pub type Error = file::Error<SecretBoxError<std::io::Error>, IntoSecretKeyError>;

/// Create a [`Prompt`] for unlocking the key storage.
pub fn prompt() -> Pwhash<Prompt<'static>> {
    let prompt = Prompt::new("please enter your passphrase: ");
    Pwhash::new(prompt, KdfParams::recommended())
}

/// Create a [`Prompt`] for unlocking the key storage.
///
/// # Safety
///
/// The encryption of the file store will be weak but fast. So this not intended
/// for production use.
#[cfg(feature = "unsafe")]
pub fn unsafe_prompt() -> Pwhash<Prompt<'static>> {
    use radicle_keystore::crypto::KDF_PARAMS_TEST;

    let prompt = Prompt::new("please enter your passphrase: ");
    Pwhash::new(prompt, *KDF_PARAMS_TEST)
}

/// Create a [`FileStorage`] for [`librad::keys`].
pub fn file_storage<C>(profile: &Profile, crypto: C) -> FileStorage<C, PublicKey, SecretKey, ()>
where
    C: Crypto,
{
    FileStorage::new(&profile.paths().keys_dir().join(LIBRAD_KEY_FILE), crypto)
}

/// Get the signer from the file store, decrypting the secret key by asking for
/// a passphrase via a prompt.
pub fn signer_prompt(profile: &Profile) -> Result<BoxedSigner, Error> {
    let store = file_storage(profile, prompt());
    let key = store.get_key()?.secret_key;
    Ok(key.into())
}

/// Get the signer from the file store, decrypting the secret key by asking for
/// a passphrase via a prompt.
///
/// # Safety
///
/// The encryption of the file store will be weak but fast. So this not intended
/// for production use.
#[cfg(feature = "unsafe")]
pub fn unsafe_signer_prompt(profile: &Profile) -> Result<BoxedSigner, Error> {
    let store = file_storage(profile, unsafe_prompt());
    let key = store.get_key()?.secret_key;
    Ok(key.into())
}
