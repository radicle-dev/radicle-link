use std::io;

use futures_codec::CborCodecError;

use failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Invalid payload")]
    InvalidPayload(#[fail(cause)] serde_cbor::Error),

    #[fail(display = "Connection to self")]
    SelfConnection,

    #[fail(display = "Too many storage errors")]
    StorageErrorRateLimitExceeded,

    #[fail(display = "{}", 0)]
    Io(#[fail(cause)] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(err: CborCodecError) -> Self {
        match err {
            CborCodecError::Cbor(e) => Self::InvalidPayload(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}
