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

use std::{
    convert::TryFrom,
    fmt::{self, Display},
};

use crate::{
    git::trailer::Trailer,
    identities::{git::error, sign::Signatures},
};

impl<'a> TryFrom<&git2::Commit<'a>> for Signatures {
    type Error = error::Signatures;

    fn try_from(commit: &git2::Commit<'a>) -> Result<Self, Self::Error> {
        commit
            .message()
            .ok_or(error::Signatures::Utf8)
            .and_then(|msg| Signatures::from_trailers(msg).map_err(Self::Error::from))
    }
}

pub struct CommitMessage<'a> {
    body: &'a str,
    trailers: Vec<Trailer<'a>>,
}

impl<'a> CommitMessage<'a> {
    pub fn new(body: &'a str, signatures: &'a Signatures) -> Self {
        Self {
            body,
            trailers: signatures.into(),
        }
    }
}

impl Display for CommitMessage<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}\n\n", self.body)?;

        for (i, trailer) in self.trailers.iter().enumerate() {
            write!(f, "{}", trailer.display(": "))?;
            if i < self.trailers.len() {
                f.write_str("\n")?;
            }
        }

        Ok(())
    }
}
