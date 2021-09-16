// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::sync::Arc;

use thiserror::Error;
use thrussh_agent::client::ClientStream;

use librad::{
    crypto::{
        keystore::{
            crypto::{Crypto, KdfParams, Pwhash, SecretBoxError},
            file,
            pinentry::Prompt,
            sign::ssh::{self, SshAgent},
            FileStorage,
            Keystore as _,
        },
        BoxedSigner,
        IntoSecretKeyError,
        SomeSigner,
    },
    git::storage::{self, ReadOnly},
    profile::Profile,
    PublicKey,
    SecretKey,
};

/// The filename for storing the secret key.
pub const LIBRAD_KEY_FILE: &str = "librad.key";

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    File(#[from] file::Error<SecretBoxError<std::io::Error>, IntoSecretKeyError>),
    #[error(transparent)]
    SshConnect(#[from] ssh::error::Connect),
    #[error(transparent)]
    StorageInit(#[from] storage::read::error::Init),
}

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
    use librad::crypto::keystore::crypto::KDF_PARAMS_TEST;

    let prompt = Prompt::new("please enter your passphrase: ");
    Pwhash::new(prompt, *KDF_PARAMS_TEST)
}

/// Create a [`FileStorage`] for [`SecretKey`]s.
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

pub async fn signer_ssh<S>(profile: &Profile) -> Result<BoxedSigner, Error>
where
    S: ClientStream + Unpin + 'static,
{
    let storage = ReadOnly::open(profile.paths())?;
    let peer_id = storage.peer_id();
    let agent = SshAgent::new((**peer_id).into());
    let signer = agent.connect::<S>().await?;
    Ok(SomeSigner {
        signer: Arc::new(signer),
    }
    .into())
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
