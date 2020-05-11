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

use std::{convert::Infallible, io};

use rpassword::read_password_from_tty;
use secstr::SecUtf8;

/// A method to obtain a passphrase from which an encryption key can be derived.
///
/// Similar in spirit to GPG's `pinentry` program, but no implementation of the
/// Assuan protocol is provided as of yet.
pub trait Pinentry {
    type Error;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error>;
}

/// Identity [`Pinentry`]
impl Pinentry for SecUtf8 {
    type Error = Infallible;

    fn get_passphrase(&self) -> Result<SecUtf8, Infallible> {
        Ok(self.clone())
    }
}

/// [`Pinentry`] which prompts the user on the TTY
pub struct Prompt<'a>(&'a str);

impl<'a> Prompt<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Self(prompt)
    }
}

impl<'a> Pinentry for Prompt<'a> {
    type Error = io::Error;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error> {
        read_password_from_tty(Some(self.0)).map(SecUtf8::from)
    }
}
