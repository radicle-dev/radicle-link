// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

use minicbor::Encode;
use serde::Serialize;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Cbor(#[from] minicbor::encode::Error<std::io::Error>),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// An enumeration of the formats the CLI can output. Note that since any of
/// these formats can be used, the corresponding data type needs to implement
/// the required traits.
pub enum Format {
    /// Requires the data type to implement [`Serialize`].
    Json,
    /// Requires the data type to implement [`Encode`].
    Cbor,
}

impl Format {
    /// Serialize the `val` to a `String`.
    pub fn format<T>(&self, val: &T) -> Result<String, Error>
    where
        T: Serialize + Encode,
    {
        match self {
            Self::Json => Ok(serde_json::to_string(val)?),
            Self::Cbor => Ok(String::from_utf8(minicbor::to_vec(val)?)?),
        }
    }
}
