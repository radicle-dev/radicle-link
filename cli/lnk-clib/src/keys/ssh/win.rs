// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt;

use serde::{de::DeserializeOwned, Serialize};
use thrussh_agent::Constraint;

use librad::{
    crypto::{keystore::crypto::Crypto, BoxedSigner},
    profile::Profile,
};

/// Get the signing key associated with this `profile`.
///
/// See [`SshAuthSock`] for how the `ssh-agent` will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn signer(_profile: &Profile, _sock: SshAuthSock) -> Result<BoxedSigner, super::Error> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

/// Add the signing key associated with this `profile` to the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
///
/// The `crypto` passed will decide how the key storage is unlocked.
pub fn add_signer<C>(
    _profile: &Profile,
    _sock: SshAuthSock,
    _crypto: C,
    _constraints: &[Constraint],
) -> Result<(), super::Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

/// Remove the signing key associated with this `profile` from the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
///
/// The `crypto` passed will decide how the key storage is unlocked.
pub fn remove_signer<C>(
    _profile: &Profile,
    _sock: SshAuthSock,
    crypto: C,
) -> Result<(), super::Error>
where
    C: Crypto,
    C::Error: fmt::Debug + fmt::Display + Send + Sync + 'static,
    C::SecretBox: Serialize + DeserializeOwned,
{
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

/// Test whether the signing key associated with this `profile` is present on
/// the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn is_signer_present(_profile: &Profile, _sock: SshAuthSock) -> Result<bool, super::Error> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

/// Sign the `payload`, using the signing key associated with this `profile`,
/// through the `ssh-agent`.
///
/// See [`SshAuthSock`] for how the agent will be connected to. Use
/// `SshAuthSock::default` to connect via `SSH_AUTH_SOCK`.
pub fn sign(
    _profile: &Profile,
    _sock: SshAuthSock,
    _payload: &[u8],
) -> Result<sign::Signature, super::Error> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

/// Verify the `signature` for the given `payload`, using the public key
/// associated with this `profile`.
pub fn verify(
    _profile: &Profile,
    _payload: &[u8],
    _signature: &Signature,
) -> Result<bool, super::Error> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}
