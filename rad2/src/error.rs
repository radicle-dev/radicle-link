use std::fmt::Debug;
use std::io;

use failure::Fail;
use librad2::keys::storage;

#[derive(Debug, Fail)]
pub enum Error<S: Fail> {
    #[fail(display = "Error: {}", 0)]
    StorageError(storage::Error<S>),

    #[fail(display = "{}", 0)]
    IoError(io::Error),

    #[fail(display = "Empty key store! Create a key using `rad2 keys new`.")]
    EmptyKeyStore,
}

impl<S> From<storage::Error<S>> for Error<S>
where
    S: Fail,
{
    fn from(err: storage::Error<S>) -> Self {
        Error::StorageError(err)
    }
}

impl<S> From<io::Error> for Error<S>
where
    S: Fail,
{
    fn from(err: io::Error) -> Self {
        Error::IoError(err)
    }
}
