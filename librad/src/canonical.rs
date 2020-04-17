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

use serde::Serialize;
use thiserror::Error;

/// Types which have a canonical representation
pub trait Canonical {
    type Error;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct CjsonError(#[from] serde_json::error::Error);

/// The canonical JSON representation of type `T`
pub struct Cjson<T>(pub T);

impl<T> Cjson<T>
where
    T: Serialize,
{
    pub fn canonical_form(&self) -> Result<Vec<u8>, CjsonError> {
        let mut buf = vec![];
        let mut ser =
            serde_json::Serializer::with_formatter(&mut buf, olpc_cjson::CanonicalFormatter::new());
        self.0.serialize(&mut ser)?;
        Ok(buf)
    }
}

impl<T> Canonical for Cjson<T>
where
    T: Serialize,
{
    type Error = CjsonError;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        self.canonical_form()
    }
}
