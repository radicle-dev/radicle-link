use std::{io, time::SystemTimeError};

use failure::Fail;

#[derive(Debug, Fail)]
pub enum Error<P: Fail> {
    #[fail(display = "The key already exists")]
    KeyExists,

    #[fail(display = "Key not found")]
    NoSuchKey,

    #[fail(display = "Unable to retrieve key: Invalid salt")]
    InvalidSalt,

    #[fail(display = "Unable to retrieve key: Invalid nonce")]
    InvalidNonce,

    #[fail(display = "Unable to retrieve key: Invalid key")]
    InvalidKey,

    #[fail(display = "Unable to retrieve key: Invalid passphrase")]
    InvalidPassphrase,

    #[fail(display = "Unable to retrieve key: Invalid creation timestamp")]
    InvalidCreationTimestamp,

    #[fail(display = "Refusing to store key: creation timestamp is before UNIX epoch")]
    BackwardsTime(#[fail(cause)] SystemTimeError),

    #[fail(display = "{}", 0)]
    IoError(io::Error),

    /*
    #[fail(display = "{}", 0)]
    SerdeError(serde_cbor::error::Error),
    */
    #[fail(display = "{}", 0)]
    PinentryError(P),
}

impl<T: Fail> From<io::Error> for Error<T> {
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}

/*
impl<T: Fail> From<serde_cbor::error::Error> for Error<T> {
    fn from(err: serde_cbor::error::Error) -> Self {
        Self::SerdeError(err)
    }
}
*/

impl<T: Fail> From<SystemTimeError> for Error<T> {
    fn from(err: SystemTimeError) -> Self {
        Self::BackwardsTime(err)
    }
}
