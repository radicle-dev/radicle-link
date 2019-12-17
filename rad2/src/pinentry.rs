use std::io;

use failure::Fail;
use rpassword;
use secstr::SecUtf8;

use librad::keys::storage;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "{}", 0)]
    Tty(io::Error),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Tty(err)
    }
}

pub struct Pinentry<'a>(&'a str);

impl<'a> Pinentry<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Pinentry(prompt)
    }
}

impl<'a> storage::Pinentry for Pinentry<'a> {
    type Error = Error;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error> {
        rpassword::read_password_from_tty(Some(self.0))
            .map(SecUtf8::from)
            .map_err(|e| e.into())
    }
}
