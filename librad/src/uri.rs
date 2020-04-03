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
    fmt::{self, Display},
    path::{Path, PathBuf},
    str::FromStr,
};

// FIXME: `Path`/`PathBuf` are poor types for use with `RadUrn` -- there's no
// equivalent to `OsStrExt::as_bytes()` on Windows.
use std::os::unix::ffi::OsStrExt;

use multibase::Base;
use multihash::Multihash;
use percent_encoding::percent_encode;
use url::Url;

use crate::peer::PeerId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Git,
    //Pijul,
}

impl Protocol {
    /// The "NSS" (namespace-specific string) of the [`Protocol`] in the context
    /// of a URN
    pub fn nss(&self) -> &str {
        match self {
            Self::Git => "git",
            //Self::Pijul => "pijul",
        }
    }

    pub fn from_nss(s: &str) -> Option<Self> {
        match s {
            "git" => Some(Self::Git),
            //"pijul" => Some(Self::Pijul),
            _ => None,
        }
    }
}

/// A `RadUrn` identifies a branch in a verifiable `radicle-link` repository,
/// where:
///
/// * The repository is named `id`
/// * The backend / protocol is [`Protocol`]
/// * The initial (parent-less) revision of an identity document (defined by
///   [`Verifier`]) has the content address `id`
/// * There exists a branch named `rad/id` pointing to the most recent revision
///   of the identity document
/// * There MAY exist a branch named `path`
///
/// The textual representation of a `RadUrn` is of the form:
///
/// ```text
/// 'rad' ':' MULTIBASE(<id>) '/' <path>
/// ```
///
/// where the preferred base is `z-base32`.
///
/// ```rust
/// use std::path::PathBuf;
/// use librad::uri::{RadUrn, Protocol};
///
/// let urn = RadUrn {
///     id: multihash::Blake2b256::digest("geez".as_bytes()),
///     proto: Protocol::Git,
///     path: PathBuf::from("rad/issues/42"),
/// };
///
/// assert_eq!(
///     "rad:git:hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/rad/issues/42",
///     urn.to_string()
/// )
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct RadUrn {
    pub id: Multihash,
    pub proto: Protocol,
    pub path: PathBuf,
}

impl RadUrn {
    pub fn into_rad_url(self, authority: PeerId) -> RadUrl {
        RadUrl {
            authority,
            urn: self,
        }
    }
}

impl Display for RadUrn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad:{}:{}",
            self.proto.nss(),
            multibase::encode(Base::Base32Z, &self.id)
        )?;

        let path = self.path.strip_prefix("/").unwrap_or(&self.path);
        if !path.as_os_str().is_empty() {
            write!(
                f,
                "/{}",
                percent_encode(path.as_os_str().as_bytes(), percent_encoding::CONTROLS)
            )?;
        }

        Ok(())
    }
}

pub mod rad_urn {
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ParseError {
        #[error("Missing {0}")]
        Missing(&'static str),

        #[error("Invalid namespace identifier: {0}")]
        InvalidNID(String),

        #[error("Invalid protocol: {0}")]
        InvalidProto(String),

        #[error("Invalid encoding")]
        Encoding(#[from] multibase::Error),

        #[error("Invalid multihash")]
        Hash(#[from] multihash::DecodeOwnedError),
    }
}

impl FromStr for RadUrn {
    type Err = rad_urn::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut components = s.split(':');

        let nid = components.next().ok_or(Self::Err::Missing("namespace"))?;
        if nid != "rad" {
            return Err(Self::Err::InvalidNID(nid.to_string()));
        }

        let proto = components
            .next()
            .ok_or_else(|| Self::Err::Missing("protocol"))
            .and_then(|proto| {
                Protocol::from_nss(proto).ok_or_else(|| Self::Err::InvalidProto(proto.to_string()))
            })?;

        components
            .next()
            .ok_or_else(|| Self::Err::Missing("id and path"))
            .and_then(|id_and_path| {
                let mut iter = id_and_path.splitn(2, '/');
                let id = iter
                    .next()
                    .ok_or_else(|| Self::Err::Missing("id"))
                    .and_then(|id| {
                        multibase::decode(id)
                            .map(|(_, bytes)| bytes)
                            .map_err(|e| e.into())
                    })
                    .and_then(|bytes| Multihash::from_bytes(bytes).map_err(|e| e.into()))?;
                let path = iter.next().map(PathBuf::from).unwrap_or_else(PathBuf::new);

                Ok(Self { id, proto, path })
            })
    }
}

/// A `RadUrl` is a URL with the scheme `rad://`.
///
/// The authority of a rad URL is a [`PeerId`], from which to retrieve the
/// `radicle-link` repository and branch identified by [`RadUrn`].
#[derive(Clone, Debug, PartialEq)]
pub struct RadUrl {
    authority: PeerId,
    urn: RadUrn,
}

impl RadUrl {
    // TODO: we should be able to open a `RadUrl` from local storage
    // pub fn open(&self) -> Result<impl Iterator<Item = Commit>, ??>
}

impl Display for RadUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad+{}://{}/{}",
            self.urn.proto.nss(),
            self.authority.default_encoding(),
            percent_encode(
                Path::new(&multibase::encode(Base::Base32Z, &self.urn.id))
                    .join(&self.urn.path)
                    .as_os_str()
                    .as_bytes(),
                percent_encoding::CONTROLS,
            )
            .to_string()
        )
    }
}

pub mod rad_url {
    use thiserror::Error;

    use crate::peer;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ParseError {
        #[error("Missing {0}")]
        Missing(&'static str),

        #[error("Invalid scheme: {0}")]
        InvalidScheme(String),

        #[error("Invalid protocol: {0}")]
        InvalidProto(String),

        #[error("Invalid PeerId")]
        PeerId(#[from] peer::conversion::Error),

        #[error("Invalid encoding")]
        Encoding(#[from] multibase::Error),

        #[error("Invalid multihash")]
        Hash(#[from] multihash::DecodeOwnedError),

        #[error("Malformed URL")]
        MalformedUrl(#[from] url::ParseError),
    }
}

impl FromStr for RadUrl {
    type Err = rad_url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;

        let mut scheme = url.scheme().split('+');
        let rad = scheme.next().ok_or_else(|| Self::Err::Missing("scheme"))?;
        if rad != "rad" {
            return Err(Self::Err::InvalidScheme(rad.to_string()));
        }
        let proto = scheme
            .next()
            .ok_or_else(|| Self::Err::Missing("+scheme"))
            .and_then(|proto| {
                Protocol::from_nss(proto).ok_or_else(|| Self::Err::InvalidProto(proto.to_string()))
            })?;

        let authority = PeerId::from_default_encoding(
            url.host_str()
                .ok_or_else(|| Self::Err::Missing("authority"))?,
        )?;

        let mut path_segments = url
            .path_segments()
            .ok_or_else(|| Self::Err::Missing("path"))?;
        let id = path_segments
            .next()
            .ok_or_else(|| Self::Err::Missing("id"))
            .and_then(|id| {
                multibase::decode(id)
                    .map(|(_, bytes)| bytes)
                    .map_err(|e| e.into())
            })
            .and_then(|bytes| Multihash::from_bytes(bytes).map_err(|e| e.into()))?;
        let path = path_segments.fold(PathBuf::new(), |buf, segment| buf.join(segment));

        Ok(Self {
            authority,
            urn: RadUrn { id, proto, path },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    use sodiumoxide::crypto::sign::Seed;

    use crate::{keys::device, peer::PeerId};

    const SEED: Seed = Seed([
        20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81,
        181, 134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
    ]);
    const CREATED_AT: u64 = 1_576_843_598;

    #[test]
    fn test_urn_roundtrip() {
        let urn = RadUrn {
            id: multihash::Blake2b256::digest("geez".as_bytes()),
            proto: Protocol::Git,
            path: PathBuf::from("rad/issues/42"),
        };

        assert_eq!(urn, urn.clone().to_string().parse().unwrap())
    }

    #[test]
    fn test_url_example() {
        let url = RadUrn {
            id: multihash::Blake2b256::digest("geez".as_bytes()),
            proto: Protocol::Git,
            path: PathBuf::from("rad/issues/42"),
        }
        .into_rad_url(PeerId::from(device::Key::from_seed(
            &SEED,
            UNIX_EPOCH
                .checked_add(Duration::from_secs(CREATED_AT))
                .unwrap(),
        )));

        assert_eq!(
            "rad+git://hyduh7ymr5a1n7zo54iyix36dyqh3o84wbi95muirt7mbiobar3d9s/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/rad/issues/42",
            url.to_string()
        )
    }

    #[test]
    fn test_url_roundtrip() {
        let url = RadUrn {
            id: multihash::Blake2b256::digest("geez".as_bytes()),
            proto: Protocol::Git,
            path: PathBuf::from("rad/issues/42"),
        }
        .into_rad_url(PeerId::from(device::Key::from_seed(
            &SEED,
            UNIX_EPOCH
                .checked_add(Duration::from_secs(CREATED_AT))
                .unwrap(),
        )));

        assert_eq!(url, url.clone().to_string().parse().unwrap())
    }
}
