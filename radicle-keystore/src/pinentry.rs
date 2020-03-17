use std::{convert::Infallible, io};

use rpassword;
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
        rpassword::read_password_from_tty(Some(self.0)).map(SecUtf8::from)
    }
}
