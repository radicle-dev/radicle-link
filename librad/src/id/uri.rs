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

use crate::id::entity::Error;
use multihash::{Multihash, Sha2_256};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RadicleUri {
    hash: Multihash,
}

impl RadicleUri {
    pub fn new(hash: Multihash) -> Self {
        Self { hash }
    }
    pub fn hash(&self) -> &Multihash {
        &self.hash
    }

    pub fn from_str(s: &str) -> Result<Self, Error> {
        if s.len() < 4 {
            return Err(Error::InvalidUri(s.to_owned()));
        }
        let prefix = &s[0..4];
        if prefix != "rad:" {
            return Err(Error::InvalidUri(s.to_owned()));
        }
        let bytes = bs58::decode((&s[4..]).as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
            .map_err(|_| Error::InvalidBufferEncoding(s.to_owned()))?;
        let hash =
            Multihash::from_bytes(bytes.to_owned()).map_err(|_| Error::InvalidUri(s.to_owned()))?;
        Ok(Self { hash })
    }
}

impl std::fmt::Display for RadicleUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("rad:")?;
        f.write_str(
            bs58::encode(&self.hash)
                .with_alphabet(bs58::alphabet::BITCOIN)
                .into_string()
                .as_ref(),
        )?;
        Ok(())
    }
}

lazy_static! {
    pub static ref EMPTY_HASH: Multihash = Sha2_256::digest(&[]);
    pub static ref EMPTY_URI: RadicleUri = RadicleUri::new(EMPTY_HASH.to_owned());
}
