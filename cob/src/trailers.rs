// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_trailers::{OwnedTrailer, Token, Trailer};

use std::convert::{TryFrom, TryInto};

const AUTHOR_TRAILER_TOKEN: &str = "X-Rad-Author";

pub mod error {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum InvalidAuthorTrailer {
        #[error("no X-Rad-Author trailer")]
        NoTrailer,
        #[error("no value for X-Rad-Author")]
        NoValue,
        #[error("invalid git OID")]
        InvalidOid,
    }

    #[derive(Debug, Error)]
    pub enum InvalidSchemaTrailer {
        #[error("no X-Rad-Schema trailer")]
        NoTrailer,
        #[error("no value for X-Rad-Schema")]
        NoValue,
        #[error("invalid git OID")]
        InvalidOid,
    }
}

pub use error::{InvalidAuthorTrailer, InvalidSchemaTrailer};

pub struct AuthorCommitTrailer(git2::Oid);

impl AuthorCommitTrailer {
    pub fn oid(&self) -> git2::Oid {
        self.0
    }
}

impl From<git2::Oid> for AuthorCommitTrailer {
    fn from(oid: git2::Oid) -> Self {
        AuthorCommitTrailer(oid)
    }
}

impl TryFrom<&Trailer<'_>> for AuthorCommitTrailer {
    type Error = error::InvalidAuthorTrailer;

    fn try_from(Trailer { values, .. }: &Trailer<'_>) -> Result<Self, Self::Error> {
        let val = values.first().ok_or(error::InvalidAuthorTrailer::NoValue)?;
        let oid = git2::Oid::from_str(val).map_err(|_| error::InvalidAuthorTrailer::InvalidOid)?;
        Ok(AuthorCommitTrailer(oid))
    }
}

impl TryFrom<&Vec<Trailer<'_>>> for AuthorCommitTrailer {
    type Error = error::InvalidAuthorTrailer;

    fn try_from(trailers: &Vec<Trailer<'_>>) -> Result<Self, Self::Error> {
        trailers
            .iter()
            .find_map(|trailer| {
                if trailer.token == Token::try_from(AUTHOR_TRAILER_TOKEN).unwrap() {
                    Some(AuthorCommitTrailer::try_from(trailer))
                } else {
                    None
                }
            })
            .unwrap_or(Err(error::InvalidAuthorTrailer::NoTrailer))
    }
}

impl From<AuthorCommitTrailer> for Trailer<'_> {
    fn from(author: AuthorCommitTrailer) -> Self {
        Trailer {
            token: Token::try_from(AUTHOR_TRAILER_TOKEN).unwrap(),
            values: vec![author.0.to_string().into()],
        }
    }
}

const SCHEMA_TRAILER_TOKEN: &str = "X-Rad-Schema";

pub struct SchemaCommitTrailer(git2::Oid);

impl SchemaCommitTrailer {
    pub fn oid(&self) -> git2::Oid {
        self.0
    }

    pub fn from_trailers<'b, A, I>(
        trailers: I,
    ) -> Result<SchemaCommitTrailer, error::InvalidSchemaTrailer>
    where
        A: Into<Trailer<'b>>,
        I: IntoIterator<Item = A>,
    {
        trailers
            .into_iter()
            .find_map(|trailer| {
                let trailer = trailer.into();
                if trailer.token == Token::try_from(SCHEMA_TRAILER_TOKEN).unwrap() {
                    Some(SchemaCommitTrailer::try_from(&trailer))
                } else {
                    None
                }
            })
            .unwrap_or(Err(error::InvalidSchemaTrailer::NoTrailer))
    }
}

impl From<git2::Oid> for SchemaCommitTrailer {
    fn from(oid: git2::Oid) -> Self {
        SchemaCommitTrailer(oid)
    }
}

impl TryFrom<&Trailer<'_>> for SchemaCommitTrailer {
    type Error = error::InvalidSchemaTrailer;

    fn try_from(Trailer { values, .. }: &Trailer<'_>) -> Result<Self, Self::Error> {
        let val = values.first().ok_or(error::InvalidSchemaTrailer::NoValue)?;
        let oid = git2::Oid::from_str(val).map_err(|_| error::InvalidSchemaTrailer::InvalidOid)?;
        Ok(SchemaCommitTrailer(oid))
    }
}

impl TryFrom<&OwnedTrailer> for SchemaCommitTrailer {
    type Error = error::InvalidSchemaTrailer;

    fn try_from(trailer: &OwnedTrailer) -> Result<Self, Self::Error> {
        (&Trailer::from(trailer)).try_into()
    }
}

impl From<SchemaCommitTrailer> for Trailer<'_> {
    fn from(schema: SchemaCommitTrailer) -> Self {
        Trailer {
            token: Token::try_from(SCHEMA_TRAILER_TOKEN).unwrap(),
            values: vec![schema.0.to_string().into()],
        }
    }
}

impl TryFrom<Vec<Trailer<'_>>> for SchemaCommitTrailer {
    type Error = error::InvalidSchemaTrailer;

    fn try_from(trailers: Vec<Trailer<'_>>) -> Result<Self, Self::Error> {
        SchemaCommitTrailer::from_trailers(trailers)
    }
}

impl TryFrom<Vec<OwnedTrailer>> for SchemaCommitTrailer {
    type Error = error::InvalidSchemaTrailer;

    fn try_from(trailers: Vec<OwnedTrailer>) -> Result<Self, Self::Error> {
        let trailer_refs = trailers.iter().map(Trailer::from);
        SchemaCommitTrailer::from_trailers(trailer_refs)
    }
}
