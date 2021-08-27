// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use minicbor::{
    decode::{self, Decoder},
    encode::{self, Encoder, Write},
};

use ::url::Url;

pub struct Encode(pub Url);

impl From<Url> for Encode {
    fn from(url: Url) -> Self {
        Self(url)
    }
}

impl minicbor::Encode for Encode {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
        e.str(self.0.as_str()).map(|_| ())
    }
}

pub struct Decode(pub Url);

impl From<Decode> for Url {
    fn from(d: Decode) -> Self {
        d.0
    }
}

impl minicbor::Decode<'_> for Decode {
    fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
        let url = d.str()?;
        Url::try_from(url)
            .map_err(|_| decode::Error::Message("failed to parse Url"))
            .map(Decode)
    }
}
