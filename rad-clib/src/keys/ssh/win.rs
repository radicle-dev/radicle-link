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

pub fn signer(_profile: &Profile) -> Result<BoxedSigner, super::Error> {
    unimplemented!("Windows is not supported, contributions are welcome :)")
}

pub fn add_signer<C>(
    _profile: &Profile,
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
