// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use minicbor::{
    decode::{self, Decoder},
    encode::{self, Encoder, Write},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Url(pub url::Url);

impl From<url::Url> for Url {
    fn from(url: url::Url) -> Self {
        Self(url)
    }
}

impl minicbor::Encode for Url {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
        e.str(self.0.as_str()).map(|_| ())
    }
}

impl From<Url> for url::Url {
    fn from(d: Url) -> Self {
        d.0
    }
}

impl minicbor::Decode<'_> for Url {
    fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
        let url = d.str()?;
        url::Url::try_from(url)
            .map_err(|_| decode::Error::Message("failed to parse Url"))
            .map(Url)
    }
}
