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
    ops::Deref,
    str::{FromStr, Utf8Error},
};

use minicbor::{Decode, Decoder, Encode, Encoder};
use percent_encoding::{percent_decode_str, percent_encode, AsciiSet};
use regex::RegexSet;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use url::Url;

use crate::{
    hash::{self, Hash},
    peer::{self, PeerId},
};

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT_PERCENT_ENCODE_SET: &AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// https://url.spec.whatwg.org/#path-percent-encode-set
const PATH_PERCENT_ENCODE_SET: &AsciiSet = &FRAGMENT_PERCENT_ENCODE_SET
    .add(b'#')
    .add(b'?')
    .add(b'{')
    .add(b'}');

/// Protocol specifier in the context of a [`RadUrn`] or [`RadUrl`]
///
/// This pertains to the VCS backend, implying the native wire protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub enum Protocol {
    #[n(0)]
    Git,
    //Pijul,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::Git
    }
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

pub mod path {
    use super::*;
    #[derive(Debug, Error, PartialEq)]
    #[error("malformed path: {reasons:?}")]
    pub struct ParseError {
        pub reasons: Vec<&'static ViolatesRefFormat>,
    }

    #[derive(Debug, Error, PartialEq)]
    pub enum ViolatesRefFormat {
        #[error("ends with `.lock`")]
        EndsWithDotLock,

        #[error("starts with a dot (`.`)")]
        StartsWithDot,

        #[error("contains consecutive dots (`..`)")]
        ConsecutiveDots,

        #[error("contains control characters")]
        ControlCharacters,

        #[error("contains reserved characters (`~`, `^`, `:`, `?`, `*`, `[`, `\\`)")]
        ReservedCharacters,

        #[error("contains `@{{`")] // nb. double-brace is to escape format string
        AtOpenBrace,

        #[error("contains consecutive slashes (`//`)")]
        ConsecutiveSlashes,

        #[error("consists of only the `@` character")]
        OnlyAt,
    }
}

/// The path component of a [`RadUrn`]
///
/// A [`Path`] is also a valid git branch name (as specified in
/// `git-check-ref-format(1)`).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Path(String);

impl Path {
    /// Invalid characters and -sequences acc. to `git-check-ref-format(1)`
    const REF_FORMAT_RULES: [(&'static str, path::ViolatesRefFormat); 8] = [
        (r"\.lock$", path::ViolatesRefFormat::EndsWithDotLock),
        (r"^\.", path::ViolatesRefFormat::StartsWithDot),
        (r"\.\.", path::ViolatesRefFormat::ConsecutiveDots),
        (r"[[:cntrl:]]", path::ViolatesRefFormat::ControlCharacters),
        (r"[~^:?*\[\\]", path::ViolatesRefFormat::ReservedCharacters),
        (r"@[{]", path::ViolatesRefFormat::AtOpenBrace),
        (r"//", path::ViolatesRefFormat::ConsecutiveSlashes),
        (r"^@$", path::ViolatesRefFormat::OnlyAt),
    ];

    pub fn new() -> Self {
        Self(String::new())
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self(String::with_capacity(capacity))
    }

    pub fn empty() -> Self {
        Self::with_capacity(0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn parse<S: AsRef<str>>(s: S) -> Result<Self, path::ParseError> {
        Self::parse_str(s).map(Path)
    }

    pub fn join<S: AsRef<str>>(mut self, segment: S) -> Result<Self, path::ParseError> {
        let segment = Self::parse_str(segment)?;

        if !self.is_empty() {
            self.0.push('/')
        }
        self.0.push_str(&segment);

        Ok(self)
    }

    pub fn push(&mut self, other: Self) {
        if !other.is_empty() {
            if !self.is_empty() {
                self.0.push('/')
            }
            self.0.push_str(&other.0);
        }
    }

    pub fn deref_or_default(&self) -> &str {
        if self.is_empty() {
            "rad/id"
        } else {
            self.deref()
        }
    }

    #[allow(clippy::trivial_regex)]
    fn parse_str<S: AsRef<str>>(s: S) -> Result<String, path::ParseError> {
        lazy_static! {
            static ref RULES_RE: RegexSet =
                RegexSet::new(Path::REF_FORMAT_RULES.iter().map(|x| x.0)).unwrap();
        }

        let s = s.as_ref().trim_matches('/');
        let matches: Vec<&path::ViolatesRefFormat> = RULES_RE
            .matches(s)
            .iter()
            .map(|ix| &Self::REF_FORMAT_RULES[ix].1)
            .collect();

        if !matches.is_empty() {
            Err(path::ParseError { reasons: matches })
        } else {
            Ok(s.to_owned())
        }
    }
}

impl FromStr for Path {
    type Err = path::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Deref for Path {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl Encode for Path {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.str(&self.0)?;
        Ok(())
    }
}

impl<'de> Decode<'de> for Path {
    fn decode(d: &mut Decoder) -> Result<Self, minicbor::decode::Error> {
        let s = d.str()?;
        s.parse().or(Err(minicbor::decode::Error::Message(
            "path violates format rules",
        )))
    }
}

impl<'de> Deserialize<'de> for Path {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
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
/// use librad::{
///     hash::Hash,
///     uri::{Path, Protocol, RadUrn},
/// };
///
/// let urn = RadUrn::new(
///     Hash::hash(b"geez"),
///     Protocol::Git,
///     Path::parse("rad/issues/42").unwrap(),
/// );
///
/// assert_eq!(
///     "rad:git:hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/rad/issues/42",
///     urn.to_string()
/// )
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[cbor(array)]
pub struct RadUrn {
    #[n(0)]
    pub id: Hash,

    #[n(1)]
    pub proto: Protocol,

    #[n(2)]
    pub path: Path,
}

impl RadUrn {
    pub fn new(id: Hash, proto: Protocol, path: Path) -> Self {
        Self { id, proto, path }
    }

    pub fn into_rad_url(self, authority: PeerId) -> RadUrl {
        RadUrl {
            authority,
            urn: self,
        }
    }

    pub fn as_rad_url_ref<'a>(&'a self, authority: &'a PeerId) -> RadUrlRef<'a> {
        RadUrlRef {
            authority,
            urn: self,
        }
    }
}

impl Display for RadUrn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rad:{}:{}", self.proto.nss(), self.id)?;

        if !self.path.is_empty() {
            write!(
                f,
                "/{}",
                percent_encode(self.path.as_bytes(), PATH_PERCENT_ENCODE_SET)
            )?;
        }

        Ok(())
    }
}

pub mod rad_urn {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ParseError {
        #[error("missing {0}")]
        Missing(&'static str),

        #[error("invalid namespace identifier: {0}")]
        InvalidNID(String),

        #[error("invalid protocol: {0}")]
        InvalidProto(String),

        #[error("malformed path")]
        Path(#[from] path::ParseError),

        #[error("must be UTF8")]
        Utf8(#[from] Utf8Error),

        #[error("invalid encoding")]
        Encoding(#[from] multibase::Error),

        #[error("invalid hash")]
        Hash(#[from] hash::ParseError),
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
                let decoded = percent_decode_str(id_and_path).decode_utf8()?;
                let mut iter = decoded.splitn(2, '/');
                let id = iter
                    .next()
                    .ok_or_else(|| Self::Err::Missing("id"))
                    .and_then(|id| Hash::from_str(id).map_err(|e| e.into()))?;
                let path = match iter.next() {
                    None => Ok(Path::new()),
                    Some(path) => Path::parse(path),
                }?;

                Ok(Self { id, proto, path })
            })
    }
}

impl Serialize for RadUrn {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RadUrn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UrnVisitor;

        impl<'de> Visitor<'de> for UrnVisitor {
            type Value = RadUrn;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a RadUrn")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(UrnVisitor)
    }
}

/// A `RadUrl` is a URL with the scheme `rad://`.
///
/// The authority of a rad URL is a [`PeerId`], from which to retrieve the
/// `radicle-link` repository and branch identified by [`RadUrn`].
#[derive(Clone, Debug, PartialEq, Encode, Decode)]
#[cbor(array)]
pub struct RadUrl {
    #[n(0)]
    pub authority: PeerId,

    #[n(1)]
    pub urn: RadUrn,
}

impl RadUrl {
    pub fn as_ref(&self) -> RadUrlRef {
        RadUrlRef {
            authority: &self.authority,
            urn: &self.urn,
        }
    }
}

impl Display for RadUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

pub mod rad_url {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ParseError {
        #[error("missing {0}")]
        Missing(&'static str),

        #[error("invalid scheme: {0}")]
        InvalidScheme(String),

        #[error("invalid protocol: {0}")]
        InvalidProto(String),

        #[error("invalid PeerId")]
        PeerId(#[from] peer::conversion::Error),

        #[error("malformed path")]
        Path(#[from] path::ParseError),

        #[error("must be UTF8")]
        Utf8(#[from] Utf8Error),

        #[error("invalid encoding")]
        Encoding(#[from] multibase::Error),

        #[error("invalid hash")]
        Hash(#[from] hash::ParseError),

        #[error("malformed URL")]
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
            .and_then(|id| Hash::from_str(id).map_err(|e| e.into()))?;
        let path = path_segments.try_fold::<_, _, Result<Path, rad_url::ParseError>>(
            Path::new(),
            |buf, segment| {
                let decoded = percent_decode_str(segment).decode_utf8()?;
                buf.join(&*decoded).map_err(|e| e.into())
            },
        )?;

        Ok(Self {
            authority,
            urn: RadUrn { id, proto, path },
        })
    }
}

impl Serialize for RadUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RadUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UrlVisitor;

        impl<'de> Visitor<'de> for UrlVisitor {
            type Value = RadUrl;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a RadUrl")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                RadUrl::from_str(s).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(UrlVisitor)
    }
}

pub struct RadUrlRef<'a> {
    pub authority: &'a PeerId,
    pub urn: &'a RadUrn,
}

impl<'a> RadUrlRef<'a> {
    pub fn to_owned(&self) -> RadUrl {
        RadUrl {
            authority: self.authority.clone(),
            urn: self.urn.clone(),
        }
    }
}

impl<'a> Display for RadUrlRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad+{}://{}/{}/{}",
            self.urn.proto.nss(),
            self.authority.default_encoding(),
            self.urn.id,
            percent_encode(self.urn.path.as_bytes(), PATH_PERCENT_ENCODE_SET,).to_string()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use sodiumoxide::crypto::sign::Seed;

    use crate::{keys::SecretKey, peer::PeerId};

    use librad_test::roundtrip::*;

    const SEED: Seed = Seed([
        20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81,
        181, 134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
    ]);

    lazy_static! {
        static ref URN: RadUrn = RadUrn {
            id: Hash::hash(b"geez"),
            proto: Protocol::Git,
            path: Path::parse("rad/issues/42").unwrap(),
        };
        static ref URL: RadUrl = URN
            .clone()
            .into_rad_url(PeerId::from(SecretKey::from_seed(&SEED)));
    }

    #[test]
    fn test_urn_str() {
        str_roundtrip(URN.clone())
    }

    #[test]
    fn test_urn_cbor() {
        cbor_roundtrip(URN.clone())
    }

    #[test]
    fn test_url_example() {
        assert_eq!(
            "rad+git://hyduh7ymr5a1n7zo54iyix36dyqh3o84wbi95muirt7mbiobar3d9s/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/rad/issues/42",
            URL.to_string()
        )
    }

    #[test]
    fn test_url_str() {
        str_roundtrip(URL.clone())
    }

    #[test]
    fn test_url_cbor() {
        cbor_roundtrip(URL.clone())
    }

    #[test]
    fn test_empty_path_parses() {
        let path = Path::parse("").unwrap();
        assert_eq!(path, Path::empty())
    }

    #[test]
    fn test_path_ref_format_rules() {
        use path::ViolatesRefFormat::*;

        [
            (Path::parse("foo.lock"), &EndsWithDotLock),
            (Path::parse(".hidden"), &StartsWithDot),
            (Path::parse("banana/../../etc/passwd"), &ConsecutiveDots),
            (Path::parse("x~"), &ReservedCharacters),
            (Path::parse("lkas^d"), &ReservedCharacters),
            (Path::parse("what?"), &ReservedCharacters),
            (Path::parse("x[yz"), &ReservedCharacters),
            (Path::parse("\\WORKGROUP"), &ReservedCharacters),
            (Path::parse("C:"), &ReservedCharacters),
            (Path::parse("foo//bar"), &ConsecutiveSlashes),
            (Path::parse("@"), &OnlyAt),
            (Path::parse("ritchie\0"), &ControlCharacters),
        ]
        .iter()
        .for_each(|(res, err)| {
            assert_eq!(res, &Err(path::ParseError { reasons: vec![err] }));
        })
    }
}
