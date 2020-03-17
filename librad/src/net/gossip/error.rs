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
