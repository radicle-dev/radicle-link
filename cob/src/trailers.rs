// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

mod author_commit {
    super::oid_trailer! {AuthorCommitTrailer, "X-Rad-Author"}
}
mod authorizing_identity {
    super::oid_trailer! {AuthorizingIdentityCommitTrailer, "X-Rad-Authorizing-Identity"}
}
mod schema_commit {
    super::oid_trailer! {SchemaCommitTrailer, "X-Rad-Schema"}
}

pub mod error {
    pub use super::author_commit::Error as InvalidAuthorTrailer;

    pub use super::schema_commit::Error as InvalidSchemaTrailer;

    pub use super::authorizing_identity::Error as InvalidAuthorizingIdentityTrailer;
}

pub use author_commit::AuthorCommitTrailer;
pub use authorizing_identity::AuthorizingIdentityCommitTrailer;
pub use schema_commit::SchemaCommitTrailer;

/// A macro for generating boilerplate From and TryFrom impls for trailers which
/// have git object IDs as their values
#[macro_export]
macro_rules! oid_trailer {
    ($typename:ident, $trailer:literal) => {
        use super::encode_oid;
        use git_trailers::{OwnedTrailer, Token, Trailer};
        use multihash::MultihashRef;
        use radicle_git_ext as ext;

        use std::convert::{TryFrom, TryInto};

        #[derive(Debug)]
        pub enum Error {
            NoTrailer,
            MultipleTrailers,
            NoValue,
            InvalidMultibase(multibase::Error),
            InvalidMultihash(multihash::DecodeError),
            FromMultiHash(radicle_git_ext::FromMultihashError),
            InvalidOid,
        }

        // We can't use `derive(thiserror::Error)` as we need to concat strings with
        // $trailer and macros are not allowed in non-key-value attributes
        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::MultipleTrailers => {
                        write!(f, concat!("multiple values found for ", $trailer))
                    },
                    Self::NoTrailer => write!(f, concat!("no ", $trailer)),
                    Self::NoValue => write!(f, concat!("no value for ", $trailer, " trailer")),
                    Self::InvalidMultibase(e) => e.fmt(f),
                    Self::InvalidMultihash(e) => e.fmt(f),
                    Self::FromMultiHash(e) => e.fmt(f),
                    Self::InvalidOid => write!(f, "invalid git OID"),
                }
            }
        }

        impl std::error::Error for Error {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match &self {
                    Self::InvalidMultibase(r) => Some(r),
                    Self::InvalidMultihash(r) => Some(r),
                    Self::FromMultiHash(r) => Some(r),
                    _ => None,
                }
            }
        }

        impl From<multibase::Error> for Error {
            fn from(e: multibase::Error) -> Self {
                Self::InvalidMultibase(e)
            }
        }

        impl From<multihash::DecodeError> for Error {
            fn from(e: multihash::DecodeError) -> Self {
                Self::InvalidMultihash(e)
            }
        }

        impl From<radicle_git_ext::FromMultihashError> for Error {
            fn from(e: radicle_git_ext::FromMultihashError) -> Self {
                Self::FromMultiHash(e)
            }
        }

        pub struct $typename(git2::Oid);

        impl $typename {
            pub fn oid(&self) -> git2::Oid {
                self.0
            }

            pub fn from_trailers<'b, A, I>(trailers: I) -> Result<$typename, Error>
            where
                A: Into<Trailer<'b>>,
                I: IntoIterator<Item = A>,
            {
                let matching_trailers: Vec<$typename> = trailers
                    .into_iter()
                    .filter_map(|trailer| {
                        let trailer = trailer.into();
                        if trailer.token == Token::try_from($trailer).ok()? {
                            Some($typename::try_from(&trailer))
                        } else {
                            None
                        }
                    })
                    .collect::<Result<Vec<$typename>, Error>>()?;
                match matching_trailers.len() {
                    0 => Err(Error::NoTrailer),
                    1 => Ok(matching_trailers.into_iter().next().unwrap()),
                    _ => Err(Error::MultipleTrailers),
                }
            }
        }

        impl From<git2::Oid> for $typename {
            fn from(oid: git2::Oid) -> Self {
                $typename(oid)
            }
        }

        impl From<$typename> for Trailer<'_> {
            fn from(containing: $typename) -> Self {
                Trailer {
                    token: Token::try_from($trailer).unwrap(),
                    values: vec![encode_oid(containing.0).into()],
                }
            }
        }

        impl TryFrom<Vec<Trailer<'_>>> for $typename {
            type Error = Error;

            fn try_from(trailers: Vec<Trailer<'_>>) -> Result<Self, Self::Error> {
                $typename::from_trailers(trailers)
            }
        }

        impl TryFrom<&Trailer<'_>> for $typename {
            type Error = Error;

            fn try_from(Trailer { values, .. }: &Trailer<'_>) -> Result<Self, Self::Error> {
                let val = values.first().ok_or(Error::NoValue)?;
                let (_, bytes) = multibase::decode(val)?;
                let mhash = MultihashRef::from_slice(&bytes)?;
                let ext_oid = radicle_git_ext::Oid::try_from(mhash)?;
                Ok($typename(ext_oid.into()))
            }
        }

        impl TryFrom<&OwnedTrailer> for $typename {
            type Error = Error;

            fn try_from(trailer: &OwnedTrailer) -> Result<Self, Self::Error> {
                (&Trailer::from(trailer)).try_into()
            }
        }

        impl TryFrom<Vec<OwnedTrailer>> for $typename {
            type Error = Error;

            fn try_from(trailers: Vec<OwnedTrailer>) -> Result<Self, Self::Error> {
                let trailer_refs = trailers.iter().map(Trailer::from);
                $typename::from_trailers(trailer_refs)
            }
        }

        impl TryFrom<&[OwnedTrailer]> for $typename {
            type Error = Error;

            fn try_from(trailers: &[OwnedTrailer]) -> Result<Self, Self::Error> {
                let trailer_refs = trailers.iter().map(Trailer::from);
                $typename::from_trailers(trailer_refs)
            }
        }

        impl From<ext::Oid> for $typename {
            fn from(oid: ext::Oid) -> Self {
                $typename(oid.into())
            }
        }
    };
}
pub(crate) use oid_trailer;

fn encode_oid(oid: git2::Oid) -> String {
    multibase::encode(
        multibase::Base::Base32Z,
        radicle_git_ext::Oid::from(oid).into_multihash(),
    )
}
