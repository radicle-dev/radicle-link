use std::{
    fmt::{self, Display},
    io,
};

use rpassword;
use secstr::SecUtf8;

#[derive(Debug)]
pub enum Error {
    Tty(io::Error),
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Tty(e) => write!(f, "{}", e),
        }
    }
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

impl<'a> keystore::Pinentry for Pinentry<'a> {
    type Error = Error;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error> {
        rpassword::read_password_from_tty(Some(self.0))
            .map(SecUtf8::from)
            .map_err(|e| e.into())
    }
}
