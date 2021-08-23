// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, str::FromStr};

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
#[derive(Debug, Clone, Copy)]
pub enum Format {
    /// Requires the data type to implement [`Serialize`].
    Json,
    /// Requires the data type to implement [`Encode`].
    Cbor,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Cbor => write!(f, "cbor"),
        }
    }
}

impl FromStr for Format {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(Self::Json),
            "cbor" => Ok(Self::Cbor),
            _ => Err("unknown format type"),
        }
    }
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
