// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{git::Urn, PublicKey};
use serde::Serialize;

#[non_exhaustive]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum Delegate {
    #[serde(rename = "direct")]
    Key(PublicKey),
    #[serde(rename = "indirect")]
    Urn(Urn),
}

impl From<PublicKey> for Delegate {
    fn from(key: PublicKey) -> Self {
        Self::Key(key)
    }
}

impl From<Urn> for Delegate {
    fn from(urn: Urn) -> Self {
        Self::Urn(urn)
    }
}

impl Delegate {
    pub fn direct(key: PublicKey) -> Self {
        key.into()
    }

    pub fn indirect(urn: Urn) -> Self {
        urn.into()
    }
}
