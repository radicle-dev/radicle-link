// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#[cfg(test)]
use std::{
    cell::RefCell,
    convert::Infallible,
    fmt::{self, Display},
    iter::Cycle,
    slice,
};

use secstr::{SecStr, SecUtf8};
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::sign;

use crate::{pinentry::Pinentry, Keystore, SecretKeyExt};

/// Pinentry which just yields the stored sequence of pins cyclicly.
pub struct PinCycle<'a>(RefCell<Cycle<slice::Iter<'a, SecUtf8>>>);

impl<'a> PinCycle<'a> {
    pub fn new(pins: &'a [SecUtf8]) -> Self {
        Self(RefCell::new(pins.iter().cycle()))
    }
}

impl<'a> Pinentry for PinCycle<'a> {
    type Error = Infallible;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error> {
        Ok(self.0.borrow_mut().next().unwrap().clone())
    }
}

pub fn default_passphrase() -> SecUtf8 {
    SecUtf8::from("asdf")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicKey(sign::PublicKey);

impl From<SecretKey> for PublicKey {
    fn from(sk: SecretKey) -> Self {
        PublicKey(sk.0.public_key())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SecretKey(sign::SecretKey);

#[derive(Debug)]
pub enum IntoSecretKeyError {
    InvalidSliceLength,
}

impl Display for IntoSecretKeyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidSliceLength => f.write_str("Invalid slice length"),
        }
    }
}

impl SecretKeyExt for SecretKey {
    type Metadata = ();
    type Error = IntoSecretKeyError;

    fn from_bytes_and_meta(bytes: SecStr, _metadata: &Self::Metadata) -> Result<Self, Self::Error> {
        sign::SecretKey::from_slice(bytes.unsecure())
            .map(SecretKey)
            .ok_or(IntoSecretKeyError::InvalidSliceLength)
    }

    fn metadata(&self) -> Self::Metadata {}
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

fn gen_keypair() -> (PublicKey, SecretKey) {
    let (pk, sk) = sign::gen_keypair();
    (PublicKey(pk), SecretKey(sk))
}

pub fn get_after_put<S>(mut store: S)
where
    S: Keystore<PublicKey = PublicKey, SecretKey = SecretKey, Metadata = ()>,
{
    let (pk, sk) = gen_keypair();

    store.put_key(sk.clone()).expect("Put failed");
    let res = store.get_key().expect("Get failed");

    assert!(sk == res.secret_key, "Secret keys don't match");
    assert!(pk == res.public_key, "Public keys don't match");
}

#[allow(unused_variables)]
pub fn put_twice<S>(mut store: S, expect_err: S::Error)
where
    S: Keystore<PublicKey = PublicKey, SecretKey = SecretKey, Metadata = ()>,
{
    let (_, sk) = gen_keypair();
    store.put_key(sk.clone()).expect("Put failed");
    assert!(matches!(store.put_key(sk), Err(expect_err)))
}

#[allow(unused_variables)]
pub fn get_empty<S>(store: S, expect_err: S::Error)
where
    S: Keystore<PublicKey = PublicKey, SecretKey = SecretKey, Metadata = ()>,
{
    assert!(matches!(store.get_key(), Err(expect_err)))
}

#[allow(unused_variables)]
pub fn passphrase_mismatch<S>(mut store: S, expect_err: S::Error)
where
    S: Keystore<PublicKey = PublicKey, SecretKey = SecretKey, Metadata = ()>,
{
    let (_, sk) = gen_keypair();
    store.put_key(sk).expect("Put failed");
    assert!(matches!(store.get_key(), Err(expect_err)))
}
