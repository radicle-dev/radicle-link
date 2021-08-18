// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_trailers::Trailer;
use std::{
    convert::TryFrom,
    fmt::{self, Display},
};

use crate::identities::{git::error, sign::Signatures};

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
